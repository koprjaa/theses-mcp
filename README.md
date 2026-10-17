# theses-mcp

Search Czech university theses and retrieve document links for MCP clients.

[![CI](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml)
[![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## What it does

- Searches thesis metadata and full texts on theses.cz.
- Reads titles, abstracts, keywords, and defense information.
- Finds thesis files, attachments, and reviewer reports on theses.cz or school repositories.
- Supports your own authenticated sessions through `login`, `use_cookie`, and `whoami`.

## Install

Requires Python 3.10 or later and uv. From the repository directory:

```bash
uv sync
```

Add this entry to your MCP client's `mcpServers` configuration. Replace the project path with the absolute path to your checkout:

```json
{
  "theses": {
    "command": "uv",
    "args": ["run", "--project", "/absolute/path/to/theses-mcp", "theses-mcp"]
  }
}
```

No environment variables are required for public access. Browser login needs the optional dependency:

```bash
uv sync --extra login
```

If `login` cannot start Chromium, install it with `uv run playwright install chromium`.

## Use

Ask your MCP client to call these tools:

```text
search(query="midjourney AND autorství", limit=10)
detail(id_or_url="pl09jx")
fulltext(id_or_url="7lfo74")
```

Use a record's code or URL for `detail` and `fulltext`. Search accepts `AND`, `OR`, and quoted phrases. Your client exposes the remaining tool parameters.

`fulltext` returns file links, not extracted text. Check its `files` and `note` fields for availability. Some repositories require login or show a CAPTCHA. Call `login` with the repository host to sign in using your own account. Sessions persist in `~/.theses-mcp/cookies.json`; delete that file to forget them.

## License

[MIT](LICENSE)
