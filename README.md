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

Plenty of theses are not public, and `files` comes back empty with a `note` saying why —
that is a real answer, not a failure:

- `access: ["světu"]` — public to the world, expect files
- `access: ["autentizovaným zaměstnancům ze stejné školy/fakulty"]` and similar — restricted
  to that school; use `archive_url` and log in yourself
- some repositories put a CAPTCHA in front of anonymous visitors; the server reports this
  and does not try to get around it

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
