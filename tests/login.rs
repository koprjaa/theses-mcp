// Project: theses-mcp
// File: tests/login.rs
// Description: The parts of the browser login that can be checked without a browser.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
mod common;

use common::{Mock, reply};
use serde_json::json;
use std::sync::Arc;
use theses_mcp::Theses;
use theses_mcp::dispatch::login_params;
use theses_mcp::login::{Params, browser_from_command, login};

#[test]
fn the_default_browser_is_read_from_a_registry_command() {
    let brave = r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe" --single-argument %1"#;
    assert_eq!(
        browser_from_command(brave).as_deref(),
        Some(r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe")
    );
    assert_eq!(
        browser_from_command(r"C:\Edge\msedge.exe --url %1").as_deref(),
        Some(r"C:\Edge\msedge.exe")
    );
}

#[test]
fn a_browser_playwright_style_automation_cannot_drive_is_not_picked() {
    // Firefox speaks a different protocol; only Chromium builds qualify
    let firefox = r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#;
    assert_eq!(browser_from_command(firefox), None);
    assert_eq!(browser_from_command("no executable here"), None);
}

#[test]
fn the_login_defaults_are_the_advertised_ones() {
    let p = login_params(&serde_json::Map::new()).unwrap();
    assert_eq!(
        (p.host.as_str(), p.wait_seconds, p.attach, p.port, p.url),
        ("theses.cz", 240, false, 9222, None)
    );
}

#[test]
fn a_wrongly_typed_login_argument_is_refused() {
    let args = json!({"wait_seconds": "soon"})
        .as_object()
        .cloned()
        .unwrap();
    assert!(login_params(&args).is_err());
}

#[tokio::test]
async fn attaching_to_a_port_nobody_listens_on_says_how_to_open_one() {
    let home = tempfile::tempdir().unwrap();
    let app = Arc::new(Theses::new(
        Mock::new(|_| Ok(reply(b"", "text/html"))),
        home.path().join(".theses-mcp"),
        None,
    ));
    // port 1 is reserved, so nothing can be listening there
    let out = login(
        app,
        Params {
            host: "theses.cz".into(),
            wait_seconds: 1,
            attach: true,
            port: 1,
            url: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(out["error"], "no browser is listening on port 1");
    assert_eq!(out["how"].as_array().unwrap().len(), 3);
    assert!(
        out["how"][1]
            .as_str()
            .unwrap()
            .contains("--remote-debugging-port=1")
    );
    assert!(
        out["command"]
            .as_str()
            .unwrap()
            .ends_with(" --remote-debugging-port=1")
    );
    assert_eq!(
        out["simpler"],
        "drop attach and sign in once in the window login opens"
    );
    let keys: Vec<&str> = out
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["error", "how", "command", "simpler"]);
}

#[tokio::test]
async fn the_closed_port_answer_has_the_shape_python_gave() {
    let python: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/login_attach_closed.json")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let app = Arc::new(Theses::new(
        Mock::new(|_| Ok(reply(b"", "text/html"))),
        home.path().join(".theses-mcp"),
        None,
    ));
    let rust = login(
        app,
        Params {
            host: "theses.cz".into(),
            wait_seconds: 1,
            attach: true,
            port: 1,
            url: None,
        },
    )
    .await
    .unwrap();
    for key in ["error", "how", "simpler"] {
        assert_eq!(rust[key], python[key], "{key}");
    }
    // only the browser path differs: Python had no way to find one off Windows
    let suffix = " --remote-debugging-port=1";
    assert!(rust["command"].as_str().unwrap().ends_with(suffix));
    assert!(python["command"].as_str().unwrap().ends_with(suffix));
}
