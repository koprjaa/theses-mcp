// Project: theses-mcp
// File: src/tools.rs
// Description: Tool names, descriptions and input schemas, identical to what the Python server advertised.
// Author: Jan Alexandr Kopřiva
// License: MIT

use serde_json::{Map, Value, json};

/// `(name, description, input schema)` for every tool, in the order the Python server listed them.
pub fn definitions() -> Vec<(&'static str, &'static str, Map<String, Value>)> {
    vec![
        (
            "search",
            r#"Search theses.cz (Czech national thesis registry) — full text and metadata.

    Returns two kinds of hits:
      kind="record"   — a thesis record; `url` points to the detail page (see `detail`)
      kind="fulltext" — a match inside the thesis PDF; `pdf_url` links to the file

    Neither URL is checked here, because checking would cost one request per hit.
    `pdf_url` in particular can point at a file the registry will not serve. Call
    `fulltext` to get links that were confirmed by reading the first bytes.

    query: search terms; supports theses.cz operators (AND, OR, "phrase")
    limit: how many results (paginated by 10, max 50)
    "#,
            schema(json!({
                "properties": {
                    "query": {
                        "title": "Query",
                        "type": "string"
                    },
                    "limit": {
                        "default": 10,
                        "title": "Limit",
                        "type": "integer"
                    }
                },
                "required": [
                    "query"
                ],
                "type": "object",
                "title": "searchArguments"
            })),
        ),
        (
            "login",
            r#"Open a browser window, let you sign in, and keep the session for later calls.

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
    "#,
            schema(json!({
                "properties": {
                    "host": {
                        "default": "theses.cz",
                        "title": "Host",
                        "type": "string"
                    },
                    "wait_seconds": {
                        "default": 240,
                        "title": "Wait Seconds",
                        "type": "integer"
                    },
                    "attach": {
                        "default": false,
                        "title": "Attach",
                        "type": "boolean"
                    },
                    "port": {
                        "default": 9222,
                        "title": "Port",
                        "type": "integer"
                    },
                    "url": {
                        "anyOf": [
                            {
                                "type": "string"
                            },
                            {
                                "type": "null"
                            }
                        ],
                        "default": null,
                        "title": "Url"
                    }
                },
                "type": "object",
                "title": "loginArguments"
            })),
        ),
        (
            "use_cookie",
            r#"Take a session cookie you copied out of your own browser.

    This is the route for a browser that `login` cannot drive, or for anyone who would
    rather not sign in a second time in a separate window. Sign in normally, open the
    developer tools, and copy the session cookie from Application, then Cookies.

    host: the site the cookie belongs to, e.g. "theses.cz"
    cookie: either the bare value of `__Host-issession`, or a whole `name=value; …` header

    The cookie is checked against the site before it is kept, so a stale one is refused
    rather than stored and puzzled over later.
    "#,
            schema(json!({
                "properties": {
                    "host": {
                        "title": "Host",
                        "type": "string"
                    },
                    "cookie": {
                        "title": "Cookie",
                        "type": "string"
                    }
                },
                "required": [
                    "host",
                    "cookie"
                ],
                "type": "object",
                "title": "use_cookieArguments"
            })),
        ),
        (
            "whoami",
            r#"Report which hosts have a session attached, and what each one answers.

    Use this after setting $THESES_COOKIES. A CAPTCHA in the answer is proof that the
    cookie failed. The opposite does not follow. These schools gate their thesis pages
    and leave the front page open, so a working cookie and no cookie at all give the
    same front page. Only `fulltext` on a thesis of that school settles it.
    "#,
            schema(json!({
                "properties": {},
                "type": "object",
                "title": "whoamiArguments"
            })),
        ),
        (
            "detail",
            r#"Fetch the full record of a thesis from theses.cz.

    id_or_url: thesis code ("pl09jx"), /id/pl09jx/ or a full URL.
    Returns author, title (incl. translation), CS/EN abstracts, keywords,
    supervisor/opponent, full-text availability and the school archive link.
    "#,
            schema(json!({
                "properties": {
                    "id_or_url": {
                        "title": "Id Or Url",
                        "type": "string"
                    }
                },
                "required": [
                    "id_or_url"
                ],
                "type": "object",
                "title": "detailArguments"
            })),
        ),
        (
            "fulltext",
            r#"Resolve a thesis to downloadable full-text files in its school's repository.

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
    "#,
            schema(json!({
                "properties": {
                    "id_or_url": {
                        "title": "Id Or Url",
                        "type": "string"
                    }
                },
                "required": [
                    "id_or_url"
                ],
                "type": "object",
                "title": "fulltextArguments"
            })),
        ),
    ]
}

fn schema(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}
