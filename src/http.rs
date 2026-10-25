// Project: theses-mcp
// File: src/http.rs
// Description: HTTP transport with the retry policy, cookie jar and text decoding the server needs.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use crate::error::{Error, Result};
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// The most of a page that is read. A school link that turns out to be a multi-gigabyte file
/// must not be pulled into memory; no page or JSON listing the server parses comes close.
pub const MAX_BODY: u64 = 16 * 1024 * 1024;

pub const USER_AGENT: &str = "theses-mcp (+https://github.com/koprjaa/theses-mcp)";

/// One cookie as `cookies.json` stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    pub timeout_secs: u64,
    /// Ask for the first bytes only (`Range: bytes=0-400`) and stop reading after 400.
    pub probe: bool,
    pub accept_json: bool,
    /// Cookies added to this one request, on top of the session's own.
    pub overlay: Vec<Cookie>,
}

impl Request {
    pub fn get(url: impl Into<String>, timeout_secs: u64) -> Self {
        Request {
            url: url.into(),
            timeout_secs,
            probe: false,
            accept_json: false,
            overlay: Vec::new(),
        }
    }

    pub fn with_params(url: &str, params: &[(&str, String)], timeout_secs: u64) -> Self {
        let full = url::Url::parse_with_params(url, params.iter().map(|(k, v)| (*k, v.as_str())))
            .map(String::from)
            .unwrap_or_else(|_| url.to_string());
        Request::get(full, timeout_secs)
    }

    pub fn probe(mut self) -> Self {
        self.probe = true;
        self
    }

    pub fn json(mut self) -> Self {
        self.accept_json = true;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// URL after redirects.
    pub url: String,
    pub content_type: Option<String>,
    pub disposition: Option<String>,
    /// `Retry-After` in seconds, when the server sent a number.
    pub retry_after: Option<u64>,
    pub body: Vec<u8>,
}

impl Response {
    /// The body as text: charset from the header, then from a `<meta>` tag, then UTF-8.
    pub fn text(&self) -> String {
        let label = self
            .content_type
            .as_deref()
            .and_then(charset_of)
            .or_else(|| sniff_meta_charset(&self.body));
        let encoding = label
            .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        encoding.decode(&self.body).0.into_owned()
    }

    pub fn json(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.text()).map_err(|e| Error::new("JSONDecodeError", e.to_string()))
    }
}

fn charset_of(content_type: &str) -> Option<String> {
    let lower = content_type.to_lowercase();
    let at = lower.find("charset=")?;
    let value = lower[at + 8..].split(';').next()?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

fn sniff_meta_charset(body: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&body[..body.len().min(1024)]).to_lowercase();
    let at = head.find("charset=")?;
    let value: String = head[at + 8..]
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    (!value.is_empty()).then_some(value)
}

