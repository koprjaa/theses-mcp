// Project: theses-mcp
// File: src/session.rs
// Description: Signed-in sessions: the saved cookie file, use_cookie, whoami and cookie verification.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use crate::Theses;
use crate::dom::Page;
use crate::error::Result;
use crate::http::{Cookie, Request, Transport};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The IS template carries "Odhlášení ze systému" even for anonymous visitors, so a logout
/// link proves nothing. The invitation to log in is what disappears once you have.
const SIGNED_OUT: [&str; 4] = ["Přihlásit se", "Přihlásit sa", "Log in", "Login"];

pub fn is_gated(text: &str) -> bool {
    text.contains("opište") || text.to_lowercase().contains("captcha")
}

/// Whether a page was served to a known user. This reads the absence of the invitation to
/// log in, so it needs a page that would have carried one. An empty body, an error, or a
/// CAPTCHA carries no invitation either, and each of those used to pass as a valid session.
pub fn is_signed_in(text: &str) -> bool {
    if text.trim().chars().count() < 400 || is_gated(text) {
        return false;
    }
    !SIGNED_OUT.iter().any(|m| text.contains(m))
}

/// `~/.theses-mcp`, where `login` keeps the sessions it collects.
pub fn default_store() -> PathBuf {
    std::env::home_dir().unwrap_or_default().join(".theses-mcp")
}

impl Theses {
    /// Build the server state: the session from `$THESES_COOKIES` first, then whatever
    /// `login` saved on an earlier run.
    pub fn new(http: Arc<dyn Transport>, store: PathBuf, env_cookies: Option<&str>) -> Self {
        let this = Theses {
            http,
            authenticated: Mutex::new(Vec::new()),
            store,
        };
        this.load_env(env_cookies.unwrap_or("").trim());
        this.load_saved();
        this
    }

    pub fn cookie_file(&self) -> PathBuf {
        self.store.join("cookies.json")
    }

    pub fn store_dir(&self) -> &Path {
        &self.store
    }

    pub fn is_authenticated(&self, host: &str) -> bool {
        self.authenticated
            .lock()
            .expect("lock")
            .iter()
            .any(|h| h == host)
    }

    pub fn authenticated_hosts(&self) -> Vec<String> {
        self.authenticated.lock().expect("lock").clone()
    }

    fn load_env(&self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let Ok(Value::Object(mapping)) = serde_json::from_str::<Value>(raw) else {
            eprintln!("THESES_COOKIES is not valid JSON — ignored");
            return;
        };
        for (host, blob) in &mapping {
            if let Some(blob) = blob.as_str() {
                for part in blob.split(';') {
                    if let Some((name, value)) = part.split_once('=') {
                        self.http.attach(&Cookie {
                            name: name.trim().to_string(),
                            value: value.trim().to_string(),
                            domain: host.clone(),
                        });
                    }
                }
            }
            self.authenticated.lock().expect("lock").push(host.clone());
        }
    }

    fn load_saved(&self) {
        let Ok(text) = std::fs::read_to_string(self.cookie_file()) else {
            return;
        };
        let Ok(Value::Object(saved)) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        for (host, cookies) in saved {
            if let Ok(cookies) = serde_json::from_value::<Vec<Cookie>>(cookies) {
                self.attach(&host, &cookies);
            }
        }
    }

    pub fn attach(&self, host: &str, cookies: &[Cookie]) {
        for c in cookies {
            self.http.attach(&Cookie {
                domain: c.domain.trim_start_matches('.').to_string(),
                ..c.clone()
            });
        }
        let mut hosts = self.authenticated.lock().expect("lock");
        if !hosts.iter().any(|h| h == host) {
            hosts.push(host.to_string());
        }
    }

