"""End-to-end check of every institution registered with theses.cz.

For each school it runs the whole chain a user would: find a thesis by that school,
open the record, resolve the files, then download one and look at the bytes. Nothing
here trusts a lookup that merely returned a URL — a school counts as working only when
a real PDF lands on disk.

    python tools/e2e_schools.py [--out DIR] [--candidates N] [--only SUBSTRING]

Writes the PDFs to DIR (default tools/e2e_out), a row per school to stdout, and the
full result to DIR/results.json.
"""

import argparse
import json
import logging
import pathlib
import re
import sys
import time
import unicodedata
import urllib.parse
import warnings

warnings.filterwarnings("ignore")
logging.getLogger("urllib3").setLevel(logging.CRITICAL)
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

import theses_mcp as th  # noqa: E402

MAX_BYTES = 30 * 1024 * 1024
SCHOOL_IN_HEADER = re.compile(r"práce[^,]*,\s*(.+?),\s*(\d{4})")


def fold(s):
    s = unicodedata.normalize("NFKD", (s or "").lower())
    return re.sub(r"[^a-z0-9 ]", "", "".join(c for c in s if not unicodedata.combining(c)))


def distinctive(name):
    """Words worth matching on — drop the ones half the country shares."""
    noise = {"v", "a", "praze", "brne", "sro", "ops", "as", "zu", "vysoka", "skola",
             "univerzita", "univerzity", "vyssi", "odborna", "ceske", "ceskych"}
    return [w for w in fold(name).split() if w not in noise and len(w) > 3]


def schools():
    soup = th._get(f"{th.BASE}/zapojeni")
    out = {}
    for a in soup.select('a[href^="/spravci?fak="]'):
        code = a["href"].split("fak=")[1]
        prev = a.find_previous(string=lambda x: x and x.strip() and "Správci" not in x)
        out[code] = (prev.strip() if prev else "?")[:80]
    return out


def queries(name, want):
    """Ways to ask for a school, cheapest and most exact first.

    The quoted full name misses the long ones. "Obchodní akademie, Střední odborná
    škola knihovnická a Vyšší odborná škola Brno" matches nothing as a phrase, so fall
    back to the words that actually identify the school.
    """
    forms = [f'"{name}"']
    if want:
        forms.append(" ".join(want[:3]))
        forms.append(want[0])
    seen = set()
    return [f for f in forms if not (f in seen or seen.add(f))]


def candidates(name, want):
    """Codes of theses whose result header names this school."""
    found = []
    for query in queries(name, want):
        for start in (1, 11, 21):
            try:
                soup = th._get(f"{th.BASE}/vyhledavani/"
                               f"?search={urllib.parse.quote(query)}&start={start}")
            except Exception:
                break
            rows = soup.select(".vyh_polozka")
            if not rows:
                break
            for it in rows:
                if it.get("data-agenda") != "T":
                    continue
                m = SCHOOL_IN_HEADER.search(th._txt(it.select_one(".vyh_hlavicky")))
                got = fold(m.group(1)) if m else ""
                if want and not all(w in got for w in want[:2]):
                    continue
                a = it.select_one("h4 a")
                cm = re.match(r"/id/(\w+)/", a["href"]) if a else None
                if cm and cm.group(1) not in found:
                    found.append(cm.group(1))
            if len(found) >= 6:
                return found
        if found:
            return found
    return found


def fetch(url, target):
    """Download and report what actually arrived."""
    got = 0
    head = b""
    with th._s.get(url, timeout=120, stream=True) as r:
        if r.status_code != 200:
            return {"ok": False, "why": f"HTTP {r.status_code}"}
        with target.open("wb") as fh:
            for chunk in r.iter_content(65536):
                if not head:
                    head = chunk[:8]
                fh.write(chunk)
                got += len(chunk)
                if got > MAX_BYTES:
                    break
    if not head.startswith(b"%PDF"):
        target.unlink(missing_ok=True)
        return {"ok": False, "why": f"not a PDF (starts {head[:8]!r})", "bytes": got}
    return {"ok": True, "bytes": got, "file": target.name}


def run_school(code, name, out_dir, tries):
    row = {"code": code, "school": name, "stage": "search", "detail": ""}
    picks = candidates(name, distinctive(name))
    if not picks:
        row["detail"] = "no thesis found whose header names this school"
        return row

    row["sampled"] = len(picks[:tries])
    last = ""
    for thesis in picks[:tries]:
        rec = th.detail(thesis)
        if rec.get("error"):
            last = rec["error"]
            continue
        row["stage"] = "detail"
        res = th.fulltext(thesis)
        row["thesis"] = thesis
        row["archive"] = res.get("archive_url")
        row["access"] = (res.get("access") or ["?"])[0]
        files = res.get("files", [])
        if not files:
            last = res.get("note", "no files")
            continue
        row["stage"] = "files"
        row["files"] = len(files)
        pdfs = [f for f in files if f["filename"].lower().endswith(".pdf")] or files
        target = out_dir / f"{code}_{thesis}_{re.sub(r'[^A-Za-z0-9._-]', '_', pdfs[0]['filename'])[:60]}"
        if not target.suffix:
            target = target.with_suffix(".pdf")
        try:
            got = fetch(pdfs[0]["url"], target)
        except Exception as e:
            last = f"download failed: {type(e).__name__}"
            continue
        if got["ok"]:
            row.update(stage="pdf", bytes=got["bytes"], file=got["file"], detail="")
            return row
        last = got["why"]
    row["detail"] = last
    return row


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(pathlib.Path(__file__).parent / "e2e_out"))
    ap.add_argument("--candidates", type=int, default=3)
    ap.add_argument("--only", default="")
    ap.add_argument("--pause", type=float, default=1.0,
                    help="seconds between schools, to stay under the rate limit")
    ap.add_argument("--cooldown", type=int, default=60,
                    help="seconds to wait before retrying a school that errored")
    args = ap.parse_args()

    out_dir = pathlib.Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    todo = {k: v for k, v in schools().items() if fold(args.only) in fold(v)}
    print(f"{len(todo)} institutions, up to {args.candidates} theses each\n", flush=True)

    rows = []
    for code, name in todo.items():
        row = None
        for attempt in (1, 2):
            try:
                row = run_school(code, name, out_dir, args.candidates)
                break
            except Exception as e:
                # theses.cz starts refusing the pace part way through a long run; the
                # schools at the end of the alphabet must not inherit that as a verdict
                if attempt == 1:
                    print(f"     {name[:34]} hit {type(e).__name__}, pausing {args.cooldown}s",
                          flush=True)
                    time.sleep(args.cooldown)
                else:
                    row = {"code": code, "school": name, "stage": "crash",
                           "detail": type(e).__name__}
        rows.append(row)
        time.sleep(args.pause)
        mark = {"pdf": "PDF ", "files": "list", "detail": "meta", "search": "----",
                "crash": "!!!!"}[row["stage"]]
        size = f"{row.get('bytes', 0) / 1024:.0f} kB" if row["stage"] == "pdf" else ""
        print(f"{mark} {name[:40]:42s} {size:9s} {row.get('detail', '')[:44]}", flush=True)

    (out_dir / "results.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1),
                                          encoding="utf-8")
    done = [r for r in rows if r["stage"] == "pdf"]
    print(f"\nPDF downloaded for {len(done)} of {len(rows)} institutions")
    print(f"files and results.json in {out_dir}")


if __name__ == "__main__":
    main()
