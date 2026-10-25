// Project: theses-mcp
// File: tests/audit.rs
// Description: Regressions for differences from the Python server found by reading both side by side.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
mod common;

use common::{Mock, reply};
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use theses_mcp::Theses;
use theses_mcp::dispatch::{call_blocking, login_params};
use theses_mcp::http::{Cookie, Jar, Request, ReqwestTransport, Transport};

fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// A server that answers every connection on its own thread, so that one slow answer does
/// not hold up the next request. `route` gets the request line and the raw request.
fn serve_with(route: impl Fn(&str, &str) -> Vec<u8> + Send + Sync + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let route = Arc::new(route);
    thread::spawn(move || {
        for conn in listener.incoming().map_while(Result::ok) {
            let route = route.clone();
            thread::spawn(move || {
                let mut conn: TcpStream = conn;
                let mut buf = vec![0u8; 8192];
                let n = conn.read(&mut buf).unwrap_or(0);
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = raw.lines().next().unwrap_or("").to_string();
                let _ = conn.write_all(&route(&line, &raw));
            });
        }
    });
    port
}

fn http(status: &str, headers: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

// --- arguments ---------------------------------------------------------------

#[test]
fn numbers_and_flags_sent_as_text_are_accepted_as_the_python_validator_did() {
    // pydantic coerces "5" and 5.0 to 5 and "true" to True, and MCP clients do send them that way
    let p = login_params(&args(&[
        ("wait_seconds", json!("5")),
        ("port", json!(9333.0)),
        ("attach", json!("true")),
    ]))
    .unwrap();
    assert_eq!((p.wait_seconds, p.port, p.attach), (5, 9333, true));
    let p = login_params(&args(&[
        ("wait_seconds", json!(" 7 ")),
        ("port", json!("9222.0")),
        ("attach", json!(0)),
    ]))
    .unwrap();
    assert_eq!((p.wait_seconds, p.port, p.attach), (7, 9222, false));

    // what pydantic refuses stays refused
    for (key, value) in [
        ("wait_seconds", json!(5.5)),
        ("wait_seconds", json!("1e2")),
        ("port", json!("abc")),
        ("port", json!(null)),
        ("attach", json!("maybe")),
        ("attach", json!(2)),
        ("attach", json!(" true ")),
    ] {
        assert!(
            login_params(&args(&[(key, value.clone())])).is_err(),
            "{key}={value}"
        );
    }

    let app = common::app_with(Mock::new(|r| panic!("no request expected: {}", r.url)));
    for limit in [json!("0"), json!(0.0), json!(" 0 ")] {
        let r = call_blocking(
            &app,
            "search",
            &args(&[("query", json!("q")), ("limit", limit.clone())]),
        );
        assert!(!r.is_error, "limit {limit}: {}", r.text);
    }
}

#[test]
fn a_huge_wait_does_not_overflow_the_login_deadline() {
    // Instant + Duration panics on overflow; Python added the float and carried on
    let start = Instant::now();
    let deadline = theses_mcp::login::deadline(i64::MAX);
    assert!(deadline > start + Duration::from_secs(86_400));
    assert!(theses_mcp::login::deadline(-5) <= Instant::now());
}

// --- one request's cookies stay with that request ----------------------------

fn cookie_echo_server() -> u16 {
    serve_with(|line, raw| {
        if line.contains("/slow") {
            thread::sleep(Duration::from_millis(600));
            http("302 Found", "Location: /check\r\n", "")
        } else if line.contains("/check") {
            let cookie = raw
                .lines()
                .find(|l| l.to_lowercase().starts_with("cookie:"))
                .unwrap_or("no cookie");
            http("200 OK", "", cookie)
        } else {
            http("200 OK", "", "fast")
        }
    })
}

fn with_cookie(url: String) -> Request {
    let mut req = Request::get(url, 10);
    req.overlay = vec![Cookie {
        name: "k".into(),
        value: "v".into(),
        domain: "127.0.0.1".into(),
    }];
    req
}

#[test]
fn a_request_that_finishes_does_not_take_the_cookie_from_one_still_running() {
    let port = cookie_echo_server();
    let t = Arc::new(ReqwestTransport::with_backoff(0.0));
    let slow = {
        let t = t.clone();
        thread::spawn(move || t.get(&with_cookie(format!("http://127.0.0.1:{port}/slow"))))
    };
    thread::sleep(Duration::from_millis(200));
    t.get(&Request::get(format!("http://127.0.0.1:{port}/fast"), 10))
        .unwrap();
    let r = slow.join().unwrap().unwrap();
    assert!(
        r.text().to_lowercase().contains("cookie: k=v"),
        "the redirect hop lost the cookie: {}",
        r.text()
    );
}

#[test]
fn a_request_does_not_borrow_the_cookie_of_one_running_beside_it() {
    let port = cookie_echo_server();
    let t = Arc::new(ReqwestTransport::with_backoff(0.0));
    let slow = {
        let t = t.clone();
        thread::spawn(move || t.get(&with_cookie(format!("http://127.0.0.1:{port}/slow"))))
    };
    thread::sleep(Duration::from_millis(200));
    let other = t
        .get(&Request::get(format!("http://127.0.0.1:{port}/check"), 10))
        .unwrap();
    slow.join().unwrap().unwrap();
    assert_eq!(other.text(), "no cookie");
}

#[test]
fn a_server_cannot_set_a_cookie_for_a_whole_top_level_domain() {
    // http.cookiejar refuses a Domain without an embedded dot; here such a cookie would go to
    // every .cz host the session talks to afterwards
    let jar = Jar::default();
    let url = url::Url::parse("https://is.school.cz/").unwrap();
    let headers = [
        HeaderValue::from_static("s=1; Domain=cz; Path=/"),
        HeaderValue::from_static("u=3; Domain=.cz"),
        HeaderValue::from_static("t=2; Domain=.school.cz"),
    ];
    jar.set_cookies(&mut headers.iter(), &url);
    assert_eq!(jar.header_for("other.cz"), None);
    assert_eq!(jar.header_for("www.school.cz").as_deref(), Some("t=2"));
}

// --- the failure names a caller sees ------------------------------------------

#[test]
fn a_request_that_never_gets_an_answer_is_a_connection_error_as_in_requests() {
    // with a retry adapter requests reports an exhausted read timeout as ConnectionError
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let held = Arc::new(Mutex::new(Vec::new()));
    let keep = held.clone();
    thread::spawn(move || {
        for conn in listener.incoming().map_while(Result::ok) {
            keep.lock().unwrap().push(conn); // accept, then say nothing
        }
    });
    let err = ReqwestTransport::with_backoff(0.0)
        .get(&Request::get(format!("http://127.0.0.1:{port}/"), 1))
        .unwrap_err();
    assert_eq!(err.class, "ConnectionError");
}

#[test]
fn a_link_that_is_not_a_url_fails_at_once_with_the_name_requests_gives_it() {
    let t = ReqwestTransport::with_backoff(1.5);
    for (url, class) in [
        ("/zp/1234", "MissingSchema"),
        ("ftp://example.cz/x", "InvalidSchema"),
        ("https://theses.cz:abc/", "InvalidURL"),
    ] {
        let started = Instant::now();
        let err = t.get(&Request::get(url, 5)).unwrap_err();
        assert_eq!(err.class, class, "{url}");
        // requests refuses these before sending anything, so there is nothing to retry
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{url} was retried"
        );
    }
}

