#
# Project: theses-mcp
# File:    theses_mcp.py
#
# Description:
# MCP server for theses.cz: searches Czech university theses, reads their metadata, and lists their documents.
#
# Author:
# Jan Alexandr Kopřiva
# jan.alexandr.kopriva@gmail.com
#
# License: MIT
#

"""MCP server for theses.cz. Search Czech university theses and their metadata."""

import json
import os
import pathlib
import re
import sys
import time

import requests
from bs4 import BeautifulSoup
from requests.adapters import HTTPAdapter
from urllib3.util import Retry

try:  # mcp >= 2.0 renamed FastMCP to MCPServer; same constructor, .tool() and .run()
    from mcp.server.mcpserver import MCPServer as _Server
except ImportError:  # mcp 1.x
    from mcp.server.fastmcp import FastMCP as _Server

BASE = "https://theses.cz"
mcp = _Server("theses")

_s = requests.Session()
_s.headers["User-Agent"] = "theses-mcp (+https://github.com/koprjaa/theses-mcp)"
# school systems drop connections and rate-limit; back off instead of failing the call
_s.mount("https://", HTTPAdapter(max_retries=Retry(
    # 500 is left out on purpose: repositories answer it for queries they cannot parse,
    # and repeating those just burns half a minute before the same failure
    total=3, backoff_factor=1.5, status_forcelist=[429, 502, 503, 504],
    allowed_methods=["GET", "HEAD"])))


def _load_cookies() -> list:
    """Attach the user's own logged-in sessions from $THESES_COOKIES.

    Many theses are marked "všem autentizovaným" (any authenticated user) or are held by
    a school whose repository asks anonymous visitors for a CAPTCHA. Both open up once
    you are logged in as yourself. Log in through the browser, copy the session cookie
    and pass it per host — cookies are scoped to their own domain, never sent elsewhere:

        THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "…"}'
    """
    raw = os.environ.get("THESES_COOKIES", "").strip()
    if not raw:
        return []
    try:
        mapping = json.loads(raw)
    except json.JSONDecodeError:
        print("THESES_COOKIES is not valid JSON — ignored", file=sys.stderr)
        return []
    for host, blob in mapping.items():
        for part in blob.split(";"):
            if "=" in part:
                name, value = part.split("=", 1)
                _s.cookies.set(name.strip(), value.strip(), domain=host)
    return list(mapping)


AUTHENTICATED = _load_cookies()

# where `login` keeps the sessions it collects, so a restart does not ask again
STORE = pathlib.Path.home() / ".theses-mcp"
COOKIE_FILE = STORE / "cookies.json"


def _attach(host: str, cookies: list) -> None:
    for c in cookies:
        _s.cookies.set(c["name"], c["value"], domain=c.get("domain", host).lstrip("."))
    if host not in AUTHENTICATED:
        AUTHENTICATED.append(host)


