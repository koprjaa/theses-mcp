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

`search` returns two kinds of hits:

- `kind="record"` — a catalogue record; `url` points to the detail page
- `kind="fulltext"` — a match **inside the thesis PDF**; `pdf_url` is a direct link to the
  full text, which your client can read directly

Not every thesis is public. `detail` reports the restriction verbatim in
`fulltext_access` (e.g. *"autentizovaným zaměstnancům ze stejné školy/fakulty"* — staff of
the same faculty only) together with `archive_url`, the school's own repository.

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
