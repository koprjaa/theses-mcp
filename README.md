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
| `use_cookie(host, cookie)` | Takes a session cookie you copied out of your own browser. It checks the cookie before it keeps it. |
| `whoami()` | Lists the hosts that have a session attached, and what each one answers. |

`search` returns two kinds of hit. A hit with `kind="record"` is a catalogue record, and `url` points to the detail page. A hit with `kind="fulltext"` matched inside the thesis PDF, and `pdf_url` links to the file.

Neither URL is checked, because a check costs one request per hit. `pdf_url` can point at a file the registry will not serve. Call `fulltext` when you need a link that was confirmed by reading the first bytes.

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

Some records link nowhere useful. ČVUT and VUT records carry no archive link at all. Both universities run a public DSpace of their own. The server searches that repository by title as a last resort. It accepts an exact title match only. A near miss would attach the PDF of a stranger to the record.

STAG needs its own path. A STAG record URL answers with an empty portal shell. The file list arrives afterwards, so the page itself holds nothing. That list comes from a portlet at `/StagPortletsJSR168/PagesDispatcherServlet`, which answers a plain GET. It needs no session, no CSRF token and no browser. The only argument is the `praceIdno` that the record URL already carries. The server asks for four blocks: the thesis, its attachments, and the two reviews.

theses.cz has no public API, so the server scrapes it. A request that the server will not answer yet returns a 117 byte `<meta refresh>` stub, which asks for a delay. An instant retry earns another stub, so the server waits for the delay and tries up to four times. This is not limited to the first request of a session. Under load, theses.cz falls back to the stub as well.

A markup change breaks the parser. Run `python theses_mcp.py --selftest` to check the selectors against known records when results look empty. Keep the request rate reasonable.

## Coverage

`tools/e2e_schools.py` walked the whole chain for all 64 institutions: find a thesis of that school, open the record, resolve the files, download one, and read the bytes. A school counts only when a complete PDF reached the disk. Complete means the file starts with `%PDF` and ends with `%%EOF`. **38 of the 64 institutions pass.**

| School | Size | Served by |
|---|---|---|
| Vysoká škola aplikované psychologie, s.r.o. | 46783 kB | `theses.cz` |
| Pražská vysoká škola psychosociálních studií, s.r.o. | 42756 kB | `theses.cz` |
| JABOK - Vyšší odborná škola sociálně pedagogická a teologická | 38305 kB | `is.jabok.cz` |
| Vysoká škola evropských a regionálních studií, z. ú. | 36128 kB | `theses.cz` |
| Česká zemědělská univerzita v Praze | 29151 kB | `is.czu.cz` |
| Vysoká škola kreativní komunikace, s.r.o. | 14406 kB | `is.vskk.cz` |
| CEVRO Univerzita, z.ú. | 10026 kB | `is.cevro.cz` |
| Škoda Auto Vysoká škola z.ú. | 4039 kB | `is.savs.cz` |
| University College Prague | 2919 kB | `is.ucp.cz` |
| České vysoké učení technické v Praze | 2670 kB | `dspace.cvut.cz` |
| Univerzita Palackého v Olomouci | 1851 kB | `stag.upol.cz` |
| Technická univerzita v Liberci | 1817 kB | `stag.tul.cz` |
| Slezská univerzita v Opavě | 1796 kB | `is.slu.cz` |
| Vysoká škola ekonomická v Praze | 1685 kB | `vskp.vse.cz` |
| Moravská vysoká škola Olomouc, o.p.s. | 1684 kB | `stag-mvso.zcu.cz` |
| Vysoké učení technické v Brně | 1646 kB | `theses.cz` |
| Vysoká škola zdravotnická, o.p.s. | 1620 kB | `is.vszdrav.cz` |
| Policejní akademie České republiky v Praze | 1619 kB | `theses.cz` |
| Veterinární univerzita Brno | 1542 kB | `stagweb.vfu.cz` |
| Univerzita Pardubice | 1505 kB | `portal.upce.cz` |
| Vysoká škola obchodní a hotelová s.r.o. | 1502 kB | `theses.cz` |
| Evropská výzkumná univerzita, z.ú. | 1481 kB | `stag-vsss.zcu.cz` |
| Soukromá vysoká škola ekonomická Znojmo, s.r.o. | 1339 kB | `theses.cz` |
| Vysoká škola báňská - Technická univerzita Ostrava | 1271 kB | `hdl.handle.net` |
| Vysoká škola finanční a správní, a.s. | 1239 kB | `is.vsfs.cz` |
| Janáčkova akademie múzických umění | 1158 kB | `is.jamu.cz` |
| Západočeská univerzita v Plzni | 888 kB | `portal.zcu.cz` |
| Vysoká škola technická a ekonomická v Českých Budějovicích | 872 kB | `is.vstecb.cz` |
| Jihočeská univerzita v Českých Budějovicích | 821 kB | `wstag.jcu.cz` |
| Mendelova univerzita v Brně | 743 kB | `is.mendelu.cz` |
| Masarykova univerzita | 705 kB | `is.muni.cz` |
| Ambis Univerzita | 660 kB | `is.ambis.cz` |
| Univerzita Jana Amose Komenského Praha s.r.o. | 592 kB | `theses.cz` |
| Vyšší odborná škola MILLS | 479 kB | `theses.cz` |
| CARITAS – Vyšší odborná škola sociální Olomouc | 415 kB | `is.caritas-vos.cz` |
| Univerzita Hradec Králové | 378 kB | `evskp.uhk.cz` |
| Univerzita Jana Evangelisty Purkyně v Ústí nad Labem | 260 kB | `portal.ujep.cz` |
| Vysoká škola logistiky o.p.s. | 102 kB | `theses.cz` |