#[test]
fn a_redirect_loop_is_reported_once_and_not_retried() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let port = serve_with(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        http("302 Found", "Location: /again\r\n", "")
    });
    let err = ReqwestTransport::with_backoff(0.0)
        .get(&Request::get(format!("http://127.0.0.1:{port}/"), 10))
        .unwrap_err();
    assert_eq!(err.class, "TooManyRedirects");
    assert!(
        hits.load(Ordering::SeqCst) <= 40,
        "{}",
        hits.load(Ordering::SeqCst)
    );
}

// --- the JavaScript placeholder page ------------------------------------------

const PLACEHOLDER: &str = "<!DOCTYPE HTML><html><head><script>\n(async function() {\n\
    const random = new Uint32Array(1);\n    crypto.getRandomValues(random);\n})();\n\
    </script></head><body></body></html>";

fn signed_in_page() -> String {
    format!("<html><body>{}</body></html>", "Obsah stránky. ".repeat(60))
}

fn app_over(answers: Vec<String>) -> (Theses, Arc<AtomicUsize>, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let seen = asked.clone();
    let mock = Mock::new(move |_| {
        let i = seen.fetch_add(1, Ordering::SeqCst);
        Ok(reply(
            answers[i.min(answers.len() - 1)].as_bytes(),
            "text/html; charset=utf-8",
        ))
    });
    let app = Theses::new(mock, home.path().join(".theses-mcp"), None);
    (app, asked, home)
}

#[test]
fn a_cookie_is_checked_after_the_placeholder_page_has_been_waited_out() {
    // the first request of a session meets the placeholder; reading it as the answer would
    // refuse every cookie pasted at the start of a session
    let (app, asked, _home) = app_over(vec![PLACEHOLDER.into(), signed_in_page()]);
    let out = app.use_cookie("theses.cz", "abc").unwrap();
    assert_eq!(out["result"], "signed in", "{out}");
    assert_eq!(asked.load(Ordering::SeqCst), 2);

    let (app, _, _home) = app_over(vec![PLACEHOLDER.into(), signed_in_page()]);
    let cookie = Cookie {
        name: "s".into(),
        value: "1".into(),
        domain: "theses.cz".into(),
    };
    assert!(app.verify(&[cookie], "theses.cz", None));
}

#[test]
fn a_placeholder_that_never_goes_away_is_given_up_on() {
    let (app, asked, _home) = app_over(vec![PLACEHOLDER.into()]);
    let out = app.use_cookie("theses.cz", "abc").unwrap();
    assert_eq!(out["result"], "that cookie does not sign you in");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        4,
        "four looks, as get_page takes"
    );
}

// --- how much of a page is read ------------------------------------------------

#[test]
fn a_page_is_read_up_to_a_limit_and_no_further() {
    // a school link that turns out to be a huge file must not be pulled into memory whole
    let port = serve_with(|_, _| {
        let body = "x".repeat(20 * 1024 * 1024);
        http("200 OK", "Content-Type: text/html\r\n", &body)
    });
    let r = ReqwestTransport::with_backoff(0.0)
        .get(&Request::get(format!("http://127.0.0.1:{port}/big"), 30))
        .unwrap();
    assert!(r.body.len() <= 16 * 1024 * 1024, "{}", r.body.len());
    assert!(r.body.len() > 1024 * 1024);
}