def _load_saved() -> None:
    """Restore sessions collected by `login` on an earlier run."""
    if not COOKIE_FILE.exists():
        return
    try:
        saved = json.loads(COOKIE_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return
    for host, cookies in saved.items():
        _attach(host, cookies)


_load_saved()


REFRESH = re.compile(r'http-equiv="refresh"[^>]*content="(\d+)', re.I)


def _get(url: str) -> BeautifulSoup:
    """GET, working around the session bootstrap.

    A request the server does not want to answer yet comes back as a 117-byte stub
    holding <meta http-equiv="refresh" content="N">. Retrying instantly just earns
    another stub, so honour the delay the page asks for. This is not only the first
    request of a session: theses.cz falls back to the stub under load as well.
    """
    for _ in range(4):
        r = _s.get(url, timeout=30)
        stub = REFRESH.search(r.text[:400])
        if not stub:
            break
        time.sleep(min(int(stub.group(1)), 3) or 1)
    # lxml, not html.parser — theses.cz leaves <li> unclosed and html.parser nests them
    soup = BeautifulSoup(r.text, "lxml")
    # keep the post-redirect URL: hdl.handle.net hands off to the real repository host,
    # and relative links must resolve against that, not against the handle
    soup.final_url = r.url
    return soup


def _txt(node, *drop) -> str:
    """Node text without expand/collapse links and section labels."""
    if node is None:
        return ""
    if not hasattr(node, "select"):  # NavigableString
        return re.sub(r"\s+", " ", str(node)).strip(" \xa0()")
    node = node.__copy__()
    for sel in ("a.rozbal", "a.sbal", "h5", "h3", *drop):
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
            # The en dash is what the page prints, so it is load-bearing here.
            m = re.search(r"Výsledky\s+\d+\s*–\s*\d+\s+z\s+([\d\s]+)", soup.get_text())  # noqa: RUF001
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


# The IS template carries "Odhlášení ze systému" even for anonymous visitors, so a logout
# link proves nothing. The invitation to log in is what disappears once you have.
SIGNED_OUT = ("Přihlásit se", "Přihlásit sa", "Log in", "Login")
CHROMIUM_BROWSERS = ("brave", "chrome", "msedge", "vivaldi", "opera", "chromium")


def _default_browser() -> str | None:
    """Path to the browser this machine opens links with, if Playwright can drive it.

    Playwright only drives Chromium builds. Brave, Edge, Vivaldi and Opera all qualify,
    so read the real default rather than starting a browser the user did not choose.
    """
    if sys.platform != "win32":
        return None
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"SOFTWARE\Microsoft\Windows\Shell\Associations"
                            r"\UrlAssociations\https\UserChoice") as key:
            prog_id = winreg.QueryValueEx(key, "ProgId")[0]
        with winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, rf"{prog_id}\shell\open\command") as key:
            command = winreg.QueryValueEx(key, "")[0]
    except OSError:
        return None
    match = re.search(r'"([^"]+\.exe)"', command) or re.search(r"(\S+\.exe)", command)
    if not match:
        return None
    exe = match.group(1)
    stem = pathlib.Path(exe).stem.lower()
    return exe if any(b in stem for b in CHROMIUM_BROWSERS) and pathlib.Path(exe).exists() else None


def _is_gated(text: str) -> bool:
    return "opište" in text or "captcha" in text.lower()


def _is_signed_in(text: str) -> bool:
    """Whether a page was served to a known user.

    This reads the absence of the invitation to log in, so it needs a page that would
    have carried one. An empty body, an error, or a CAPTCHA carries no invitation
    either, and each of those used to pass as a valid session.
    """
    if len(text.strip()) < 400 or _is_gated(text):
        return False
    return not any(m in text for m in SIGNED_OUT)


def _verify(cookies: list, host: str) -> bool:
    """Ask the site whether these cookies amount to a session."""
    if not cookies:
        return False
    jar = requests.cookies.RequestsCookieJar()
    for c in cookies:
        jar.set(c["name"], c["value"], domain=c["domain"].lstrip("."))
    try:
        probe = _s.get(f"https://{host}/", timeout=30, cookies=jar, allow_redirects=True)
    except Exception:
        return False
    return _is_signed_in(BeautifulSoup(probe.text, "lxml").get_text())