    /// Write the session for `host` into `cookies.json` (mode 0600) and attach it.
    pub fn store_session(&self, host: &str, cookies: &[Cookie]) -> Result<()> {
        let io = |e: std::io::Error| crate::error::Error::new("OSError", e.to_string());
        let existing = std::fs::read_to_string(self.cookie_file()).ok();
        let mut saved: Map<String, Value> =
            match existing.as_deref().map(serde_json::from_str::<Value>) {
                None => Map::new(),
                Some(Ok(Value::Object(o))) => o,
                // keep what cannot be read; overwriting it would lose every other host's session
                Some(_) => {
                    let broken = self.store.join("cookies.json.bad");
                    std::fs::copy(self.cookie_file(), broken).map_err(io)?;
                    Map::new()
                }
            };
        std::fs::create_dir_all(&self.store).map_err(io)?;
        saved.insert(host.to_string(), json!(cookies));
        // written beside the file with the final mode, then renamed: never half-written,
        // never readable by others
        let temp = self.store.join("cookies.json.tmp");
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp).map_err(io)?;
            file.write_all(Value::Object(saved).to_string().as_bytes())
                .map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        std::fs::rename(&temp, self.cookie_file()).map_err(io)?;
        self.attach(host, cookies);
        Ok(())
    }

    /// Ask the site whether these cookies buy anything. Two things count: a session the site
    /// recognises as a person, and a session that has already passed the CAPTCHA the gated
    /// schools put in front of anonymous callers.
    pub fn verify(&self, cookies: &[Cookie], host: &str, url: Option<&str>) -> bool {
        if cookies.is_empty() {
            return false;
        }
        let target = match url {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => format!("https://{host}/"),
        };
        let mut req = Request::get(target, 30);
        req.overlay = cookies
            .iter()
            .map(|c| Cookie {
                domain: c.domain.trim_start_matches('.').to_string(),
                ..c.clone()
            })
            .collect();
        let Ok(resp) = self.get_waiting(&req) else {
            return false;
        };
        let text = Page::parse(&resp.text(), &resp.url).text();
        is_signed_in(&text) || (url.is_some() && !is_gated(&text))
    }

    pub fn use_cookie(&self, host: &str, cookie: &str) -> Result<Value> {
        let mut parsed: Vec<Cookie> = cookie
            .split(';')
            .filter_map(|p| p.split_once('='))
            .map(|(n, v)| Cookie {
                name: n.trim().to_string(),
                value: v.trim().to_string(),
                domain: host.to_string(),
            })
            .collect();
        if parsed.is_empty() {
            parsed.push(Cookie {
                name: "__Host-issession".into(),
                value: cookie.trim().to_string(),
                domain: host.to_string(),
            });
        }
        let mut req = Request::get(format!("https://{host}/"), 30);
        req.overlay = parsed.clone();
        let probe = match self.get_waiting(&req) {
            Ok(p) => p,
            Err(e) => return Ok(json!({"error": format!("could not reach {host}: {}", e.class)})),
        };
        if !is_signed_in(&Page::parse(&probe.text(), &probe.url).text()) {
            return Ok(json!({
                "host": host,
                "result": "that cookie does not sign you in",
                "hint": "copy it again from a tab where you are signed in",
            }));
        }
        self.store_session(host, &parsed)?;
        Ok(
            json!({"host": host, "result": "signed in", "stored": self.cookie_file().display().to_string()}),
        )
    }

    pub fn whoami(&self) -> Result<Value> {
        let hosts = self.authenticated_hosts();
        if hosts.is_empty() {
            return Ok(json!({
                "authenticated": [],
                "hint": "set THESES_COOKIES to use your own login; see the README section on authenticated access",
            }));
        }
        let mut out = Map::new();
        for host in hosts {
            let status = match self.get_page(&format!("https://{host}/")) {
                Err(e) => format!("unreachable: {}", e.class),
                Ok(page) => {
                    let text = page.text();
                    if is_gated(&text) {
                        "cookie not accepted — still asked for a CAPTCHA".to_string()
                    } else if is_signed_in(&text) {
                        "logged in".to_string()
                    } else {
                        // the gate sits on the thesis pages, not here, so this proves nothing either way
                        "cookie attached; the front page is not gated, so ask fulltext instead"
                            .to_string()
                    }
                }
            };
            out.insert(host, json!(status));
        }
        Ok(json!({"authenticated": out}))
    }
}
