// Project: theses-mcp
// File: src/login.rs
// Description: Browser sign-in over the Chrome DevTools Protocol: opens a window, waits, reads back session cookies.
// Author: Jan Alexandr Kopřiva
// License: MIT

use crate::Theses;
use crate::http::Cookie;
use crate::session::{is_gated, is_signed_in};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CHROMIUM_BROWSERS: [&str; 6] = ["brave", "chrome", "msedge", "vivaldi", "opera", "chromium"];

pub struct Params {
    pub host: String,
    pub wait_seconds: i64,
    pub attach: bool,
    pub port: i64,
    pub url: Option<String>,
}

/// The `.exe` named by a shell command from the registry, if it is a Chromium-family browser.
pub fn browser_from_command(command: &str) -> Option<String> {
    let quoted = regex::Regex::new(r#""([^"]+\.exe)""#).expect("static");
    let bare = regex::Regex::new(r"(\S+\.exe)").expect("static");
    let exe = quoted
        .captures(command)
        .or_else(|| bare.captures(command))?[1]
        .to_string();
    let stem = std::path::Path::new(&exe)
        .file_stem()?
        .to_string_lossy()
        .to_lowercase();
    CHROMIUM_BROWSERS
        .iter()
        .any(|b| stem.contains(b))
        .then_some(exe)
}

/// Path to the browser this machine opens links with, if it is a Chromium build.
///
/// Windows reads the real default from the registry. Elsewhere there is no cheap way to ask,
/// so the first Chromium-family browser found at its usual place is used.
#[cfg(windows)]
pub fn default_browser() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    let choice = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(
            r"SOFTWARE\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        )
        .ok()?;
    let prog_id: String = choice.get_value("ProgId").ok()?;
    let command: String = RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!(r"{prog_id}\shell\open\command"))
        .ok()?
        .get_value("")
        .ok()?;
    let exe = PathBuf::from(browser_from_command(&command)?);
    exe.exists().then_some(exe)
}