def _store(host: str, cookies: list) -> None:
    saved = {}
    if COOKIE_FILE.exists():
        try:
            saved = json.loads(COOKIE_FILE.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            saved = {}
    STORE.mkdir(parents=True, exist_ok=True)
    saved[host] = cookies
    COOKIE_FILE.write_text(json.dumps(saved), encoding="utf-8")
    COOKIE_FILE.chmod(0o600)
    _attach(host, cookies)


def _harvest(ctx, host: str) -> bool:
    """Take the session for `host` out of a browser context, if there is one."""
    cookies = [{"name": c["name"], "value": c["value"], "domain": c["domain"]}
               for c in ctx.cookies() if host.endswith(c["domain"].lstrip("."))]
    if not _verify(cookies, host):
        return False
    _store(host, cookies)
    return True


@mcp.tool()
def login(host: str = "theses.cz", wait_seconds: int = 240, attach: bool = False,
          port: int = 9222) -> dict:
    """Open a browser window, let you sign in, and keep the session for later calls.

    Nothing here handles your password. A real browser opens on the school's own login
    page, you sign in there — EduID included, with whatever second factor it asks for —
    and this reads back only the session cookies that host set. They are stored under
    ~/.theses-mcp/cookies.json so a restart does not ask again; delete that file to
    forget them.

    The CAPTCHA on the gated schools stands in front of anonymous visitors. Any account
    the school federation accepts gets past it, so EduID from one Czech university often
    opens the repository of another. The browser profile persists between calls, so the
    second school usually signs in without asking again.

    The window uses a profile of its own, so it starts signed out and without your
    extensions. You sign in once and the session is kept. theses.cz has no OAuth, so
    there is no way for your everyday browser to hand a session to another program.
    Nothing else needs setting up.

    host: the repository to sign in to, e.g. "is.slu.cz" or "theses.cz"
    wait_seconds: how long to leave the window open for you
    attach: use the browser you already have open, with your extensions and saved
        passwords. It only works if that browser was started with
        `--remote-debugging-port`, which most people will not have done.
    port: the debugging port, when `attach` is set

    Requires the optional browser extra: pip install "theses-mcp[login]".
    """
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        return {"error": 'browser login needs the optional extra: pip install "theses-mcp[login]"'}

    STORE.mkdir(parents=True, exist_ok=True)
    collected, signed_in = [], False
    exe = _default_browser()
    with sync_playwright() as pw:
        attached = None
        if attach:
            try:
                attached = pw.chromium.connect_over_cdp(f"http://127.0.0.1:{port}", timeout=5000)
            except Exception:
                return {
                    "error": f"no browser is listening on port {port}",
                    "how": ["quit the browser completely, including any tray icon",
                            f"start it once with --remote-debugging-port={port}",
                            "run login again, then restart it normally to close the port"],
                    "command": f'"{exe or "browser.exe"}" --remote-debugging-port={port}',
                    "simpler": "drop attach and sign in once in the window login opens",
                }
        try:
            # the browser already on the machine, so no download is needed; the bundled
            # Chromium is the fallback for a machine that has none
            ctx = attached.contexts[0] if attached else pw.chromium.launch_persistent_context(
                str(STORE / "browser"), headless=False, **({"executable_path": exe} if exe else {}))
        except Exception as e:
            return {"error": f"could not start a browser: {type(e).__name__}",
                    "how": "run: playwright install chromium"}

        if attached and _harvest(ctx, host):
            # already signed in from an earlier visit; no need to open anything
            return {"host": host, "result": "signed in", "source": "the browser you had open",
                    "stored": str(COOKIE_FILE), "next": "call whoami to confirm, then fulltext"}
        page = ctx.new_page() if attached else (ctx.pages[0] if ctx.pages else ctx.new_page())
        try:
            # /shibboleth/ is the EduID entry where a host has one, and /auth/ is the
            # local sign-in, which the IS family forwards to islogin.cz. The root page
            # only links to them, so land on the form itself.
            for path in ("/shibboleth/", "/auth/", "/"):
                landed = page.goto(f"https://{host}{path}", timeout=60_000)
                if not landed or landed.status < 400:
                    break
            deadline = time.monotonic() + wait_seconds
            while time.monotonic() < deadline and not page.is_closed():
                try:
                    signed_in = _is_signed_in(page.content())
                    if signed_in:
                        break
                    page.wait_for_timeout(1000)
                except Exception:
                    # closing the window is a normal way to end this, and a navigation
                    # in flight raises here too; either way, look at the cookies
                    break
            collected = [c for c in ctx.cookies() if host.endswith(c["domain"].lstrip("."))]
        finally:
            if attached:
                # the browser belongs to the user. Closing a tab in the middle of a
                # sign-in would throw away the flow they are half way through, so the
                # tab only goes once the session is in hand.
                if signed_in and not page.is_closed():
                    page.close()
            else:
                ctx.close()

    # Reading the page inside the browser guesses. Asking the server with the cookies
    # in hand is the real test, and it is the same request the rest of the tools make.
    collected = [{"name": c["name"], "value": c["value"], "domain": c["domain"]}
                 for c in collected]
    if not _verify(collected, host):
        names = {c["name"] for c in collected}
        midway = any(n.startswith(("_shibstate", "_opensaml")) for n in names)
        return {"host": host, "result": "sign-in not detected, nothing stored",
                "cookies_seen": sorted(names),
                "hint": "the eduID exchange started but never came back; finish it in the "
                        "tab that is still open, then run login again"
                        if midway else "the wait ran out before the sign-in finished"}

    _store(host, collected)
    return {"host": host, "result": "signed in", "cookies": len(collected),
            "stored": str(COOKIE_FILE), "next": "call whoami to confirm, then fulltext"}


@mcp.tool()
def use_cookie(host: str, cookie: str) -> dict:
    """Take a session cookie you copied out of your own browser.

    This is the route for a browser that `login` cannot drive, or for anyone who would
    rather not sign in a second time in a separate window. Sign in normally, open the
    developer tools, and copy the session cookie from Application, then Cookies.

    host: the site the cookie belongs to, e.g. "theses.cz"
    cookie: either the bare value of `__Host-issession`, or a whole `name=value; …` header

    The cookie is checked against the site before it is kept, so a stale one is refused
    rather than stored and puzzled over later.
    """
    pairs = [p for p in cookie.split(";") if "=" in p] or [f"__Host-issession={cookie.strip()}"]
    jar = requests.cookies.RequestsCookieJar()
    parsed = []
    for part in pairs:
        name, value = part.split("=", 1)
        parsed.append({"name": name.strip(), "value": value.strip(), "domain": host})
        jar.set(name.strip(), value.strip(), domain=host)

    try:
        probe = _s.get(f"https://{host}/", timeout=30, cookies=jar, allow_redirects=True)
    except Exception as e:
        return {"error": f"could not reach {host}: {type(e).__name__}"}
    if not _is_signed_in(BeautifulSoup(probe.text, "lxml").get_text()):
        return {"host": host, "result": "that cookie does not sign you in",
                "hint": "copy it again from a tab where you are signed in"}

    saved = {}
    if COOKIE_FILE.exists():
        try:
            saved = json.loads(COOKIE_FILE.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            saved = {}
    STORE.mkdir(parents=True, exist_ok=True)
    saved[host] = parsed
    COOKIE_FILE.write_text(json.dumps(saved), encoding="utf-8")
    COOKIE_FILE.chmod(0o600)
    _attach(host, parsed)
    return {"host": host, "result": "signed in", "stored": str(COOKIE_FILE)}


@mcp.tool()
def whoami() -> dict:
    """Report which hosts you are logged in to, and whether the login actually works.

    Use this after setting $THESES_COOKIES. A cookie that has expired or was copied
    from the wrong browser profile looks exactly like no cookie at all — the gated
    repository simply answers with its CAPTCHA again — so this asks each configured
    host directly instead of leaving you to guess from empty `files` lists.
    """
    if not AUTHENTICATED:
        return {"authenticated": [], "hint": "set THESES_COOKIES to use your own login; "
                "see the README section on authenticated access"}
    def status(host):
        try:
            text = _get(f"https://{host}/").get_text()
        except Exception as e:
            return f"unreachable: {type(e).__name__}"
        if _is_gated(text):
            return "cookie not accepted — still asked for a CAPTCHA"
        if _is_signed_in(text):
            return "logged in"
        return "reachable, but no sign of a session"

    return {"authenticated": {host: status(host) for host in AUTHENTICATED}}


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
        hidden = "Chybná adresa v ISu" in soup.get_text()
        return {"error": f"record {code} is not publicly viewable on theses.cz"
                if hidden else f"record {code} not found"}

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


DOC_EXT = (".pdf", ".docx", ".doc", ".rtf", ".odt", ".zip", ".txt")
DOC_RE = re.compile(r"(\S+\.(?:pdf|docx?|rtf|odt|zip|txt))\b", re.I)
DOC_LABEL = re.compile(
    r"plný text|full text|final thesis|závěrečná práce|posudek|review|"
    r"oponent|vedoucí|supervisor|příloha|attachment|hlavní práce", re.I)


def _documents(page, base: str) -> list:
    """Collect downloadable files from a school repository page.

    Extensions in the href are the easy case (is.muni.cz). Others hide the file behind
    an extensionless URL and put the name in the link text — VŠE renders
    "Hlavní práce 82000_kliv06.pdf, 7.2 MB Stáhnout" pointing at /zp/82000 — so the
    label is parsed too, and anything still ambiguous is confirmed with a HEAD request.
    """
    out, probe, seen = [], [], set()
    for a in page.select("a[href]"):
        href = requests.compat.urljoin(base, a["href"])
        if href in seen or href.startswith("mailto:"):
            continue
        label = _txt(a)
        named = DOC_RE.search(label)
        by_href = href.lower().split("?")[0].endswith(DOC_EXT)
        if by_href or named:
            seen.add(href)
            out.append({
                "label": re.sub(r"\s*St[áa]hnout\s*$", "", label) or href.rsplit("/", 1)[-1],
                "filename": named.group(1) if named else href.rsplit("/", 1)[-1],
                "url": href,
            })
        elif DOC_LABEL.search(label):
            seen.add(href)
            probe.append((label, href))

    for f in out:  # confirm the extensionless ones actually serve a file
        if f["url"].lower().split("?")[0].endswith(DOC_EXT):
            continue
        doc = _as_document(f["url"])
        f["confirmed"] = bool(doc)

    # ČZU and Škoda Auto label their downloads "Final thesis" / "Supervisor's review"
    # with neither the URL nor the text carrying a filename — ask the server instead.
    for label, href in probe:
        doc = _as_document(href)
        if doc:
            out.append({**doc, "label": label})
    return out


MAGIC = (b"%PDF", b"PK\x03\x04", b"{\\rtf", b"\xd0\xcf\x11\xe0")
MAGIC_EXT = {b"%PDF": ".pdf", b"PK\x03\x04": ".docx", b"{\\rtf": ".rtf", b"\xd0\xcf\x11\xe0": ".doc"}


def _as_document(url: str):
    """Describe `url` if it serves a file rather than a landing page, else None.

    theses.cz is inconsistent about what it links to: VŠE points at an HTML page that
    lists downloads, MENDELU points straight at the PDF. Sniff eight bytes instead of
    fetching a multi-megabyte file and trying to parse it as HTML.
    """
    try:
        r = _s.get(url, timeout=25, stream=True, headers={"Range": "bytes=0-7"})
        magic = next(r.iter_content(8), b"")
        disposition = r.headers.get("Content-Disposition", "")
        r.close()
    except Exception:
        return None
    hit = next((m for m in MAGIC if magic.startswith(m)), None)
    if not hit:
        return None
    named = re.search(r'filename\*?=(?:UTF-8\'\')?"?([^";]+)', disposition)
    name = named.group(1) if named else url.rstrip("/").rsplit("/", 1)[-1] + MAGIC_EXT[hit]
    return {"label": "Plný text práce", "filename": name, "url": url, "confirmed": True}


# Schools whose theses.cz records point nowhere, or only at a study-information system
# that holds no files, but which run a public repository of their own.
REPOSITORY = {
    "České vysoké učení technické": ("dspace.cvut.cz", "dspace7"),
    "Vysoké učení technické v Brně": ("dspace.vutbr.cz", "dspace7"),
    "Vysoká škola báňská": ("dspace.vsb.cz", "dspace7"),
    "Technická univerzita v Liberci": ("dspace.tul.cz", "dspace7"),
    "Univerzita Pardubice": ("dk.upce.cz", "dspace7"),
    "Západočeská univerzita": ("dspace.zcu.cz", "dspace7"),
    "Univerzita Karlova": ("dspace.cuni.cz", "dspace6"),
    "Jihočeská univerzita": ("dspace.jcu.cz", "dspace6"),
    "Univerzita Tomáše Bati": ("digilib.k.utb.cz", "dspace5"),
    "Vysoká škola chemicko-technologická": ("repozitar.vscht.cz", "invenio"),
}

# ZČU serves the interface and the REST backend from different hosts
REPOSITORY_API = {"dspace.zcu.cz": "naos-be.zcu.cz"}


def _norm(s: str) -> str:
    return re.sub(r"[^0-9a-zá-žA-ZÁ-Ž]+", "", s or "").lower()


def _dspace7(host: str, title: str):
    """DSpace 7: search the REST backend, then read the files off the item page."""
    api = REPOSITORY_API.get(host, host)
    r = _s.get(f"https://{api}/server/api/discover/search/objects", timeout=30,
               params={"query": title, "dsoType": "item", "size": 5})
    objects = r.json()["_embedded"]["searchResult"]["_embedded"]["objects"]
    for o in objects:
        item = o.get("_embedded", {}).get("indexableObject", {})
        if _norm(item.get("name")) != _norm(title):
            continue
        page = f"https://{host}/items/{item['uuid']}"
        soup = _get(page)
        return page, _documents(soup, getattr(soup, "final_url", page))
    return None


def _dspace6(host: str, title: str):
    """DSpace 6: the REST API hands over the bitstreams directly.

    Only the ORIGINAL bundle is wanted. DSpace also stores an extracted-text copy of
    every PDF in a TEXT bundle and a THUMBNAIL image, and those are not the thesis.
    """
    r = _s.get(f"https://{host}/rest/filtered-items", timeout=30,
               headers={"Accept": "application/json"},
               params={"query_field[]": "dc.title", "query_op[]": "contains",
                       "query_val[]": title, "limit": 5, "expand": "bitstreams"})
    for item in r.json().get("items", []):
        if _norm(item.get("name")) != _norm(title):
            continue
        files = []
        for b in item.get("bitstreams") or []:
            if b.get("bundleName") != "ORIGINAL" or b.get("mimeType") == "text/plain":
                continue
            url = f"https://{host}{b['retrieveLink']}"
            doc = _as_document(url)
            files.append(doc or {"label": b.get("name", ""), "filename": b.get("name", ""),
                                 "url": url, "confirmed": False})
            files[-1]["label"] = b.get("description") or b.get("name", "")
        return f"https://{host}/handle/{item.get('handle')}", files
    return None


def _dspace5(host: str, title: str):
    """DSpace 5: no searchable REST API, but the XMLUI search page is server-rendered.

    The search lives at /discover; /simple-search answers 200 with a login page and no
    results, which reads like an empty result set unless you look.
    """
    r = _s.get(f"https://{host}/discover", params={"query": title}, timeout=30)
    for a in BeautifulSoup(r.text, "lxml").select("a[href*='/handle/']"):
        if _norm(_txt(a)) != _norm(title):
            continue
        page = requests.compat.urljoin(f"https://{host}/", a["href"])
        soup = _get(page)
        return page, _documents(soup, getattr(soup, "final_url", page))
    return None


def _invenio(host: str, title: str):
    """Invenio: VŠCHT runs one of these instead of a DSpace.

    Records list a cover image alongside the thesis, so keep only real documents.
    """
    r = _s.get(f"https://{host}/api/theses", params={"q": title, "size": 5}, timeout=30)
    for hit in r.json()["hits"]["hits"]:
        name = (hit.get("metadata") or hit).get("title")
        if isinstance(name, dict):
            name = name.get("cs") or next(iter(name.values()), "")
        if _norm(name) != _norm(title):
            continue
        listing = hit["links"]["files"]
        files = []
        for entry in _s.get(listing, timeout=25).json().get("entries", []):
            key = entry.get("key", "")
            if not key.lower().endswith(DOC_EXT):
                continue
            url = f"{listing}/{key}/content"
            doc = _as_document(url)
            files.append(doc or {"filename": key, "url": url, "confirmed": False})
            files[-1]["label"] = key
        return hit["links"].get("self_html", f"https://{host}/theses/{hit['id']}"), files
    return None


def _repository_lookup(school: str, title: str) -> list:
    """Find the thesis in the school's own repository and list its files.

    theses.cz links ČVUT and VUT records nowhere at all, and points the STAG schools at
    a study-information system that carries metadata only. Their repositories are public
    and searchable, so look the thesis up by title there. Only an exact title match is
    accepted — a near miss would attach someone else's PDF to this record.

    Each school records which flavour it runs. Trying the other flavours as a fallback
    was worse than useless: asking an Invenio host for DSpace endpoints costs a round of
    retries and never succeeds. A repository that changes flavour needs an edit here.
    """
    # some records spell the school in capitals, so compare normalised
    hay = _norm(school)
    entry = next((v for k, v in REPOSITORY.items() if _norm(k) in hay), None)
    if not (entry and title):
        return []
    host, flavour = entry
    probes = {"dspace7": _dspace7, "dspace6": _dspace6, "dspace5": _dspace5, "invenio": _invenio}

    try:
        hit = probes[flavour](host, title)
    except Exception:
        return []
    if not hit:
        return []
    page, files = hit
    for f in files:
        f["source"] = page
    return files


def _archive_url(soup, code: str):
    """Locate the school repository link for a record.

    Usually it sits on the record page. VŠB and UHK records do not carry it there at
    all — theses.cz only renders it in the search listing — so fall back to looking the
    thesis up by title and reading the link off its own result row.
    """
    link = soup.select_one(".plny_text_ext .ext_prez a[href]") or soup.select_one(".plny_text_ext a[href]")
    if link:
        return link["href"]
    title = _txt(soup.select_one(".th-title"))
    if not title:
        return None
    listing = _get(f"{BASE}/vyhledavani/?search={requests.utils.quote(title)}&start=1")
    for it in listing.select(".vyh_polozka"):
        a = it.select_one("h4 a")
        if a and a["href"].startswith(f"/id/{code}/"):
            ext = [x["href"] for x in it.select("a[href^='http']") if "theses.cz" not in x["href"]]
            if ext:
                return ext[0]
    return None


@mcp.tool()
def fulltext(id_or_url: str) -> dict:
    """Resolve a thesis to downloadable full-text files in its school's repository.

    theses.cz stores only metadata; the files live in each school's own system
    (mostly the IS family — is.muni.cz, is.slu.cz, vskp.vse.cz …). This follows the
    record to that repository and lists the documents it exposes publicly.

    id_or_url: thesis code ("7lfo74"), /id/7lfo74/ or a full URL.

    `access` is the visibility of the theses.cz copy and does NOT decide what you can
    download: a thesis marked "autentizovaným zaměstnancům ze stejné školy" on theses.cz
    is often served freely by the school's own repository, so always check `files`.

    Each file carries `label`, `filename` and `url`; extensionless URLs also get
    `confirmed` and `content_type` from a HEAD request. An empty `files` plus a `note`
    means the school really does gate it (login or CAPTCHA) — a real answer, not an error.
    """
    code = id_or_url.strip("/ ").split("/")[-1]
    soup = _get(f"{BASE}/id/{code}/")
    if soup.select_one("#metadata") is None:
        # theses.cz answers with a file-manager error for records it will not show
        hidden = "Chybná adresa v ISu" in soup.get_text()
        return {"error": f"record {code} is not publicly viewable on theses.cz"
                if hidden else f"record {code} not found"}

    access = [_txt(li) for li in soup.select("#th-obsah li")]
    archive = _archive_url(soup, code)
    res = {"url": f"{BASE}/id/{code}/", "access": access, "archive_url": archive, "files": []}

    page = None
    if archive:
        direct = _as_document(archive)
        if direct:  # the link is the file itself (MENDELU and other IS /zp/ hosts)
            res["files"] = [direct]
            return res
        try:
            page = _get(archive)
            res["files"] = _documents(page, getattr(page, "final_url", archive))
        except Exception as e:  # SSL/DNS — some school systems are simply broken
            res["note"] = f"school repository unreachable: {type(e).__name__}"

    searched = None
    if not res["files"]:  # last resort: the school's own digital library
        school = _txt(soup.select_one("#th-sloupec .oddil"))
        searched = next((h for k, (h, _) in REPOSITORY.items() if _norm(k) in _norm(school)), None)
        found = _repository_lookup(school, _txt(soup.select_one(".th-title")))
        if found:
            res["files"] = found
            res["archive_url"] = found[0].pop("source", archive)
            res.pop("note", None)
            for f in res["files"]:
                f.pop("source", None)

    if res["files"] and all(f.get("confirmed") is False for f in res["files"]):
        # TUL answers its own bitstream URLs with 410; the listing is real, the files are not
        res["note"] = "repository lists these files but will not serve them anonymously"

    if not res["files"] and "note" not in res:
        if page is None:
            # do not blame a missing link when the repository was searched and came up empty
            res["note"] = (f"the record links nowhere, and {searched} holds no thesis under this title"
                           if searched else "no school repository link in the record")
        else:
            host = requests.compat.urlparse(archive).netloc
            wall = _is_gated(page.get_text())
            hint = "" if host in AUTHENTICATED else f" — set THESES_COOKIES for {host} to use your own login"
            # "světu" means theses.cz publishes this to the world; a CAPTCHA in front of
            # it is the school's own doing, not a restriction on the thesis
            public = "světu" in access
            res["note"] = (f"{'thesis is public, but the ' if public else ''}"
                           f"repository gates anonymous access with a CAPTCHA/login{hint}"
                           if wall else "no public files listed; access is likely restricted")
    return res


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
    pub = fulltext("7lfo74")  # public thesis → real PDF in the school repository
    assert pub["access"] == ["světu"], pub["access"]
    assert any(f["url"].endswith(".pdf") for f in pub["files"]), pub
    # restricted on theses.cz, but VŠE serves the PDF from an extensionless /zp/ URL
    vse = fulltext("pl09jx")
    main = next(f for f in vse["files"] if f["filename"].endswith(".pdf"))
    assert main["confirmed"], main
    # VŠB: archive link missing from the record page, and a handle redirect to DSpace
    vsb = fulltext("xggdtq")
    assert vsb["archive_url"], "archive link not recovered from the search listing"
    assert [f for f in vsb["files"] if f["confirmed"]], vsb["files"]
    men = fulltext("x52k58")  # MENDELU: the archive link IS the PDF, not a landing page
    assert len(men["files"]) == 1 and men["files"][0]["confirmed"], men
    czu = fulltext("wjep6w")  # ČZU: downloads labelled, no filename in URL or text
    assert len(czu["files"]) == 3 and all(f["confirmed"] for f in czu["files"]), czu
    cvut = fulltext("dvopnp")  # ČVUT: record links nowhere, files live in its own DSpace
    assert "dspace.cvut.cz" in (cvut["archive_url"] or ""), cvut
    assert len(cvut["files"]) == 4 and all(f["confirmed"] for f in cvut["files"]), cvut
    jcu = fulltext("tfmf5y")  # JČU: DSpace 6 REST, and the school is spelled in capitals
    assert "dspace.jcu.cz" in (jcu["archive_url"] or ""), jcu
    assert jcu["files"] and all(f["confirmed"] for f in jcu["files"]), jcu
    zcu = fulltext("b1tsqt")  # ZČU: interface and REST backend on different hosts
    assert "dspace.zcu.cz" in (zcu["archive_url"] or ""), zcu
    assert zcu["files"] and all(f["confirmed"] for f in zcu["files"]), zcu
    # UTB is checked at the lookup, not through a record: its DSpace 5 has no searchable
    # API and no theses.cz record was found that is both public and present in it
    utb = _repository_lookup("Univerzita Tomáše Bati ve Zlíně", "Korupce a její vliv na společnost")
    assert len(utb) == 3, utb
    vscht = _repository_lookup("Vysoká škola chemicko-technologická v Praze",
                               "Wikipedia v chemii, chemie na Wikipedii")  # Invenio, not DSpace
    assert vscht and all(f["confirmed"] for f in vscht), vscht
    print("OK", r["total"], "hits |", d["author"], "|", d["type"], "|",
          len(pub["files"]), "+", len(vse["files"]), "+", len(vsb["files"]),
          "+", len(men["files"]), "files")


def main():
    _selftest() if "--selftest" in sys.argv else mcp.run()


if __name__ == "__main__":
    main()
