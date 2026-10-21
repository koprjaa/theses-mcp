// Project: theses-mcp
// File: tests/transport.rs
// Description: The real HTTP transport against a local server: retries, cookies, ranged probes, text decoding.
// Author: Jan Alexandr Kopřiva
// License: MIT

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use theses_mcp::http::{Cookie, Jar, Request, ReqwestTransport, Response, Transport, backoff_secs};

/// Serve one scripted answer per connection and record what was asked.
fn serve(answers: Vec<String>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for answer in answers {
            let Ok((mut conn, _)) = listener.accept() else {
                return;
            };
            let mut buf = vec![0u8; 8192];
            let n = conn.read(&mut buf).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let _ = conn.write_all(answer.as_bytes());
            let _ = conn.flush();
        }
    });
    (port, seen)
}

fn http(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn transport() -> ReqwestTransport {
    ReqwestTransport::with_backoff(0.0)
}

#[test]
fn the_backoff_schedule_is_the_one_the_python_session_used() {
    let waits: Vec<f64> = (1..=3).map(|n| backoff_secs(1.5, n)).collect();
    assert_eq!(waits, [0.0, 3.0, 6.0]);
}

#[test]
fn a_503_is_retried_until_the_server_gives_a_page() {
    let (port, seen) = serve(vec![
        http("503 Service Unavailable", "", "busy"),
        http("503 Service Unavailable", "", "busy"),
        http(
            "200 OK",
            "Content-Type: text/html; charset=utf-8\r\n",
            "ready",
        ),
    ]);
    let r = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/x"), 10))
        .unwrap();
    assert_eq!((r.status, r.text().as_str()), (200, "ready"));
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[test]
fn three_retries_and_then_the_failure_is_reported() {
    let busy = || http("503 Service Unavailable", "", "busy");
    let (port, seen) = serve(vec![busy(), busy(), busy(), busy(), busy()]);
    let err = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/x"), 10))
        .unwrap_err();
    assert_eq!(err.class, "RetryError");
    assert_eq!(seen.lock().unwrap().len(), 4, "one try and three retries");
}

#[test]
fn a_500_is_not_retried() {
    // repositories answer 500 for queries they cannot parse; repeating burns time for the same failure
    let (port, seen) = serve(vec![
        http("500 Internal Server Error", "", "no"),
        http("200 OK", "", "late"),
    ]);
    let r = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/x"), 10))
        .unwrap();
    assert_eq!(r.status, 500);
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn a_closed_port_is_a_connection_error_after_the_retries() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/"), 5))
        .unwrap_err();
    assert_eq!(err.class, "ConnectionError");
}

#[test]
fn a_session_cookie_set_by_the_server_comes_back_on_the_next_request() {
    let (port, seen) = serve(vec![
        http(
            "200 OK",
            "Set-Cookie: __Host-issession=abc123; path=/; secure; HttpOnly\r\n",
            "one",
        ),
        http("200 OK", "", "two"),
    ]);
    let t = transport();
    let url = format!("http://127.0.0.1:{port}/");
    t.get(&Request::get(url.clone(), 10)).unwrap();
    t.get(&Request::get(url, 10)).unwrap();
    let asked = seen.lock().unwrap().clone();
    assert!(!asked[0].to_lowercase().contains("cookie:"));
    assert!(
        asked[1]
            .to_lowercase()
            .contains("cookie: __host-issession=abc123"),
        "{}",
        asked[1]
    );
}

#[test]
fn a_cookie_given_for_one_request_is_not_kept() {
    let (port, seen) = serve(vec![http("200 OK", "", "a"), http("200 OK", "", "b")]);
    let t = transport();
    let url = format!("http://127.0.0.1:{port}/");
    let mut with = Request::get(url.clone(), 10);
    with.overlay = vec![Cookie {
        name: "k".into(),
        value: "v".into(),
        domain: "127.0.0.1".into(),
    }];
    t.get(&with).unwrap();
    t.get(&Request::get(url, 10)).unwrap();
    let asked = seen.lock().unwrap().clone();
    assert!(
        asked[0].to_lowercase().contains("cookie: k=v"),
        "{}",
        asked[0]
    );
    assert!(!asked[1].to_lowercase().contains("cookie:"), "{}", asked[1]);
}

#[test]
fn the_user_agent_names_the_project() {
    let (port, seen) = serve(vec![http("200 OK", "", "x")]);
    transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/"), 10))
        .unwrap();
    assert!(
        seen.lock().unwrap()[0].contains("theses-mcp (+https://github.com/koprjaa/theses-mcp)")
    );
}

#[test]
fn a_probe_asks_for_the_first_bytes_and_stops_reading_at_400() {
    // a server that ignores Range sends the whole file; only the magic bytes are wanted
    let (port, seen) = serve(vec![http(
        "200 OK",
        "Content-Type: application/pdf\r\n",
        &"%PDF".repeat(5000),
    )]);
    let r = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/f"), 10).probe())
        .unwrap();
    assert!(
        seen.lock().unwrap()[0]
            .to_lowercase()
            .contains("range: bytes=0-400")
    );
    assert_eq!(r.body.len(), 400);
    assert!(r.body.starts_with(b"%PDF"));
}

#[test]
fn a_json_request_says_so() {
    let (port, seen) = serve(vec![http("200 OK", "", "{}")]);
    transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/"), 10).json())
        .unwrap();
    assert!(
        seen.lock().unwrap()[0]
            .to_lowercase()
            .contains("accept: application/json")
    );
}

#[test]
fn a_redirect_is_followed_and_the_final_url_is_reported() {
    let (port, _) = serve(vec![
        http("302 Found", "Location: /there\r\n", ""),
        http("200 OK", "", "arrived"),
    ]);
    let r = transport()
        .get(&Request::get(format!("http://127.0.0.1:{port}/here"), 10))
        .unwrap();
    assert!(r.url.ends_with("/there"), "{}", r.url);
    assert_eq!(r.text(), "arrived");
}

// --- text decoding ------------------------------------------------------------

fn body(bytes: &[u8], content_type: Option<&str>) -> Response {
    Response {
        status: 200,
        url: String::new(),
        content_type: content_type.map(String::from),
        disposition: None,
        retry_after: None,
        body: bytes.to_vec(),
    }
}

#[test]
fn the_charset_in_the_header_wins() {
    let latin2 = [0x50, 0xF8, 0xED, 0x6C, 0x69, 0xB9]; // "Příliš" in ISO-8859-2
    assert_eq!(
        body(&latin2, Some("text/html; charset=iso-8859-2")).text(),
        "Příliš"
    );
}

#[test]
fn a_page_without_a_charset_header_is_read_from_its_meta_tag() {
    let page = b"<meta charset=\"windows-1250\"><p>\xe8\xe1p</p>";
    assert!(body(page, Some("text/html")).text().contains("čáp"));
}

#[test]
fn a_page_with_no_charset_anywhere_is_utf8() {
    // requests would have decoded text/html without a charset as ISO-8859-1 and returned mojibake
    assert_eq!(
        body("Výsledky 1 – 10".as_bytes(), Some("text/html")).text(),
        "Výsledky 1 – 10"
    );
    assert_eq!(body("Výsledky".as_bytes(), None).text(), "Výsledky");
}

// --- which host gets a cookie -------------------------------------------------

#[test]
fn a_cookie_goes_to_its_domain_and_its_subdomains() {
    let jar = Jar::default();
    jar.add(&Cookie {
        name: "s".into(),
        value: "1".into(),
        domain: ".is.vsfs.cz".into(),
    });
    assert_eq!(jar.header_for("is.vsfs.cz").as_deref(), Some("s=1"));
    assert_eq!(jar.header_for("www.is.vsfs.cz").as_deref(), Some("s=1"));
    assert_eq!(jar.header_for("IS.VSFS.CZ").as_deref(), Some("s=1"));
}

#[test]
fn a_cookie_does_not_leak_to_a_host_that_merely_ends_the_same_way() {
    // same as the Python session: scoping is by whole labels, never by suffix
    let jar = Jar::default();
    jar.add(&Cookie {
        name: "s".into(),
        value: "1".into(),
        domain: "theses.cz".into(),
    });
    assert_eq!(jar.header_for("xtheses.cz"), None);
    assert_eq!(jar.header_for("cz"), None);
    assert_eq!(jar.header_for("example.com"), None);
}
