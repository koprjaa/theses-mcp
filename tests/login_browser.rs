// Project: theses-mcp
// File: tests/login_browser.rs
// Description: End to end check of the browser login against a headless Chromium-family browser.
// Author: Jan Alexandr Kopřiva
// License: MIT

//! Needs an installed Chromium-family browser, so it is skipped by default:
//!
//!     cargo test --test login_browser -- --ignored
//!
//! It starts the browser headless with a debugging port, serves a page that sets a cookie,
//! and runs `login` in attach mode against that page.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use theses_mcp::Theses;
use theses_mcp::http::ReqwestTransport;
use theses_mcp::login::{Params, default_browser, login};

fn serve_pages() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                let body = format!(
                    "<html><body>{}</body></html>",
                    "Moje studium a závěrečné práce. ".repeat(30)
                );
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nSet-Cookie: sess=abc123; Path=/\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    port
}

#[tokio::test]
#[ignore = "needs a Chromium-family browser"]
async fn login_reads_the_session_out_of_a_running_browser() {
    let exe = default_browser().expect("no Chromium-family browser installed");
    let profile = tempfile::tempdir().unwrap();
    let debug_port = 9333;
    let mut browser = Command::new(exe)
        .args([
            "--headless=new",
            "--no-first-run",
            &format!("--remote-debugging-port={debug_port}"),
            &format!("--user-data-dir={}", profile.path().display()),
            "about:blank",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_secs(4));

    let page_port = serve_pages();
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".theses-mcp");
    // the blocking client has to be built and dropped off the async threads
    let store_for_app = store.clone();
    let app = tokio::task::spawn_blocking(move || {
        Arc::new(Theses::new(
            Arc::new(ReqwestTransport::new()),
            store_for_app,
            None,
        ))
    })
    .await
    .unwrap();
    let out = login(
        app.clone(),
        Params {
            host: "127.0.0.1".into(),
            wait_seconds: 20,
            attach: true,
            port: debug_port,
            url: Some(format!("http://127.0.0.1:{page_port}/")),
        },
    )
    .await;
    let _ = browser.kill();
    let _ = browser.wait();
    let out = out.unwrap();
    assert_eq!(out["result"], "signed in", "{out}");
    assert!(out["cookies"].as_u64().unwrap() >= 1, "{out}");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(store.join("cookies.json")).unwrap())
            .unwrap();
    assert_eq!(saved["127.0.0.1"][0]["name"], "sess");
    assert!(app.is_authenticated("127.0.0.1"));
    tokio::task::spawn_blocking(move || drop(app))
        .await
        .unwrap();
}

/// Opens a real, visible browser window for a few seconds with a profile of its own.
#[tokio::test]
#[ignore = "opens a browser window"]
async fn login_opens_its_own_window_and_closes_it() {
    let page_port = serve_pages();
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".theses-mcp");
    let store_for_app = store.clone();
    let app = tokio::task::spawn_blocking(move || {
        Arc::new(Theses::new(
            Arc::new(ReqwestTransport::new()),
            store_for_app,
            None,
        ))
    })
    .await
    .unwrap();
    let out = login(
        app.clone(),
        Params {
            host: "127.0.0.1".into(),
            wait_seconds: 30,
            attach: false,
            port: 9222,
            url: Some(format!("http://127.0.0.1:{page_port}/")),
        },
    )
    .await
    .unwrap();
    assert_eq!(out["result"], "signed in", "{out}");
    assert!(
        store.join("browser").is_dir(),
        "the profile persists between calls"
    );
    tokio::task::spawn_blocking(move || drop(app))
        .await
        .unwrap();
}
