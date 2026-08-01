"""MCP server for theses.cz — search Czech university theses and their metadata."""

import re
import sys

import requests
from bs4 import BeautifulSoup

try:  # mcp >= 2.0 renamed FastMCP to MCPServer; same constructor, .tool() and .run()
    from mcp.server.mcpserver import MCPServer as _Server
except ImportError:  # mcp 1.x
    from mcp.server.fastmcp import FastMCP as _Server

BASE = "https://theses.cz"
mcp = _Server("theses")

_s = requests.Session()
_s.headers["User-Agent"] = "theses-mcp (+https://github.com/koprjaa/theses-mcp)"


def _get(url: str) -> BeautifulSoup:
    """GET, working around the session bootstrap.

    The first request without a cookie returns a 117-byte stub with
    <meta http-equiv="refresh">; only the second one (now carrying
    __Host-issession) returns the real page.
    """
    for _ in range(2):
        html = _s.get(url, timeout=30).text
        if 'http-equiv="refresh"' not in html[:400]:
            break
    # lxml, not html.parser — theses.cz leaves <li> unclosed and html.parser nests them
    return BeautifulSoup(html, "lxml")


def _txt(node, *drop) -> str:
    """Node text without expand/collapse links and section labels."""
    if node is None:
        return ""
    if not hasattr(node, "select"):  # NavigableString
        return re.sub(r"\s+", " ", str(node)).strip(" \xa0()")
    node = node.__copy__()
    for sel in ("a.rozbal", "a.sbal", "h5", "h3") + drop:
        for x in node.select(sel):
            x.decompose()
    return re.sub(r"\s+", " ", node.get_text(" ", strip=True)).strip()


@mcp.tool()
def search(query: str, limit: int = 10) -> dict:
    """Search theses.cz (Czech national thesis registry) — full text and metadata.

    Returns two kinds of hits:
      kind="record"   — a thesis record; `url` points to the detail page (see `detail`)
      kind="fulltext" — a match inside the thesis PDF; `pdf_url` is a direct link to
                        the publicly readable full text

    query: search terms; supports theses.cz operators (AND, OR, "phrase")
    limit: how many results (paginated by 10, max 50)
    """
    out, total = [], None
    for start in range(1, min(limit, 50) + 1, 10):
        soup = _get(f"{BASE}/vyhledavani/?search={requests.utils.quote(query)}&start={start}")
        if total is None:
            m = re.search(r"Výsledky\s+\d+\s*–\s*\d+\s+z\s+([\d\s]+)", soup.get_text())
            total = int(re.sub(r"\D", "", m.group(1))) if m else None
        items = soup.select(".vyh_polozka")
        if not items:
            break
        for it in items:
            a = it.select_one("h4 a")
            if a is None:
                continue
            href = a["href"]
            fulltext = it.get("data-agenda") != "T"
            rec = it.select_one(".archiv a")
            hit = {
                "kind": "fulltext" if fulltext else "record",
                "title": _txt(a),
                "author": _txt(it.select_one("h4").next_sibling) or None,
                "meta": _txt(it.select_one(".vyh_hlavicky")),
                "snippet": _txt(it.select_one(".vyh_text")),
            }
            if fulltext:
                hit["pdf_url"] = href.split("?")[0]
                hit["url"] = BASE + rec["href"] if rec else None
            else:
                hit["url"] = BASE + href.split("?")[0]
            out.append(hit)
        if len(out) >= limit:
            break
    return {"total": total, "returned": len(out[:limit]), "results": out[:limit]}


@mcp.tool()
def detail(id_or_url: str) -> dict:
    """Fetch the full record of a thesis from theses.cz.

    id_or_url: thesis code ("pl09jx"), /id/pl09jx/ or a full URL.
    Returns author, title (incl. translation), CS/EN abstracts, keywords,
    supervisor/opponent, full-text availability and the school archive link.
    """
    code = id_or_url.strip("/ ").split("/")[-1]
    soup = _get(f"{BASE}/id/{code}/")
    meta = soup.select_one("#metadata")
    if meta is None:
        return {"error": f"record {code} not found"}

    anot = meta.select(".anotace")
    kw = {}
    for blok in meta.select(".klslova"):
        h3 = blok.select_one("h3")
        label = h3.get_text(strip=True) if h3 else "?"
        kw[label] = [s.get_text(strip=True) for s in blok.select("span.tg4")]

    servis = {}
    for col in meta.select(".servis_info > div"):
        t = _txt(col)
        if ":" in t:
            k, v = t.split(":", 1)
            servis[k.strip()] = v.strip()

    blok = meta.select_one("h3:-soup-contains('Obhajoba')")
    defense = [_txt(li) for li in blok.parent.select("ul li")] if blok else []

    plny = soup.select_one(".plny_text_th")
    ext = soup.select_one(".plny_text_ext a")
    return {
        "url": f"{BASE}/id/{code}/",
        "author": _txt(meta.select_one(".th-autor")),
        "type": _txt(meta.select_one(".typ-prace")),
        "title": _txt(meta.select_one(".th-title")),
        "title_translated": _txt(meta.select_one(".transl")),
        "abstract_cs": _txt(anot[0]) if anot else "",
        "abstract_en": _txt(anot[1]) if len(anot) > 1 else "",
        "keywords": kw,
        "defense": [x for x in defense if x],
        "language": servis.get("Jazyk práce"),
        "submitted": servis.get("Datum vytvoření / odevzdání či podání práce"),
        "school": _txt(soup.select_one("#th-sloupec .oddil")),
        "fulltext_access": _txt(plny),
        "archive_url": ext["href"] if ext else None,
        "related": [
            {"title": _txt(a), "url": BASE + a["href"]}
            for a in soup.select("#th-sloupec .oddil a[href^='/id/']")
        ],
    }


def _selftest():
    """Live check against theses.cz — the parsers break when the site markup changes."""
    r = search("midjourney", limit=12)
    assert r["total"] and r["total"] > 100, r["total"]
    assert len(r["results"]) == 12, len(r["results"])  # exercises pagination via start=11
    assert all(x["title"] for x in r["results"])
    assert any(x["kind"] == "fulltext" and x["pdf_url"] for x in r["results"])
    rec = next(x for x in r["results"] if x["kind"] == "record")
    d = detail(rec["url"])
    assert d["author"] and d["title"] and d["abstract_cs"], d
    assert d["keywords"] and d["archive_url"] and d["related"], d
    d2 = detail("pl09jx")  # unclosed <li> must not collapse into a single entry
    assert d2["defense"] == ["Obhajoba proběhla 12. 6. 2023", "Vedoucí: Jiří Korčák",
                             "Oponent: Tomáš Sigmund"], d2["defense"]
    print("OK", r["total"], "hits |", d["author"], "|", d["type"])


def main():
    _selftest() if "--selftest" in sys.argv else mcp.run()


if __name__ == "__main__":
    main()