#[cfg(not(windows))]
pub fn default_browser() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        for (app, bin) in [
            ("Brave Browser", "Brave Browser"),
            ("Google Chrome", "Google Chrome"),
            ("Microsoft Edge", "Microsoft Edge"),
            ("Vivaldi", "Vivaldi"),
            ("Opera", "Opera"),
            ("Chromium", "Chromium"),
        ] {
            candidates.push(format!("/Applications/{app}.app/Contents/MacOS/{bin}").into());
        }
    } else if let Some(path) = std::env::var_os("PATH") {
        for name in [
            "brave-browser",
            "brave",
            "google-chrome",
            "google-chrome-stable",
            "microsoft-edge",
            "microsoft-edge-stable",
            "vivaldi",
            "opera",
            "chromium",
            "chromium-browser",
        ] {
            candidates.extend(std::env::split_paths(&path).map(|d| d.join(name)));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn host_cookies(
    all: Vec<chromiumoxide::cdp::browser_protocol::network::Cookie>,
    host: &str,
) -> Vec<Cookie> {
    all.into_iter()
        .filter(|c| host.ends_with(c.domain.trim_start_matches('.')))
        .map(|c| Cookie {
            name: c.name,
            value: c.value,
            domain: c.domain,
        })
        .collect()
}

/// When the wait for the user to sign in runs out.
pub fn deadline(wait_seconds: i64) -> Instant {
    // a year is as good as forever, and Instant + Duration panics when it overflows
    const YEAR: i64 = 365 * 24 * 3600;
    Instant::now() + Duration::from_secs(wait_seconds.clamp(0, YEAR) as u64)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("worker thread")
}

/// Verify the cookies against the site and keep them if they buy anything.
async fn harvest(app: &Arc<Theses>, cookies: Vec<Cookie>, host: &str, url: Option<String>) -> bool {
    let (app, host) = (app.clone(), host.to_string());
    blocking(move || {
        if !app.verify(&cookies, &host, url.as_deref()) {
            return false;
        }
        app.store_session(&host, &cookies).is_ok()
    })
    .await
}

fn no_listener(port: i64, exe: &Option<PathBuf>) -> Value {
    let default = if cfg!(windows) {
        "browser.exe"
    } else {
        "chromium"
    };
    let shown = exe
        .as_ref()
        .map_or(default.to_string(), |p| p.display().to_string());
    json!({
        "error": format!("no browser is listening on port {port}"),
        "how": [
            "quit the browser completely, including any tray icon",
            format!("start it once with --remote-debugging-port={port}"),
            "run login again, then restart it normally to close the port",
        ],
        "command": format!("\"{shown}\" --remote-debugging-port={port}"),
        "simpler": "drop attach and sign in once in the window login opens",
    })
}

/// Open a browser, wait for the user to sign in, and keep the session.
pub async fn login(app: Arc<Theses>, p: Params) -> Result<Value, String> {
    crate::http::install_crypto();
    let store: PathBuf = app.store_dir().to_path_buf();
    std::fs::create_dir_all(&store).map_err(|e| e.to_string())?;
    let exe = default_browser();
    let host = p.host.clone();
    let explicit_url = p.url.as_deref().filter(|u| !u.is_empty()).map(String::from);

    let (mut browser, mut handler) = if p.attach {
        let endpoint = format!("http://127.0.0.1:{}", p.port);
        match tokio::time::timeout(Duration::from_secs(5), Browser::connect(endpoint)).await {
            Ok(Ok(pair)) => pair,
            _ => return Ok(no_listener(p.port, &exe)),
        }
    } else {
        // the browser already on the machine, so no download is needed
        let mut config = BrowserConfig::builder()
            .with_head()
            .viewport(None)
            .window_size(1280, 900)
            .user_data_dir(store.join("browser"));
        if let Some(path) = &exe {
            config = config.chrome_executable(path);
        }
        let launched = match config.build() {
            Ok(c) => Browser::launch(c).await.map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        match launched {
            Ok(pair) => pair,
            Err(_) => {
                return Ok(json!({
                    "error": "could not start a browser: LaunchError",
                    "how": "install a Chromium-based browser (Chrome, Edge, Brave, Vivaldi, Opera or Chromium)",
                }));
            }
        }
    };
    let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let outcome = drive(&app, &mut browser, &p, &host, explicit_url).await;

    if !p.attach {
        let _ = browser.close().await;
        let _ = browser.wait().await;
    }
    pump.abort();
    outcome
}

async fn drive(
    app: &Arc<Theses>,
    browser: &mut Browser,
    p: &Params,
    host: &str,
    explicit_url: Option<String>,
) -> Result<Value, String> {
    let url_given = p.url.is_some();
    if p.attach {
        let cookies = host_cookies(browser.get_cookies().await.unwrap_or_default(), host);
        if harvest(app, cookies, host, p.url.clone()).await {
            // already signed in from an earlier visit; no need to open anything
            return Ok(json!({
                "host": host,
                "result": "signed in",
                "source": "the browser you had open",
                "stored": app.cookie_file().display().to_string(),
                "next": "call whoami to confirm, then fulltext",
            }));
        }
    }
    let existing = if p.attach {
        None
    } else {
        browser
            .pages()
            .await
            .ok()
            .and_then(|v| v.into_iter().next())
    };
    let page = match existing {
        Some(page) => page,
        None => browser
            .new_page("about:blank")
            .await
            .map_err(|e| e.to_string())?,
    };

    // /shibboleth/ is the EduID entry where a host has one, and /auth/ is the local sign-in,
    // which the IS family forwards to islogin.cz. The root page only links to them, so land
    // on the form itself.
    let nav = Duration::from_secs(60);
    if let Some(url) = &explicit_url {
        tokio::time::timeout(nav, page.goto(url.as_str()))
            .await
            .map_err(|_| "navigation timed out".to_string())?
            .map_err(|e| e.to_string())?;
    } else {
        for path in ["/shibboleth/", "/auth/", "/"] {
            tokio::time::timeout(nav, page.goto(format!("https://{host}{path}")))
                .await
                .map_err(|_| "navigation timed out".to_string())?
                .map_err(|e| e.to_string())?;
            let status = page
                .evaluate("performance.getEntriesByType('navigation')[0]?.responseStatus ?? 0")
                .await
                .ok()
                .and_then(|r| r.into_value::<i64>().ok())
                .unwrap_or(0);
            if status < 400 {
                break;
            }
        }
    }

    let deadline = deadline(p.wait_seconds);
    let mut signed_in = false;
    while Instant::now() < deadline {
        // closing the window is a normal way to end this, and a navigation in flight
        // fails here too; either way, look at the cookies
        let Ok(body) = page.content().await else {
            break;
        };
        signed_in = is_signed_in(&body) || (url_given && !is_gated(&body));
        if signed_in {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let collected = host_cookies(browser.get_cookies().await.unwrap_or_default(), host);

    if p.attach && signed_in {
        // the browser belongs to the user: a tab is only closed once the session is in hand
        let _ = page.close().await;
    }

    // Reading the page inside the browser guesses. Asking the server with the cookies in
    // hand is the real test, and it is the same request the rest of the tools make.
    let ok = {
        let (app, host, cookies, url) = (
            app.clone(),
            host.to_string(),
            collected.clone(),
            p.url.clone(),
        );
        blocking(move || app.verify(&cookies, &host, url.as_deref())).await
    };
    if !ok {
        let mut names: Vec<&str> = collected.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let midway = names
            .iter()
            .any(|n| n.starts_with("_shibstate") || n.starts_with("_opensaml"));
        return Ok(json!({
            "host": host,
            "result": "sign-in not detected, nothing stored",
            "cookies_seen": names,
            "hint": if midway {
                "the eduID exchange started but never came back; finish it in the tab that is still open, then run login again"
            } else {
                "the wait ran out before the sign-in finished"
            },
        }));
    }
    {
        let (app, host, cookies) = (app.clone(), host.to_string(), collected.clone());
        blocking(move || app.store_session(&host, &cookies))
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({
        "host": host,
        "result": "signed in",
        "cookies": collected.len(),
        "stored": app.cookie_file().display().to_string(),
        "next": "call whoami to confirm, then fulltext",
    }))
}
