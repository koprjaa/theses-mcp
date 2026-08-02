# theses-mcp

MCP server for [theses.cz](https://theses.cz), the Czech national registry of university theses. It gives an MCP client full text search over Czech bachelor, master, and doctoral theses. It also returns their metadata and links to the PDF files.

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
| `search(query, limit=10)` | Searches records and thesis full texts. Accepts the theses.cz operators `AND`, `OR`, and `"phrase"`. Pages by 10, maximum 50. |
| `detail(id_or_url)` | Full record: author, Czech and English title and abstract, keywords, supervisor, opponent, defense date, full text availability, archive link, related theses. |
| `fulltext(id_or_url)` | Follows the record into the school repository and lists the files. The list holds the thesis PDF and the two reviewer reports. |
| `login(host)` | Opens a browser window on the sign-in page of the school. It waits for you, then keeps the session. |
| `whoami()` | Lists the hosts where you hold a session. It also reports whether each session still works. |

`search` returns two kinds of hit. A hit with `kind="record"` is a catalogue record, and `url` points to the detail page. A hit with `kind="fulltext"` matched inside the thesis PDF, and `pdf_url` links to the file.

## How it works

theses.cz stores metadata only. The files live in the system of each school. 64 institutions take part. Most of them run the same IS software as theses.cz, such as `is.muni.cz`, `is.slu.cz`, `is.ambis.cz`, `is.vsfs.cz`, and `is.jamu.cz`. A few others sit alongside, such as `vskp.vse.cz`. `fulltext` follows the record into the correct system and lists the documents:

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

The `access` field does not control what you can download. It describes the copy that theses.cz holds, not the policy of the school. The VŠE thesis `pl09jx` shows *"autentizovaným zaměstnancům ze stejné školy/fakulty"* on theses.cz. The host `vskp.vse.cz` still serves its 7.2 MB PDF to anyone. Read the `files` list, not the `access` field.

Schools hide their files in three ways, so the server looks for all three. Some links carry the filename in the URL. Some carry it in the link text only. VŠE writes "Hlavní práce 82000_kliv06.pdf, 7.2 MB Stáhnout" as the link text. The link itself points at `/zp/82000`. ČZU and Škoda Auto give no filename at all. They label the link "Final thesis" or "Supervisor's review".

A link without an extension needs a check. The server reads the first eight bytes of the file and reports the result in `confirmed`. A HEAD request does not settle it, because DSpace answers HEAD with `text/html` and then serves a PDF.

Two more shapes matter. The archive link is sometimes the file itself, not a page that lists files. MENDELU and ČZU work this way. VŠB and UHK records carry no archive link on the record page. Only the search listing holds it, so the server looks the thesis up by title to recover the link.

Some records link nowhere useful. ČVUT and VUT records carry no archive link at all. The STAG schools point at a study information system that holds metadata and no files. Those universities run a public DSpace of their own. The server searches that repository by title as a last resort. It accepts an exact title match only. A near miss would attach the PDF of a stranger to the record.

theses.cz has no public API, so the server scrapes it. A request that the server will not answer yet returns a 117 byte `<meta refresh>` stub, which asks for a delay. An instant retry earns another stub, so the server waits for the delay and tries up to four times. This is not limited to the first request of a session. Under load, theses.cz falls back to the stub as well.

A markup change breaks the parser. Run `python theses_mcp.py --selftest` to check the selectors against known records when results look empty. Keep the request rate reasonable.

## Coverage

`tools/e2e_schools.py` walked the whole chain for all 64 institutions. It downloaded a PDF for 12 of them. Those 12 are the only schools this project claims outright, because bytes that start with `%PDF` reached the disk:

| School | Size | Source |
|---|---|---|
| Škoda Auto | 4039 kB | `is.savs.cz` |
| ČVUT | 2670 kB | `dspace.cvut.cz` |
| ČZU | 2536 kB | `is.czu.cz` |
| ZČU | 2079 kB | `dspace.zcu.cz` |
| VŠE | 1685 kB | `vskp.vse.cz` |
| VUT | 1646 kB | `dspace.vutbr.cz` |
| UPCE | 1505 kB | `dk.upce.cz` |
| VŠB-TUO | 1271 kB | `hdl.handle.net` |
| VŠKK | 956 kB | `is.vskk.cz` |
| JČU | 821 kB | `dspace.jcu.cz` |
| MENDELU | 743 kB | `is.mendelu.cz` |
| MU | 705 kB | `is.muni.cz` |

The other 52 split into four groups. 23 reached the record and found no file. 22 gave no thesis whose result header names the school, which is a limit of the sampling. 7 crashed with a retry error near the end of the run, after theses.cz began to refuse the pace, so they carry no verdict at all.

Three schools work at the lookup and did not finish the chain. UK, UTB, and VŠCHT each return files when asked for a title that their repository holds. The run found no thesis for them that is both publicly viewable on theses.cz and present in that repository. Treat those three as probable, not proven.

The table below records the mechanism per repository. It comes from a wider sample: one thesis for each of the 64 institutions, plus about 500 records from 20 unrelated subject searches.

| Repository | Schools | Result |
|---|---|---|
| `is.muni.cz` | Masaryk University | Works. Full text, both reviewer reports, often DOCX and TXT. |
| `vskp.vse.cz`, `www.vse.cz` | VŠE Praha | Works. Thesis, appendix, both reports. |
| `is.czu.cz`, `is.savs.cz` | ČZU, Škoda Auto | Works. Thesis and both reviews. |
| `is.mendelu.cz`, `is.vskk.cz` | MENDELU, VŠKK | Works. The link is the PDF itself. |
| `hdl.handle.net`, `dspace.cvut.cz`, `dspace.vutbr.cz`, `dk.upce.cz`, `dspace.cuni.cz`, `dspace.jcu.cz`, `dspace.zcu.cz`, `digilib.k.utb.cz`, `repozitar.vscht.cz` | VŠB-TUO, ČVUT, VUT, UPCE, UK, JČU, ZČU, UTB, VŠCHT | Works. Handle redirect, or a title lookup in the repository of the school. |
| `dspace.tul.cz` | TUL | Lists the files, then answers 410 for every one. Reported, not hidden. |
| `is.ambis.cz`, `is.vsfs.cz`, `is.slu.cz`, `is.caritas-vos.cz`, `is.vstecb.cz`, `is.vszdrav.cz`, `is.cevro.cz`, `is.ucp.cz`, `is.jamu.cz`, `is.jabok.cz`, `is.sting.cz` | AMBIS, VŠFS, SU Opava, CARITAS, VŠTE, VŠ zdravotnická, CEVRO, UCP, JAMU, JABOK, Sting | CAPTCHA for anonymous visitors. Use your own login. |
| `wstag.jcu.cz`, `stag.tul.cz`, `stag.upol.cz`, `portal.upce.cz`, `portal.zcu.cz`, `portal.osu.cz`, `portal.ujep.cz`, `stagweb.vfu.cz` | JČU, TUL, UPOL, UPCE, ZČU, OSU, UJEP, VFU | STAG holds metadata only. |
| `is.vsh.cz`, `is.vshe.cz` | VŠH, VŠHE | Broken TLS certificate. The hostname does not match. |
| `evskp.uhk.cz` | Hradec Králové | Own system. Not implemented. |

The sampling is the weak part of every sweep here. It picks a thesis by matching the school name in the result headers, and for 22 institutions it finds none. That is a limit of the method, not a verdict on the school. An earlier sweep found no sample for VŠB, and VŠB downloads a PDF.

The CAPTCHA is not bot detection. A fresh session sent one request twice, once as this server and once as a browser. Both answers were the same page, byte for byte. `is.muni.cz` runs the same software and serves that client without a problem. Those eleven schools gate anonymous access as a matter of policy. A person in a browser meets the same wall. The server reports the CAPTCHA and does not try to defeat it.

What stands behind that wall is worth knowing. Five theses from each of the eleven schools gave 55 empty results out of 55. Of those 55, the registry marks 43 as *"světu"*, published to the world. It marks only 5 as closed to everyone. These are not secret documents. The registry calls them public. The archive in front of them refuses an anonymous caller. `fulltext` reports that, rather than filing them under "restricted".

## Authenticated access

Two groups of theses open after you log in. The first group carries the mark *"všem autentizovaným"*. The second group sits in a repository that gates anonymous visitors.

Check first whether you have anything to log in with. theses.cz turns students away from eduID, and the gated schools want an account of their own. See Limits before you spend time on this.

Use `login` for this. It opens a browser window on the sign-in page of the school. You sign in there, with EduID and any second factor. The server then keeps the session cookies that the host set.

The window runs a profile of its own, so it starts signed out and without your extensions. You sign in once and the session persists. There is no way around that. `claude login` and similar commands reuse your everyday browser because OAuth lets the site hand a token back to a loopback address. theses.cz has no OAuth. Its only trace of a login is a cookie inside the browser, and browsers keep those away from other programs on purpose.

```
login("is.slu.cz")     opens a window, waits for you, stores the session
whoami()               confirms that the cookie still works
fulltext("qr1tvh")     now sees what your account sees
```

No password passes through this code. The server writes sessions to `~/.theses-mcp/cookies.json`. Delete that file to forget them. This tool needs the optional browser extra:

```bash
pip install "theses-mcp[login]"
```

`login` drives the Chromium browser already on the machine, which on Windows means the one the system opens links with. Run `playwright install chromium` only if the machine has none.

Two other routes exist. `login(attach=True)` uses the browser you already have open, with your extensions and saved passwords, but only if you started it with `--remote-debugging-port=9222`. Or set the cookies yourself:

```bash
THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "__Host-issession=…"}'
```

Each cookie goes to its own domain only. Either route gives you the access of your account and nothing more.

Call `whoami` afterwards rather than guessing. An expired cookie gives the same empty `files` list as no cookie at all. A logout link on the page proves nothing. The IS template shows one to anonymous visitors too. Instead, `whoami` looks for the missing invitation to log in.

## Limits

- Each school records which repository software it runs. Blind probing costs a round of retries per wrong guess and never succeeds against the wrong flavour. A repository that changes flavour needs a correction here.
- UTB and VŠCHT pass against their repositories, not through a record. No theses.cz thesis was both publicly viewable and present in them.
- STAG does serve files, which contradicts the row above. They come from a portlet at `/StagPortletsJSR168/PagesDispatcherServlet` with `pp_page=souboryStudentuDownloadPage` and a `soubidno` file id. Three such URLs, from UJEP, VFU and the Evropská výzkumná univerzita, each returned a real PDF. What is missing is the file id. It appears on the thesis page only after the session bound portlet flow, and guessing the name of the listing page earns a 500. Follow the flow rather than construct the URL.
- UJEP also runs ARL. This server does not speak that system.
- Give theses.cz room. A long sweep plus ad hoc queries pushed it into answering 503 twice in one day. Pace the runs and stop when it starts refusing.
- OSU has no repository of its own. Its theses live in STAG, which holds no files. UPOL was not located.
- Signing in helps less than it sounds. theses.cz accepts eduID but answers a student with *"Systém theses.cz zatím neumožňuje přihlašování studentů"*, so only staff get a session. The eleven gated schools offer no eduID at all. Their sign-in goes to `islogin.cz/<school>/login/` and wants an account at that school. A student therefore has nothing to log in to, and `login` is for staff, or for a member of the school that holds the thesis.
- The CAPTCHA schools have no second source. A search for five sample theses found none of them. That search covered NUŠL, OpenAIRE, and repositories of their own. Those files exist in one place, and that place wants a login.
- `evskp.uhk.cz` runs its own system. The server does not parse it.
- `is.vsh.cz` and `is.vshe.cz` serve certificates that do not match their hostname. The session verifies TLS and refuses them. Turning verification off would hide the problem. It would also remove the proof that you talk to the school.

Pull requests are welcome.

## Development

```bash
uv run --extra dev ruff check .
uv run --extra dev pytest -q
```

The suite reaches no network. It covers the text extraction, the document link patterns, and the three repository shapes. Those shapes are an extension in the href, a filename in the link text, and a label with neither. CI runs on Python 3.10, 3.11, and 3.12, on Linux and Windows.

`python theses_mcp.py --selftest` is the other half. It queries theses.cz for real, and it fails when the selectors stop matching. Run it when results look empty. A markup change breaks parsing in a way that unit tests cannot see.

`python tools/e2e_schools.py` walks the whole chain for every institution on the register. It finds a thesis, opens the record, resolves the files, and downloads one. A school counts as working only when bytes that start with `%PDF` land on disk.

## License

[MIT](LICENSE)
