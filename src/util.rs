// Project: theses-mcp
// File: src/util.rs
// Description: Small string helpers that reproduce the Python behaviour the server relies on.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static pattern")
}

pub static REFRESH: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)http-equiv="refresh"[^>]*content="(\d+)"#));
pub static DOC_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(\S+\.(?:pdf|docx?|rtf|odt|zip|txt))\b"));
pub static DOC_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)plný text|full text|final thesis|závěrečná práce|posudek|review|oponent|vedoucí|supervisor|příloha|attachment|hlavní práce|k dispozici|available here",
    )
});
pub static NOT_YET: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)až po obhajobě|po obhajobě práce|after the defen[cs]e"));
pub static STAG_ID: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[?&]praceIdno=(\d+)"));
pub static DISPOSITION: LazyLock<Regex> =
    LazyLock::new(|| re(r#"filename\*?=(?:UTF-8'')?"?([^";]+)"#));

pub const DOC_EXT: [&str; 7] = [".pdf", ".docx", ".doc", ".rtf", ".odt", ".zip", ".txt"];

pub fn has_doc_ext(s: &str) -> bool {
    let lower = s.to_lowercase();
    DOC_EXT.iter().any(|e| lower.ends_with(e))
}

/// Same as `re.sub(r"\s+", " ", s)`.
pub fn squash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars() {
        if c.is_whitespace() {
            gap = true;
        } else {
            if gap {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        }
    }
    if gap {
        out.push(' ');
    }
    out
}

/// Title key used to compare a record with a repository hit: letters and digits only.
///
/// The Python pattern is `[^0-9a-zá-žA-ZÁ-Ž]+`, whose ranges run over the code points
/// U+00E1..U+017E and U+00C1..U+017D. Kept as is so both versions accept the same titles.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| {
            c.is_ascii_alphanumeric()
                || ('\u{e1}'..='\u{17e}').contains(c)
                || ('\u{c1}'..='\u{17d}').contains(c)
        })
        .collect::<String>()
        .to_lowercase()
}

/// `urllib.parse.quote` with its default safe set ("/").
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parts {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

/// `urllib.parse.urlsplit`, including the removal of leading control characters and tabs.
pub fn urlsplit(url: &str, default_scheme: &str) -> Parts {
    let cleaned: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = cleaned.as_str();
    let mut parts = Parts {
        scheme: default_scheme.to_string(),
        ..Parts::default()
    };
    if let Some(i) = rest.find(':')
        && i > 0
        && rest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        parts.scheme = rest[..i].to_lowercase();
        rest = &rest[i + 1..];
    }
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        parts.netloc = after[..end].to_string();
        rest = &after[end..];
    }
    if let Some(i) = rest.find('#') {
        parts.fragment = rest[i + 1..].to_string();
        rest = &rest[..i];
    }
    if let Some(i) = rest.find('?') {
        parts.query = rest[i + 1..].to_string();
        rest = &rest[..i];
    }
    parts.path = rest.to_string();
    parts
}

const USES_RELATIVE: [&str; 19] = [
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms",
    "prospero", "rtsp", "rtspu", "sftp", "svn", "svn+ssh", "ws", "wss",
];
const USES_NETLOC: [&str; 25] = [
    "", "ftp", "http", "gopher", "nntp", "telnet", "imap", "wais", "file", "mms", "https", "shttp",
    "snews", "prospero", "rtsp", "rtspu", "rsync", "svn", "svn+ssh", "sftp", "nfs", "git",
    "git+ssh", "ws", "wss",
];

fn unsplit(p: &Parts) -> String {
    let mut url = p.path.clone();
    let netloc_scheme =
        !p.scheme.is_empty() && USES_NETLOC.contains(&p.scheme.as_str()) && !url.starts_with("//");
    if !p.netloc.is_empty() || netloc_scheme {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{}{}", p.netloc, url);
    }
    if !p.scheme.is_empty() {
        url = format!("{}:{}", p.scheme, url);
    }
    if !p.query.is_empty() {
        url = format!("{url}?{}", p.query);
    }
    if !p.fragment.is_empty() {
        url = format!("{url}#{}", p.fragment);
    }
    url
}

/// `urllib.parse.urljoin`, which `requests.compat` exposes and the Python server used.
pub fn urljoin(base: &str, url: &str) -> String {
    if base.is_empty() {
        return url.to_string();
    }
    if url.is_empty() {
        return base.to_string();
    }
    let b = urlsplit(base, "");
    let mut u = urlsplit(url, &b.scheme);
    if u.scheme != b.scheme || !USES_RELATIVE.contains(&u.scheme.as_str()) {
        return url.to_string();
    }
    if USES_NETLOC.contains(&u.scheme.as_str()) {
        if !u.netloc.is_empty() {
            return unsplit(&u);
        }
        u.netloc = b.netloc.clone();
    }
    if u.path.is_empty() {
        u.path = b.path.clone();
        if u.query.is_empty() {
            u.query = b.query.clone();
        }
        return unsplit(&u);
    }
    let mut base_parts: Vec<&str> = b.path.split('/').collect();
    if base_parts.last() != Some(&"") {
        base_parts.pop();
    }
    let segments: Vec<&str> = if u.path.starts_with('/') {
        u.path.split('/').collect()
    } else {
        let mut all = base_parts;
        all.extend(u.path.split('/'));
        // drop empty inner segments, which would re-join as a doubled slash
        let last = all.len() - 1;
        let mut kept = vec![all[0]];
        if last > 1 {
            kept.extend(all[1..last].iter().copied().filter(|s| !s.is_empty()));
        }
        if last > 0 {
            kept.push(all[last]);
        }
        kept
    };
    let mut resolved: Vec<&str> = Vec::new();
    for seg in &segments {
        match *seg {
            ".." => {
                resolved.pop();
            }
            "." => {}
            s => resolved.push(s),
        }
    }
    if matches!(segments.last(), Some(&".") | Some(&"..")) {
        resolved.push("");
    }
    let path = resolved.join("/");
    u.path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    unsplit(&u)
}

/// Python `str.strip(chars)` for a character set.
pub fn strip_chars<'a>(s: &'a str, set: &[char]) -> &'a str {
    s.trim_matches(|c| set.contains(&c))
}

/// Python truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// What an f-string prints for a JSON value (`None`, `True`, text as is).
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
