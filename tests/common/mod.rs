// Project: theses-mcp
// File: tests/common/mod.rs
// Description: Replays recorded HTTP responses so every test runs offline.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
#![allow(dead_code)]

use base64::Engine;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use theses_mcp::error::{Error, Result};
use theses_mcp::http::{Cookie, Request, Response, Transport};

/// Request key shared with the recorder: `scheme://host/path?sorted decoded query`, plus
/// ` [range]` for a probe. Encoding differences between the two clients drop out.
pub fn key_of(url: &str, probe: bool) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let netloc = rest[..end].to_lowercase();
    let tail = &rest[end..];
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    let qs = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let path = if path.is_empty() { "/" } else { path };
    let mut key = format!("{scheme}://{netloc}{path}");
    if !qs.is_empty() {
        key.push('?');
        key.push_str(&qs);
    }
    if probe {
        key.push_str(" [range]");
    }
    key
}

pub struct Fixture {
    table: HashMap<String, Vec<Value>>,
    next: Mutex<HashMap<String, usize>>,
    pub log: Mutex<Vec<String>>,
    pub attached: Mutex<Vec<Cookie>>,
}

impl Fixture {
    pub fn new(responses: &[Value]) -> Arc<Self> {
        let mut table: HashMap<String, Vec<Value>> = HashMap::new();
        for r in responses {
            table
                .entry(r["key"].as_str().expect("key").to_string())
                .or_default()
                .push(r.clone());
        }
        Arc::new(Fixture {
            table,
            next: Mutex::new(HashMap::new()),
            log: Mutex::new(Vec::new()),
            attached: Mutex::new(Vec::new()),
        })
    }
}

impl Transport for Fixture {
    fn get(&self, req: &Request) -> Result<Response> {
        let key = key_of(&req.url, req.probe);
        let mut extra: Vec<String> = req
            .overlay
            .iter()
            .map(|c| format!("{}={}@{}", c.name, c.value, c.domain))
            .collect();
        extra.sort();
        let logged = if extra.is_empty() {
            key.clone()
        } else {
            format!("{key} cookies={}", extra.join(","))
        };
        self.log.lock().unwrap().push(logged);
        let list = self
            .table
            .get(&key)
            .ok_or_else(|| Error::new("ConnectionError", format!("no fixture: {key}")))?;
        let mut next = self.next.lock().unwrap();
        let i = next.entry(key).or_insert(0);
        let rec = &list[(*i).min(list.len() - 1)];
        *i += 1;
        let body = match rec.get("body_b64").and_then(Value::as_str) {
            Some(b) => base64::engine::general_purpose::STANDARD
                .decode(b)
                .expect("base64"),
            None => rec["body"].as_str().expect("body").as_bytes().to_vec(),
        };
        let header = |name: &str| {
            rec["headers"]
                .get(name)
                .and_then(Value::as_str)
                .map(String::from)
        };
        Ok(Response {
            status: rec["status"].as_u64().unwrap_or(200) as u16,
            url: rec["url"].as_str().unwrap_or(&req.url).to_string(),
            content_type: header("Content-Type"),
            disposition: header("Content-Disposition"),
            retry_after: header("Retry-After").and_then(|v| v.parse().ok()),
            body,
        })
    }

    fn attach(&self, cookie: &Cookie) {
        self.attached.lock().unwrap().push(cookie.clone());
    }

    fn sleep(&self, _d: Duration) {}
}

pub fn args_of(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// A transport backed by a closure, for tests that care about one call at a time.
pub struct Mock<F: Fn(&Request) -> Result<Response> + Send + Sync>(pub F, pub Mutex<Vec<Request>>);

impl<F: Fn(&Request) -> Result<Response> + Send + Sync> Mock<F> {
    pub fn new(f: F) -> Arc<Self> {
        Arc::new(Mock(f, Mutex::new(Vec::new())))
    }

    pub fn calls(&self) -> Vec<Request> {
        self.1.lock().unwrap().clone()
    }
}

impl<F: Fn(&Request) -> Result<Response> + Send + Sync> Transport for Mock<F> {
    fn get(&self, req: &Request) -> Result<Response> {
        self.1.lock().unwrap().push(req.clone());
        (self.0)(req)
    }

    fn sleep(&self, _d: Duration) {}
}

pub fn reply(body: &[u8], content_type: &str) -> Response {
    Response {
        status: 200,
        url: String::new(),
        content_type: Some(content_type.to_string()),
        disposition: None,
        retry_after: None,
        body: body.to_vec(),
    }
}

pub fn app_with(t: Arc<dyn Transport>) -> Theses {
    let dir = std::env::temp_dir().join("theses-mcp-test-never-written");
    Theses::new(t, dir, None)
}

use theses_mcp::Theses;