/// Everything the tools need from the network. Tests replace it with recorded responses.
pub trait Transport: Send + Sync {
    fn get(&self, req: &Request) -> Result<Response>;
    /// Remember a cookie for later requests.
    fn attach(&self, _cookie: &Cookie) {}
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Whether a cookie of `domain` goes to `host`: the host itself or a subdomain, not a mere suffix.
pub fn domain_matches(host: &str, domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_lowercase();
    let host = host.to_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// A cookie store that sends every cookie to its domain and subdomains, as the Python
/// session did, and keeps what the servers set.
#[derive(Default)]
pub struct Jar {
    cookies: Mutex<Vec<Cookie>>,
    /// `(name, domain)` of cookies the server marked `Secure`.
    secure: Mutex<Vec<(String, String)>>,
    overlay: Mutex<Vec<Cookie>>,
}

impl Jar {
    pub fn add(&self, cookie: &Cookie) {
        let mut jar = self.cookies.lock().expect("jar lock");
        let domain = cookie.domain.trim_start_matches('.').to_string();
        jar.retain(|c| !(c.name == cookie.name && c.domain == domain));
        jar.push(Cookie {
            domain,
            ..cookie.clone()
        });
    }

    /// The `Cookie` header value this host would receive.
    pub fn header_for(&self, host: &str) -> Option<String> {
        self.header(host, true)
    }

    /// Same, but a cookie that must not travel in clear text stays home on `http`.
    fn header(&self, host: &str, https: bool) -> Option<String> {
        let jar = self.cookies.lock().expect("jar lock");
        let overlay = self.overlay.lock().expect("jar lock");
        let secure = self.secure.lock().expect("jar lock");
        let mut pairs: Vec<(String, String)> = Vec::new();
        for c in jar.iter().chain(overlay.iter()) {
            let only_https = c.name.starts_with("__Host-")
                || c.name.starts_with("__Secure-")
                || secure.iter().any(|(n, d)| *n == c.name && *d == c.domain);
            if domain_matches(host, &c.domain) && (https || !only_https) {
                pairs.retain(|(n, _)| *n != c.name);
                pairs.push((c.name.clone(), c.value.clone()));
            }
        }
        (!pairs.is_empty()).then(|| {
            pairs
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}

impl reqwest::cookie::CookieStore for Jar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let host = url.host_str().unwrap_or_default().to_string();
        for h in headers {
            let Ok(text) = h.to_str() else { continue };
            let mut parts = text.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
                continue;
            };
            let mut domain = host.clone();
            let mut expired = false;
            let mut refused = false;
            let mut secure = false;
            for attr in parts {
                let attr = attr.trim();
                let lower = attr.to_lowercase();
                if let Some(d) = lower.strip_prefix("domain=") {
                    // like http.cookiejar: no cookie for a Domain without an embedded dot
                    // ("cz"), which would reach every host under it
                    if !d.trim_start_matches('.').contains('.') {
                        refused = true;
                    } else if domain_matches(&host, d) {
                        domain = d.trim_start_matches('.').to_string();
                    }
                } else if lower == "secure" {
                    secure = true;
                } else if lower == "max-age=0" || lower.starts_with("max-age=-") {
                    expired = true;
                }
            }
            if refused {
                continue;
            }
            let cookie = Cookie {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
                domain,
            };
            if secure {
                let mut flagged = self.secure.lock().expect("jar lock");
                if !flagged
                    .iter()
                    .any(|(n, d)| *n == cookie.name && *d == cookie.domain)
                {
                    flagged.push((cookie.name.clone(), cookie.domain.clone()));
                }
            }
            if expired {
                self.cookies
                    .lock()
                    .expect("jar lock")
                    .retain(|c| !(c.name == cookie.name && c.domain == cookie.domain));
            } else {
                self.add(&cookie);
            }
        }
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let host = url.host_str()?;
        // browsers treat loopback as a secure context, and so do the tests' local servers
        let private =
            url.scheme() == "https" || ["localhost", "127.0.0.1", "[::1]"].contains(&host);
        let header = self.header(host, private)?;
        HeaderValue::from_str(&header).ok()
    }
}

/// The real network. Retries like the Python session did (urllib3 `Retry`): three retries
/// of GET on 429, 502, 503, 504 and on connection errors, with a backoff of 0, 3 and 6 s.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
    jar: Arc<Jar>,
    backoff_factor: f64,
    /// The overlay cookies live in the one jar, so a request that carries them runs alone
    /// (write) and every other request runs beside its peers (read). Otherwise a request that
    /// finishes would clear the cookies of one still on its way, and a new one would borrow them.
    gate: RwLock<()>,
}

/// reqwest is built without a bundled crypto provider, and the browser client needs one too.
/// Installing it twice is harmless.
pub fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn backoff_secs(factor: f64, failures: u32) -> f64 {
    if failures <= 1 {
        0.0
    } else {
        factor * 2f64.powi(failures as i32 - 1)
    }
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self::with_backoff(1.5)
    }

    pub fn with_backoff(backoff_factor: f64) -> Self {
        install_crypto();
        let jar = Arc::new(Jar::default());
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .redirect(reqwest::redirect::Policy::limited(30))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        ReqwestTransport {
            client,
            jar,
            backoff_factor,
            gate: RwLock::new(()),
        }
    }