Nine come from STAG. Six of those returned nothing before: UPOL, TUL, VFU, UJEP, Moravská VŠ Olomouc and the Evropská výzkumná univerzita. The other three, JČU, UPCE and ZČU, used to arrive through a title lookup in a DSpace next door. The portlet answers with a thesis id instead. A near miss can no longer attach the PDF of a stranger.

Ten come from schools that gate anonymous visitors with a CAPTCHA. Those rows need your own session. See [Authenticated access](#authenticated-access).

Ten come from theses.cz itself. The registry does not only point at the school. Where it holds the documents, it serves them from `/id/<code>/<filename>`, and that block sits outside the metadata section. Reading only the archive link misses a copy that is already there.

The remaining 26 split in two.

16 gave no thesis at all. An earlier version of this file called that a limit of the sampling. It is not. A control settles it. A quoted search for "Univerzita Pardubice" returns 29 of that school's own theses in 30 results. The search does surface a school when the registry holds it. Each of the 15 below returns zero of its own across 50 to 90 results:

> AVU · Univerzita Karlova · Univerzita obrany · Metropolitní · Panevropská · Palestra · Slovenská poľnohospodárska univerzita v Nitre · both CEDUK schools · five vyšší odborné školy · Institut pro veřejnou správu Praha

A second test agrees. Three real thesis titles, taken straight from `dspace.cuni.cz`, return nothing on theses.cz. **Charles University does not publish its theses here.** It runs a repository of its own.

The page that lists these schools is titled "Zapojené instituce", participating institutions. A school joins to use the system, which includes the plagiarism check through Odevzdej.cz. Joining does not put its theses in the public registry. Institut pro veřejnou správu Praha is the single entry under "Veřejná správa". It awards no degrees at all.

The sixteenth is UTB. The harness sampled six of its records. Every one answers "not publicly viewable on theses.cz". Its own repository, `digilib.k.utb.cz`, serves files when asked for a title it holds.

10 reached the record and found no file:

| School | Reason |
|---|---|
| OSU | STAG inside a WebSphere portal. Needs a browser. |
| Sting | CAPTCHA, and no session for that host. |
| NEWTON | No public files listed. |
| AMU | The sampled record is not publicly viewable on theses.cz. |
| VŠCHT | The record links nowhere, and `repozitar.vscht.cz` holds no thesis under that title. |
| VŠP Jihlava, Unicorn, Jahodovka, OA Brno, Veřejně správní akademie | No repository link in the record. |

The table below records the mechanism per repository. It comes from a wider sample: one thesis for each of the 64 institutions, plus about 500 records from 20 unrelated subject searches.

| Repository | Schools | Result |
|---|---|---|
| `is.muni.cz` | Masaryk University | Works. Full text, both reviewer reports, often DOCX and TXT. |
| `vskp.vse.cz`, `www.vse.cz` | VŠE Praha | Works. Thesis, appendix, both reports. |
| `is.czu.cz`, `is.savs.cz` | ČZU, Škoda Auto | Works. Thesis and both reviews. |
| `is.mendelu.cz`, `is.vskk.cz` | MENDELU, VŠKK | Works. The link is the PDF itself. |
| `hdl.handle.net`, `dspace.cvut.cz`, `dspace.vutbr.cz`, `dspace.cuni.cz`, `digilib.k.utb.cz`, `repozitar.vscht.cz` | VŠB-TUO, ČVUT, VUT, UK, UTB, VŠCHT | Works. Handle redirect, or a title lookup in the repository of the school. |
| `dspace.tul.cz` | TUL | Reached through STAG, which links here. Some bitstreams answer 410. Reported, not hidden. |
| `is.ambis.cz`, `is.vsfs.cz`, `is.slu.cz`, `is.caritas-vos.cz`, `is.vstecb.cz`, `is.vszdrav.cz`, `is.cevro.cz`, `is.ucp.cz`, `is.jamu.cz`, `is.jabok.cz`, `is.sting.cz` | AMBIS, VŠFS, SU Opava, CARITAS, VŠTE, VŠ zdravotnická, CEVRO, UCP, JAMU, JABOK, Sting | CAPTCHA for anonymous visitors. Ten of the eleven download with a session of your own. |
| `stag.tul.cz`, `stag.upol.cz`, `portal.ujep.cz`, `stagweb.vfu.cz`, `stag-vsss.zcu.cz`, `stag-mvso.zcu.cz`, `wstag.jcu.cz`, `portal.upce.cz`, `portal.zcu.cz` | TUL, UPOL, UJEP, VFU, Evropská výzkumná, Moravská VŠ Olomouc, JČU, UPCE, ZČU | Works. The file list comes from the portlet, not the page. |
| `portal.osu.cz` | OSU | STAG inside a WebSphere portal. Needs a browser. |
| `is.vsh.cz`, `is.vshe.cz` | VŠH, VŠHE | Broken TLS certificate. The hostname does not match. |
| `evskp.uhk.cz` | Hradec Králové | Works. theses.cz also holds a copy of its own. |

The CAPTCHA is not bot detection. A fresh session sent one request twice, once as this server and once as a browser. Both answers were the same page, byte for byte. `is.muni.cz` runs the same software and serves that client without a problem. Those eleven schools gate anonymous access as a matter of policy. A person in a browser meets the same wall. The server reports the CAPTCHA and does not try to defeat it.

What stands behind that wall is worth knowing. Five theses from each of the eleven schools gave 55 empty results out of 55. Of those 55, the registry marks 43 as *"světu"*, published to the world. It marks only 5 as closed to everyone. These are not secret documents. The registry calls them public. The archive in front of them refuses an anonymous caller. `fulltext` reports that, rather than filing them under "restricted".

A session of your own removes the wall. Ten of the eleven then download a complete PDF. Sting is the one left. This run held no session for that host.

## Authenticated access

Two groups of theses open after you log in. The first group carries the mark *"všem autentizovaným"*. The second group sits in a repository that gates anonymous visitors.

Check first whether you have anything to log in with. theses.cz turns students away from eduID, and the gated schools want an account of their own. See Limits before you spend time on this.

Use `login` for this. It opens a browser window on the sign-in page of the school. You sign in there, with EduID and any second factor. The server then keeps the session cookies that the host set.

The window runs a profile of its own, so it starts signed out and without your extensions. You sign in once and the session persists. There is no way around that. `claude login` and similar commands reuse your everyday browser because OAuth lets the site hand a token back to a loopback address. theses.cz has no OAuth. Its only trace of a login is a cookie inside the browser, and browsers keep those away from other programs on purpose.

```
login("is.slu.cz")     opens a window, waits for you, stores the session
whoami()               reports what each configured host answers
fulltext("qr1tvh")     now sees what your account sees
```

No password passes through this code. The server writes sessions to `~/.theses-mcp/cookies.json`. Delete that file to forget them. This tool needs the optional browser extra:

```bash
pip install "theses-mcp[login]"
```

`login` drives the Chromium browser already on the machine, which on Windows means the one the system opens links with. Run `playwright install chromium` only if the machine has none.

Two other routes exist. `login(attach=True)` uses the browser you already have open, with your extensions and saved passwords, but only if you started it with `--remote-debugging-port=9222`.

The simplest route needs no browser control at all. Sign in as you normally would, copy the session cookie from the developer tools, and hand it over:

```
use_cookie("is.slu.cz", "__Host-issession=…")
```

`use_cookie` checks the cookie against the site before it keeps it. It refuses a stale one instead of storing it and puzzling you later. To configure the same thing outside the client, set the cookies in the environment:

```bash
THESES_COOKIES='{"theses.cz": "__Host-issession=…", "is.vsfs.cz": "__Host-issession=…"}'
```

Each cookie goes to its own domain only. Either route gives you the access of your account and nothing more.

Call `whoami` afterwards, and read it for what it is. A CAPTCHA in the answer proves the cookie failed. The opposite does not follow. These schools gate their thesis pages and leave the front page open, so a working cookie and no cookie give the same front page. Only `fulltext` on a thesis of that school settles it.

A logout link proves nothing either. The IS template shows one to anonymous visitors too, so `whoami` looks for the missing invitation to log in instead.

## Limits

- Each school records which repository software it runs. Blind probing costs a round of retries per wrong guess and never succeeds against the wrong flavour. A repository that changes flavour needs a correction here.
- UTB and VŠCHT pass against their repositories, not through a record. The harness sampled six UTB records. Every one is closed on theses.cz.
- Do not guess a STAG page name. A wrong one earns a 500. The four that work are `ssProhlizeniElPodobaVSKPPage`, `ssProhlizeniElPodobaVSKPPrilohyPage`, and `ssProhlizeniPosudkyVSKPPage` twice, once per reviewer. The reviews also need `sou_aplikace`.
- TUL keeps its STAG files in DSpace. It labels the link "Zde k dispozici" and names the file nowhere. The server reads the name from the file itself.
- UJEP also runs ARL. This server does not speak that system.
- A search result header comes in two shapes. `Diplomová práce, <School>, <year>` and `Diplomová práce: <title> (<author>) <School>, <faculty>, <year>`. In the second shape, a pattern anchored on "práce," swallows the title and the author. It returns the faculty alone. That misread every thesis of Charles University as "Filozofická fakulta". `tests/test_harness.py` holds both shapes.
- theses.cz has no field search and no school facet. `fak`, `fakulta` and `skola` change nothing on the search URL. The only way to sample a school is to search its name and read the result headers.
- The title of a record in the search listing sometimes links straight at a file path, such as `/id/gt2ymz/94924_bezm05.pdf`. That path often answers with the stub and never with the file. `search` derives `/id/<code>/` instead of repeating it.
- "Not defended yet" is not a restriction. VŠE writes "Soubory budou k dispozici až po obhajobě práce" on the page and publishes nothing until the defense. `fulltext` reports that as `pending_defense`, because it becomes available on its own.
- Give theses.cz room. A long sweep plus ad hoc queries pushed it into answering 503 twice in one day. Pace the runs and stop when it starts refusing.
- OSU is the one STAG school this server cannot reach. It runs STAG inside a WebSphere portal at `portal.osu.cz/wps/`, where the servlet path does not exist. The files are there. A click in a real browser downloads them. A WebSphere navigational state token wraps the Struts action behind that link. The token comes from a page that renders only under JavaScript. One school does not justify a browser in this path.
- Signing in helps less than it sounds. theses.cz accepts eduID but answers a student with *"Systém theses.cz zatím neumožňuje přihlašování studentů"*, so only staff get a session. The eleven gated schools offer no eduID at all. Their sign-in goes to `islogin.cz/<school>/login/` and wants an account at that school. A student therefore has nothing to log in to, and `login` is for staff, or for a member of the school that holds the thesis.
- The CAPTCHA schools have no second source. A search for five sample theses found none of them. That search covered NUŠL, OpenAIRE, and repositories of their own. Those files exist in one place, and that place wants a login.
- `is.vsh.cz` and `is.vshe.cz` serve certificates that do not match their hostname. The session verifies TLS and refuses them. Turning verification off would hide the problem. It would also remove the proof that you talk to the school.

Pull requests are welcome.

## Development

```bash
uv run --extra dev ruff check .
uv run --extra dev pytest -q
```

The suite reaches no network. It covers the text extraction, the document link patterns, and the three repository shapes. Those shapes are an extension in the href, a filename in the link text, and a label with neither. CI runs on Python 3.10, 3.11, and 3.12, on Linux and Windows.

`python theses_mcp.py --selftest` is the other half. It queries theses.cz for real, and it fails when the selectors stop matching. Run it when results look empty. A markup change breaks parsing in a way that unit tests cannot see.

`python tools/e2e_schools.py` walks the whole chain for every institution on the register. It finds a thesis, opens the record, resolves the files, and downloads one. A school counts as working only when a complete PDF lands on disk. Complete means the file starts with `%PDF` and ends with `%%EOF`. A download cut short still starts with `%PDF`, and four schools once passed that way with files that would not open.

## License

[MIT](LICENSE)
