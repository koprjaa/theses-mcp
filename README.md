# theses-mcp

MCP server for [theses.cz](https://theses.cz), the Czech national registry of university theses. It gives an MCP client full text search over Czech bachelor, master, and doctoral theses, their metadata, and links to the PDF files.

![python](https://img.shields.io/badge/python-3.10+-3776AB?style=flat-square&logo=python&logoColor=white)
![license](https://img.shields.io/badge/license-MIT-A31F34?style=flat-square)
![status](https://img.shields.io/badge/status-active-22863A?style=flat-square)
[![ci](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/koprjaa/theses-mcp/actions/workflows/ci.yml)

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

Some records link nowhere useful. ČVUT and VUT records carry no archive link at all, and the STAG schools point at a study information system that holds metadata and no files. Those universities run a public DSpace of their own, so the server searches it by title as a last resort and accepts only an exact title match, because a near miss would attach someone else's PDF to the record. A ČVUT thesis that theses.cz links nowhere resolves this way to its PDF, its poster appendix, and both reviews.

theses.cz has no public API, so the server scrapes it. A request the server does not want to answer yet returns a 117 byte `<meta refresh>` stub asking for a delay; retrying instantly earns another stub, so the delay is honoured and the request is tried up to four times. This is not only the first request of a session, as theses.cz falls back to the stub under load too. Markup changes break the parser, so run `python theses_mcp.py --selftest` to check the selectors against known records when results look empty.

Keep the request rate reasonable.

## Coverage

One thesis was sampled for each of the 64 institutions, and separately about 500 records were drawn from 20 unrelated subject searches.

| Repository | Schools | Result |
|---|---|---|
| `is.muni.cz` | Masaryk University | Works. Full text, both reviewer reports, often DOCX and TXT. |
| `vskp.vse.cz`, `www.vse.cz` | VŠE Praha | Works. Thesis, appendix, both reports. |
| `is.czu.cz`, `is.savs.cz` | ČZU, Škoda Auto | Works. Thesis and both reviews. |
| `is.mendelu.cz`, `is.vskk.cz` | MENDELU, VŠKK | Works. The link is the PDF itself. |
| `hdl.handle.net`, `dspace.cvut.cz`, `dspace.vutbr.cz`, `dk.upce.cz`, `dspace.cuni.cz`, `dspace.jcu.cz`, `dspace.zcu.cz` | VŠB-TUO, ČVUT, VUT, UPCE, UK, JČU, ZČU | Works. Handle redirect, or a title lookup in the school's DSpace. |
| `dspace.tul.cz` | TUL | Lists the files and then answers 410 for every one. Reported, not hidden. |
| `is.ambis.cz`, `is.vsfs.cz`, `is.slu.cz`, `is.caritas-vos.cz`, `is.vstecb.cz`, `is.vszdrav.cz`, `is.cevro.cz`, `is.ucp.cz`, `is.jamu.cz`, `is.jabok.cz`, `is.sting.cz` | AMBIS, VŠFS, SU Opava, CARITAS, VŠTE, VŠ zdravotnická, CEVRO, UCP, JAMU, JABOK, Sting | CAPTCHA for anonymous visitors. Use your own login. |
| `wstag.jcu.cz`, `stag.tul.cz`, `stag.upol.cz`, `portal.upce.cz`, `portal.zcu.cz`, `portal.osu.cz`, `portal.ujep.cz`, `stagweb.vfu.cz` | JČU, TUL, UPOL, UPCE, ZČU, OSU, UJEP, VFU | STAG holds metadata only. Their DSpace repositories run an older API and are not wired up yet. |
| `is.vsh.cz`, `is.vshe.cz` | VŠH, VŠHE | Broken TLS certificate. The hostname does not match. |
| `evskp.uhk.cz` | Hradec Králové | Own system. Not implemented. |

Roughly twenty institutions are still untested. The per-institution sweep picks a thesis by matching the school name in the result headers and misses them, which is a limit of the sampling and not a verdict on the school. VŠB proves the point: the sweep found no sample for it, yet it works.

The CAPTCHA is not bot detection. `is.muni.cz` serves the same client without a problem. Those schools gate anonymous access, and a person in a browser meets the same wall. The server reports the CAPTCHA and does not try to defeat it.

## Authenticated access

Theses marked *"všem autentizovaným"* and repositories that gate anonymous visitors both open after you log in. Log in through your browser, copy the session cookie, and pass it per host:

```bash
THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "__Host-issession=…"}'
```

Each cookie goes to its own domain only. This gives you the access that your account has and nothing more.

## Limits

- The DSpace lookup speaks both REST generations and tries them in turn, so a repository that upgrades keeps working without an edit. UTB is the one that fits neither: `digilib.k.utb.cz` runs a DSpace 5 whose REST API can list items but cannot search them.
- UJEP runs ARL rather than DSpace, a different system altogether. The repositories for VŠCHT, UPOL, and OSU were not located.
- The CAPTCHA schools have no second source. Five sample theses were looked for in NUŠL and OpenAIRE and in repositories of their own; none of them are anywhere else. Those files exist in one place, and that place wants a login.
- `evskp.uhk.cz` runs its own system. The server does not parse it.
- `is.vsh.cz` and `is.vshe.cz` serve certificates that do not match their hostname. The session verifies TLS and refuses them. Turning verification off would hide the problem and remove the proof that you talk to the school.

Pull requests are welcome.

## Development

```bash
uv run --extra dev ruff check .
uv run --extra dev pytest -q
```

The suite reaches no network. It covers the text extraction, the document link
patterns, and the three repository shapes: an extension in the href, a filename
in the link text, and a label with neither. CI runs on Python 3.10, 3.11, and
3.12, on Linux and Windows.

`python theses_mcp.py --selftest` is the other half. It queries theses.cz for
real and fails when the selectors stop matching. Run it when results look empty,
because a markup change breaks parsing in a way unit tests cannot see.

## License

[MIT](LICENSE)