    /// One attempt. The flag says whether a failure is worth another try: urllib3 retried what
    /// went wrong while connecting or waiting for the headers, and nothing after that.
    fn once(&self, req: &Request) -> std::result::Result<Response, (Error, bool)> {
        let fail = |e: &reqwest::Error| {
            let (class, retry) = classify(&req.url, e);
            (Error::new(class, e.to_string()), retry)
        };
        let mut builder = self
            .client
            .get(&req.url)
            .timeout(Duration::from_secs(req.timeout_secs));
        if req.probe {
            builder = builder.header("Range", "bytes=0-400");
        }
        if req.accept_json {
            builder = builder.header("Accept", "application/json");
        }
        let mut resp = builder.send().map_err(|e| fail(&e))?;
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        };
        let (content_type, disposition) = (header("content-type"), header("content-disposition"));
        let retry_after = header("retry-after").and_then(|v| v.trim().parse::<u64>().ok());
        let (status, url) = (resp.status().as_u16(), resp.url().to_string());
        let mut body = Vec::new();
        // a server that ignores Range would send the whole file; stop after the magic bytes
        let limit = if req.probe { 400 } else { MAX_BODY };
        let read = (&mut resp).take(limit).read_to_end(&mut body);
        if let (Err(e), false) = (read, req.probe) {
            return Err((Error::new("ConnectionError", e.to_string()), false));
        }
        Ok(Response {
            status,
            url,
            content_type,
            disposition,
            retry_after,
            body,
        })
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// What `requests` calls a URL it will not send to.
fn url_class(url: &str) -> &'static str {
    match url::Url::parse(url) {
        Err(url::ParseError::RelativeUrlWithoutBase) => "MissingSchema",
        Ok(u) if !matches!(u.scheme(), "http" | "https") => "InvalidSchema",
        _ => "InvalidURL",
    }
}

/// The name `requests` would have given the failure, and whether urllib3 would have tried
/// again. An exhausted read timeout is a `ConnectionError` there, and a bad URL, a redirect
/// loop or a body that cannot be decoded is raised once, not retried.
fn classify(url: &str, e: &reqwest::Error) -> (&'static str, bool) {
    if e.is_builder() {
        return (url_class(url), false);
    }
    if e.is_redirect() {
        return ("TooManyRedirects", false);
    }
    let chain = format!("{e:?}").to_lowercase();
    if e.is_timeout() {
        return (
            if e.is_connect() {
                "ConnectTimeout"
            } else {
                "ConnectionError"
            },
            true,
        );
    }
    if chain.contains("certificate") || chain.contains("handshake") {
        return ("SSLError", true);
    }
    if e.is_decode() {
        return ("ContentDecodingError", false);
    }
    ("ConnectionError", true)
}

impl Transport for ReqwestTransport {
    fn get(&self, req: &Request) -> Result<Response> {
        let (_shared, _alone);
        if req.overlay.is_empty() {
            _shared = self.gate.read().unwrap_or_else(|e| e.into_inner());
        } else {
            _alone = self.gate.write().unwrap_or_else(|e| e.into_inner());
        }
        *self.jar.overlay.lock().expect("jar lock") = req.overlay.clone();
        let mut failures = 0u32;
        let outcome = loop {
            let attempt = self.once(req);
            let mut retry_after = None;
            let failure = match attempt {
                Ok(r) if matches!(r.status, 429 | 502 | 503 | 504) => {
                    retry_after = r.retry_after;
                    Error::new(
                        "RetryError",
                        format!("too many {} error responses", r.status),
                    )
                }
                Ok(r) => break Ok(r),
                Err((e, false)) => break Err(e),
                Err((e, true)) => e,
            };
            failures += 1;
            if failures > 3 {
                break Err(failure);
            }
            let wait =
                retry_after.map_or(backoff_secs(self.backoff_factor, failures), |s| s as f64);
            if wait > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(wait.min(120.0)));
            }
        };
        self.jar.overlay.lock().expect("jar lock").clear();
        outcome
    }

    fn attach(&self, cookie: &Cookie) {
        self.jar.add(cookie);
    }
}
