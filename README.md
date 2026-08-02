# theses-mcp

MCP server for [theses.cz](https://theses.cz), the Czech national registry of university theses. It gives an MCP client full text search over Czech bachelor, master, and doctoral theses, their metadata, and links to the PDF files.

![python](https://img.shields.io/badge/python-3.10+-3776AB?style=flat-square&logo=python&logoColor=white)
![license](https://img.shields.io/badge/license-MIT-A31F34?style=flat-square)
![status](https://img.shields.io/badge/status-active-22863A?style=flat-square)

The registry holds about one million records from most Czech universities. The Faculty of Informatics at Masaryk University operates it.

## Install

Claude Code:

```bash
claude mcp add theses -s user -- uvx --from git+https://github.com/koprjaa/theses-mcp theses-mcp
```

Other clients, in `mcpServers`:

```json
{
  "theses": {
    "command": "uvx",
    "args": ["--from", "git+https://github.com/koprjaa/theses-mcp", "theses-mcp"]
  }
}
```

From a clone, run `pip install -e .` and set `command` to `theses-mcp`.

## Use

```
> find Czech theses about Midjourney and copyright

search("midjourney AND autorství")   12 hits, some with public PDFs
detail("c5uqln")                     abstract, keywords, supervisor, defense date
fulltext("7lfo74")                   PDF, DOCX, and both reviewer reports
```

## Tools

| Tool | Result |
|---|---|
| `search(query, limit=10)` | Searches records and thesis full texts. Accepts theses.cz operators `AND`, `OR`, and `"phrase"`. Pages by 10, maximum 50. |
| `detail(id_or_url)` | Full record: author, Czech and English title and abstract, keywords, supervisor, opponent, defense date, full text availability, archive link, related theses. |
| `fulltext(id_or_url)` | Follows the record into the school repository and lists the files. This includes the thesis PDF and the supervisor and opponent reports. |

`search` returns two kinds of hit. A hit with `kind="record"` is a catalogue record and `url` points to the detail page. A hit with `kind="fulltext"` matched inside the thesis PDF and `pdf_url` links to the file.

## How it works

theses.cz stores metadata only. The files live in the system of each school. 64 institutions take part. Most of them run the same IS software as theses.cz (`is.muni.cz`, `is.slu.cz`, `is.ambis.cz`, `is.vsfs.cz`, `is.jamu.cz`), with `vskp.vse.cz` and a few others alongside. `fulltext` follows the record into the correct system and lists the documents:

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

The `access` field does not control what you can download. It describes the copy that theses.cz holds, not the policy of the school. The VŠE thesis `pl09jx` shows *"autentizovaným zaměstnancům ze stejné školy/fakulty"* on theses.cz, but `vskp.vse.cz` serves its 7.2 MB PDF to anyone. Read the `files` list, not the `access` field.

Schools hide their files in three different ways, so the server looks for all of them. Some links carry the filename in the URL. Some carry it in the link text only, as VŠE does with "Hlavní práce 82000_kliv06.pdf, 7.2 MB Stáhnout" pointing at `/zp/82000`. ČZU and Škoda Auto give no filename at all and label the link "Final thesis" or "Supervisor's review". Anything without an extension is settled by reading the first eight bytes of the file and reporting `confirmed`, because DSpace answers a HEAD request with `text/html` and then serves a PDF.

Two more shapes worth knowing. The archive link is sometimes the file itself rather than a page listing files, which is how MENDELU and ČZU work. And VŠB and UHK records carry no archive link on the record page, only in the search listing, so the server looks the thesis up by title to recover it.

theses.cz has no public API, so the server scrapes it. A request the server does not want to answer yet returns a 117 byte `<meta refresh>` stub asking for a delay; retrying instantly earns another stub, so the delay is honoured and the request is tried up to four times. This is not only the first request of a session, as theses.cz falls back to the stub under load too. Markup changes break the parser, so run `python theses_mcp.py --selftest` to check the selectors against known records when results look empty.

Keep the request rate reasonable.

## Coverage

Measured over about 500 records from 20 unrelated subject searches, across 17 repository hosts.

| Repository | Schools | Result |
|---|---|---|
| `is.muni.cz` | Masaryk University | Works. Full text, both reviewer reports, often DOCX and TXT. |
| `vskp.vse.cz`, `www.vse.cz` | VŠE Praha | Works. Thesis, appendix, both reports. |
| `hdl.handle.net` | DSpace repositories such as VŠB-TUO | Works. The handle redirects to the repository. |
| `is.ambis.cz`, `is.vsfs.cz`, `is.slu.cz`, `is.caritas-vos.cz`, `is.vstecb.cz`, `is.vszdrav.cz`, `is.cevro.cz`, `is.ucp.cz`, `is.jamu.cz`, `is.jabok.cz` | AMBIS, VŠFS, SU Opava, CARITAS, VŠTE, VŠ zdravotnická, CEVRO, UCP, JAMU, JABOK | CAPTCHA for anonymous visitors. |
| `is.vsh.cz`, `is.vshe.cz` | VŠH, VŠHE | Broken TLS certificate. The hostname does not match. |
| `evskp.uhk.cz` | Hradec Králové | Different system. Not implemented. |
| `stag.upol.cz`, `portal.ujep.cz` | UPOL, UJEP | STAG portal. Not implemented. |

About twenty of the 64 institutions never appeared in the sample. Their repositories are untested.

The CAPTCHA is not bot detection. `is.muni.cz` serves the same client without a problem. Those schools gate anonymous access, and a person in a browser meets the same wall. The server reports the CAPTCHA and does not try to defeat it.

## Authenticated access

Theses marked *"všem autentizovaným"* and repositories that gate anonymous visitors both open after you log in. Log in through your browser, copy the session cookie, and pass it per host:

```bash
THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "__Host-issession=…"}'
```

Each cookie goes to its own domain only. This gives you the access that your account has and nothing more.

## Limits

- STAG portals (`stag.upol.cz`, and the same software at ZČU, UPCE, JČU, TUL) put the download behind a session bound portlet flow with `pc_phs` and `_csrf` parameters. UPOL does not expose the STAG web service at the usual `/ws/services/rest2/` path.
- `evskp.uhk.cz` and `portal.ujep.cz` run their own systems. The server does not parse them.
- `is.vsh.cz` and `is.vshe.cz` serve certificates that do not match their hostname. The session verifies TLS and refuses them. Turning verification off would hide the problem and remove the proof that you talk to the school.

Pull requests are welcome.

## License

[MIT](LICENSE)
