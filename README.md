# theses-mcp

Search Czech university theses and retrieve document links for MCP clients.

[![CI](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

## What it does

- Searches thesis metadata and full texts on theses.cz.
- Reads titles, abstracts, keywords, and defense information.
- Finds thesis files, attachments, and reviewer reports on theses.cz or school repositories.
- Supports your own authenticated sessions through `login`, `use_cookie`, and `whoami`.

## Install

Install a current stable Rust toolchain, then:

```bash
cargo install --git https://github.com/koprjaa/theses-mcp
```

This puts `theses-mcp` in `~/.cargo/bin`. Add this entry to your MCP client's `mcpServers` configuration:

```json
{
  "theses": {
    "command": "theses-mcp"
  }
}
```

No environment variables are required for public access.

## Use

Ask your MCP client to call these tools:

```text
search(query="midjourney AND autorství", limit=10)
detail(id_or_url="pl09jx")
fulltext(id_or_url="7lfo74")
```

Use a record's code or URL for `detail` and `fulltext`. Search accepts `AND`, `OR`, and quoted phrases. Your client exposes the remaining tool parameters.

`fulltext` returns file links, not extracted text. Check its `files` and `note` fields for availability. Some repositories require login or show a CAPTCHA. Call `login` with the repository host to sign in using your own account. Sessions persist in `~/.theses-mcp/cookies.json`; delete that file to forget them. The file format is the one the earlier Python version wrote, so saved sessions keep working.

## Browser login

`login` opens a window of a Chromium-family browser (Chrome, Edge, Brave, Vivaldi, Opera, or Chromium) with a profile of its own in `~/.theses-mcp/browser`, and reads back only the cookies of the host you sign in to. It talks to the browser over the Chrome DevTools Protocol, so nothing is downloaded.

- On Windows it opens your default browser if that is a Chromium build. On macOS and Linux there is no cheap way to ask, so it uses the first Chromium-family browser it finds at the usual places.
- Firefox and Safari cannot be driven. Use `use_cookie` with a cookie copied from the browser's developer tools instead.
- `attach=true` uses a browser that is already running with `--remote-debugging-port`.

## Check

`theses-mcp --selftest` runs the parsers against the live sites. It makes several dozen requests, so run it by hand now and then, not in CI.

## License

[AGPL-3.0-or-later](LICENSE)
