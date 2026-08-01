# theses-mcp

MCP server for [theses.cz](https://theses.cz) — the Czech national registry of university
theses (~1M records from most Czech universities, operated by the Faculty of Informatics,
Masaryk University).

Gives any MCP client full-text search over Czech bachelor/master/doctoral theses, their
metadata, and direct links to the publicly readable PDFs.

## Tools

| Tool | What it does |
|------|--------------|
| `search(query, limit=10)` | Search records and thesis full texts. Supports theses.cz operators (`AND`, `OR`, `"phrase"`). Paginates by 10, max 50. |
| `detail(id_or_url)` | Full record: author, CS/EN title and abstract, keywords, supervisor and opponent, defense date, full-text availability, school archive link, related theses. |
| `fulltext(id_or_url)` | Follows the record into the school's own repository and lists the downloadable files — the thesis PDF plus supervisor/opponent reports. |

`search` returns two kinds of hits:

- `kind="record"` — a catalogue record; `url` points to the detail page
- `kind="fulltext"` — a match **inside the thesis PDF**; `pdf_url` is a direct link to the
  full text, which your client can read directly

## Getting the PDF

theses.cz stores metadata only — the files live in each school's own system. 64
institutions are registered, but they are not 64 different problems: the large majority run
the same IS software as theses.cz itself (`is.muni.cz`, `is.slu.cz`, `is.ambis.cz`,
`is.vsfs.cz`, `is.jamu.cz`, …), with `vskp.vse.cz` and a few others alongside. `fulltext`
follows the record into whichever one it is and lists the documents on offer:

```json
{
  "access": ["světu"],
  "archive_url": "https://is.muni.cz/th/avwwh/",
  "files": [
    {"label": "Plný text práce",   "url": "https://is.muni.cz/th/avwwh/Moutelik_Bakalarska_Prace_Final.pdf"},
    {"label": "Posudek vedoucího", "url": "https://is.muni.cz/th/avwwh/posudek_vedouciho_Minjarikova.pdf"}
  ]
}
```

**`access` does not decide what you can download.** It describes the copy held by
theses.cz, not the school's own policy. The VŠE thesis `pl09jx` is marked *"autentizovaným
zaměstnancům ze stejné školy/fakulty"* on theses.cz while `vskp.vse.cz` links its 7.2 MB
PDF to anyone — so always look at `files`, not at `access`.

That link had no `.pdf` in the URL; the filename was only in the link text. Files are
therefore matched by href **and** by label, and extensionless URLs are confirmed with a
HEAD request (`confirmed`, `content_type`) rather than optimistically reported as PDFs.

### What actually comes back

Measured over 24 theses drawn from six unrelated subject searches:

| repository | result |
|---|---|
| `is.muni.cz` | 7/7 — full text, both reviewer reports, usually DOCX and TXT too |
| `vskp.vse.cz` | thesis, appendix and both reports, via extensionless `/zp/` URLs |
| `is.vsfs.cz`, `is.slu.cz`, `is.ambis.cz`, `is.ucp.cz` | 0/14 — CAPTCHA for anonymous visitors |
| `stag.upol.cz`, `portal.ujep.cz` | 0/2 — different systems, not implemented |

The CAPTCHA is not bot detection — `is.muni.cz` serves this same client fine. Those
schools gate *anonymous* access, and a human in a browser meets the same wall. This server
reports it and does not try to defeat it.

### Using your own login

The way past a gate is to be someone entitled to pass it. Theses marked *"všem
autentizovaným"* (any authenticated user) and repositories that gate anonymous visitors
both open up once you are logged in. Log in through your browser, copy the session cookie
and hand it over per host:

```bash
THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "__Host-issession=…"}'
```

Cookies are scoped to their own domain and are never sent to any other host. This gets you
what your account is entitled to — nothing more.

### Known gaps

STAG-based portals (`stag.upol.cz`, and the same software at ZČU, UPCE, JČU, TUL) put the
download behind a session-bound portlet flow with `pc_phs` and `_csrf` parameters, and UPOL
does not expose the STAG web service at the usual `/ws/services/rest2/` path. Not
implemented — PRs welcome.

## Install

Claude Code:

```bash
claude mcp add theses -s user -- uvx --from git+https://github.com/koprjaa/theses-mcp theses-mcp
```

Any other client, in `mcpServers`:

```json
{
  "theses": {
    "command": "uvx",
    "args": ["--from", "git+https://github.com/koprjaa/theses-mcp", "theses-mcp"]
  }
}
```

From a clone, use `pip install -e .` and `command: "theses-mcp"`.

## Example

```
> find Czech theses about Midjourney and copyright

search("midjourney AND autorství")   → 12 hits, incl. public PDFs
detail("c5uqln")                     → abstract, keywords, supervisor, defense date
fulltext("7lfo74")                   → PDF, DOCX and both reviewer reports
```

## Notes

theses.cz is scraped, not queried through an API — it has no public one. Two consequences:

- **Session bootstrap.** The first request returns a 117-byte `<meta refresh>` stub; the
  server retries once with the `__Host-issession` cookie it just received.
- **Markup drift breaks parsing.** `python theses_mcp.py --selftest` runs a live check
  against known records and fails loudly when the selectors stop matching. Run it first
  when results look empty.

Be a decent citizen with request rates.

## License

MIT
