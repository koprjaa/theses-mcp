// Project: theses-mcp
// File: tests/cookies.rs
// Description: Regressions for the cookie store: file handling, host matching and Secure cookies.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
mod common;

use common::Mock;
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use theses_mcp::Theses;
use theses_mcp::http::{Cookie, Jar, domain_matches};

fn store(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("theses-mcp-fixes-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn cookie(domain: &str) -> Cookie {
    Cookie {
        name: "s".into(),
        value: "1".into(),
        domain: domain.into(),
    }
}

#[test]
fn the_cookie_file_is_replaced_whole_and_private() {
    let dir = store("atomic");
    let t = Theses::new(Mock::new(|_| unreachable!()), dir.clone(), None);
    t.store_session("a.cz", &[cookie("a.cz")]).unwrap();
    t.store_session("b.cz", &[cookie("b.cz")]).unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("cookies.json")).unwrap()).unwrap();
    assert!(saved["a.cz"].is_array() && saved["b.cz"].is_array());
    assert!(!dir.join("cookies.json.tmp").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("cookies.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_unreadable_cookie_file_is_kept_aside_not_overwritten() {
    let dir = store("broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cookies.json"), r#"{"a.cz": [{"name":"s","#).unwrap();
    let t = Theses::new(Mock::new(|_| unreachable!()), dir.clone(), None);
    t.store_session("b.cz", &[cookie("b.cz")]).unwrap();
    assert!(
        std::fs::read_to_string(dir.join("cookies.json.bad"))
            .unwrap()
            .contains("a.cz")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_browser_cookie_is_taken_for_the_host_and_its_parents_only() {
    assert!(domain_matches("is.slu.cz", ".slu.cz"));
    assert!(domain_matches("theses.cz", "theses.cz"));
    assert!(!domain_matches("theses.cz", "ses.cz"));
}

#[test]
fn a_secure_cookie_does_not_travel_over_http() {
    let jar = Jar::default();
    let url = url::Url::parse("https://is.school.cz/").unwrap();
    let headers = [
        HeaderValue::from_static("s=1; Secure; Domain=school.cz"),
        HeaderValue::from_static("p=2; Domain=school.cz"),
        HeaderValue::from_static("__Host-h=3"),
    ];
    jar.set_cookies(&mut headers.iter(), &url);
    let plain = url::Url::parse("http://is.school.cz/").unwrap();
    assert_eq!(jar.cookies(&plain).unwrap().to_str().unwrap(), "p=2");
    let secure = jar.cookies(&url).unwrap();
    assert!(secure.to_str().unwrap().contains("s=1"));
}
