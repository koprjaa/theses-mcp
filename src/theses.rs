// Project: theses-mcp
// File: src/theses.rs
// Description: The search, detail and fulltext tools: theses.cz parsing and school repository lookups.
// Author: Jan Alexandr Kopřiva
// License: MIT

use crate::dom::{Page, compact_text, node_text, sel, sibling_text};
use crate::error::{Error, Result};
use crate::http::{Request, Response};
use crate::util::{
    DISPOSITION, DOC_LABEL, DOC_RE, NOT_YET, REFRESH, STAG_ID, has_doc_ext, norm, py_str, quote,
    strip_chars, truthy, urljoin, urlsplit,
};
use crate::{Theses, session::is_gated};
use regex::Regex;
use scraper::ElementRef;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

pub const BASE: &str = "https://theses.cz";

type Obj = Map<String, Value>;

const MAGIC: [(&[u8], &str); 4] = [
    (b"%PDF", ".pdf"),
    (b"PK\x03\x04", ".docx"),
    (b"{\\rtf", ".rtf"),
    (b"\xd0\xcf\x11\xe0", ".doc"),
];

/// Schools whose theses.cz records point nowhere, or only at a study-information system
/// that holds no files, but which run a public repository of their own.
pub const REPOSITORY: [(&str, &str, &str); 10] = [
    ("České vysoké učení technické", "dspace.cvut.cz", "dspace7"),
    (
        "Vysoké učení technické v Brně",
        "dspace.vutbr.cz",
        "dspace7",
    ),
    ("Vysoká škola báňská", "dspace.vsb.cz", "dspace7"),
    ("Technická univerzita v Liberci", "dspace.tul.cz", "dspace7"),
    ("Univerzita Pardubice", "dk.upce.cz", "dspace7"),
    ("Západočeská univerzita", "dspace.zcu.cz", "dspace7"),
    ("Univerzita Karlova", "dspace.cuni.cz", "dspace6"),
    ("Jihočeská univerzita", "dspace.jcu.cz", "dspace6"),
    ("Univerzita Tomáše Bati", "digilib.k.utb.cz", "dspace5"),
    (
        "Vysoká škola chemicko-technologická",
        "repozitar.vscht.cz",
        "invenio",
    ),
];

/// ZČU serves the interface and the REST backend from different hosts.
fn repository_api(host: &str) -> &str {
    if host == "dspace.zcu.cz" {
        "naos-be.zcu.cz"
    } else {
        host
    }
}

const STAG_PAGES: [(&str, Option<&str>); 4] = [
    ("ssProhlizeniElPodobaVSKPPage", None),
    ("ssProhlizeniElPodobaVSKPPrilohyPage", None),
    (
        "ssProhlizeniPosudkyVSKPPage",
        Some("PROHLIZENI_VSKP_POSUDKY_OPONENTA_K_VSKP"),
    ),
    (
        "ssProhlizeniPosudkyVSKPPage",
        Some("PROHLIZENI_VSKP_POSUDKY_VEDOUCIHO_K_VSKP"),
    ),
];

static RESULTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Výsledky\s+\d+\s*–\s*\d+\s+z\s+([\d\s]+)").expect("static"));
static STAHNOUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*St[áa]hnout\s*$").expect("static"));
static ID_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/id/(\w+)").expect("static"));

fn attr(el: ElementRef, name: &str) -> Result<String> {
    el.value()
        .attr(name)
        .map(String::from)
        .ok_or_else(|| Error::new("KeyError", name))
}

fn last_segment(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn head_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Seconds to wait if a response is a placeholder instead of the page: the meta refresh
/// stub the registry served before, or the JavaScript proof-of-work page it serves now.
pub fn stub_delay(head: &str) -> Option<u64> {
    if let Some(m) = REFRESH.captures(head) {
        let n = m[1].parse::<u64>().unwrap_or(u64::MAX);
        return Some(n.clamp(1, 3));
    }
    head.contains("crypto.getRandomValues").then_some(3)
}

/// Code of a record from a code, `/id/<code>/` or a URL.
pub fn record_code(id_or_url: &str) -> String {
    let trimmed = strip_chars(id_or_url, &['/', ' ']);
    last_segment(trimmed).to_string()
}

fn norm_json(v: Option<&Value>) -> Result<String> {
    match v {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(norm(s)),
        Some(_) => Err(Error::new(
            "TypeError",
            "expected string or bytes-like object",
        )),
    }
}

fn missing(key: &str) -> Error {
    Error::new("KeyError", key)
}

fn get_obj<'a>(v: &'a Value, key: &str) -> Result<&'a Value> {
    v.get(key).ok_or_else(|| missing(key))
}

impl Theses {
    /// GET a page, waiting out the placeholder pages the registry answers with under load.
    pub fn get_page(&self, url: &str) -> Result<Page> {
        let mut last: Option<Response> = None;
        for _ in 0..4 {
            let r = self.http.get(&Request::get(url, 30))?;
            let delay = stub_delay(head_chars(&r.text(), 400));
            last = Some(r);
            match delay {
                None => break,
                Some(d) => self.http.sleep(Duration::from_secs(d)),
            }
        }
        let r = last.expect("at least one attempt");
        Ok(Page::parse(&r.text(), &r.url))
    }

    pub fn search(&self, query: &str, limit: i64) -> Result<Value> {
        let mut out: Vec<Value> = Vec::new();
        let mut total: Option<i64> = None;
        let upto = limit.min(50);
        let mut start = 1;
        while start <= upto {
            let page = self.get_page(&format!(
                "{BASE}/vyhledavani/?search={}&start={start}",
                quote(query)
            ))?;
            if total.is_none()
                && let Some(m) = RESULTS.captures(&page.text())
            {
                let digits: String = m[1].chars().filter(char::is_ascii_digit).collect();
                total = Some(
                    digits
                        .parse()
                        .map_err(|_| Error::new("ValueError", "invalid literal for int()"))?,
                );
            }
            let items = page.select(".vyh_polozka");
            if items.is_empty() {
                break;
            }
            for it in items {
                let Some(a) = it.select(&sel("h4 a")).next() else {
                    continue;
                };
                let href = attr(a, "href")?;
                let fulltext = it.value().attr("data-agenda") != Some("T");
                let rec = it.select(&sel(".archiv a")).next();
                let h4 = it.select(&sel("h4")).next();
                let author = h4.map(sibling_text).unwrap_or_default();
                let mut hit = Obj::new();
                hit.insert(
                    "kind".into(),
                    json!(if fulltext { "fulltext" } else { "record" }),
                );
                hit.insert("title".into(), json!(node_text(Some(a), &[])));
                hit.insert(
                    "author".into(),
                    if author.is_empty() {
                        Value::Null
                    } else {
                        json!(author)
                    },
                );
                hit.insert(
                    "meta".into(),
                    json!(node_text(it.select(&sel(".vyh_hlavicky")).next(), &[])),
                );
                hit.insert(
                    "snippet".into(),
                    json!(node_text(it.select(&sel(".vyh_text")).next(), &[])),
                );
                if fulltext {
                    hit.insert(
                        "pdf_url".into(),
                        json!(href.split('?').next().unwrap_or("")),
                    );
                    hit.insert(
                        "url".into(),
                        match rec {
                            Some(r) => json!(format!("{BASE}{}", attr(r, "href")?)),
                            None => Value::Null,
                        },
                    );
                } else {
                    // the title of a record sometimes links straight at a file path, and that
                    // path is often dead. The record page is always /id/<code>/.
                    let url = match ID_PREFIX.captures(&href) {
                        Some(c) => format!("{BASE}/id/{}/", &c[1]),
                        None => format!("{BASE}{}", href.split('?').next().unwrap_or("")),
                    };
                    hit.insert("url".into(), json!(url));
                }
                out.push(Value::Object(hit));
            }
            if out.len() as i64 >= limit {
                break;
            }
            start += 10;
        }
        out.truncate(limit.max(0) as usize);
        Ok(json!({"total": total, "returned": out.len(), "results": out}))
    }

    fn record_error(page: &Page, code: &str) -> Value {
        let hidden = page.text().contains("Chybná adresa v ISu");
        json!({"error": if hidden {
            format!("record {code} is not publicly viewable on theses.cz")
        } else {
            format!("record {code} not found")
        }})
    }

    pub fn detail(&self, id_or_url: &str) -> Result<Value> {
        let code = record_code(id_or_url);
        let page = self.get_page(&format!("{BASE}/id/{code}/"))?;
        let Some(meta) = page.select_one("#metadata") else {
            return Ok(Self::record_error(&page, &code));
        };
        let one = |css: &str| node_text(meta.select(&sel(css)).next(), &[]);
        let anot: Vec<ElementRef> = meta.select(&sel(".anotace")).collect();

        let mut kw = Obj::new();
        for blok in meta.select(&sel(".klslova")) {
            let label = match blok.select(&sel("h3")).next() {
                Some(h3) => compact_text(h3),
                None => "?".to_string(),
            };
            let words: Vec<Value> = blok
                .select(&sel("span.tg4"))
                .map(|s| json!(compact_text(s)))
                .collect();
            kw.insert(label, Value::Array(words));
        }

        let mut servis: Vec<(String, String)> = Vec::new();
        for col in meta.select(&sel(".servis_info > div")) {
            let t = node_text(Some(col), &[]);
            if let Some((k, v)) = t.split_once(':') {
                servis.retain(|(key, _)| key != k.trim());
                servis.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let servis_get = |k: &str| {
            servis
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| json!(v))
        };

        // the first h3 that mentions the defense, then every list item beside it
        let defense: Vec<String> = meta
            .select(&sel("h3"))
            .find(|h| h.text().collect::<String>().contains("Obhajoba"))
            .and_then(|h| h.parent().and_then(ElementRef::wrap))
            .map(|p| {
                p.select(&sel("ul li"))
                    .map(|li| node_text(Some(li), &[]))
                    .collect()
            })
            .unwrap_or_default();

        let plny = page.select_one(".plny_text_th");
        let ext = page.select_one(".plny_text_ext a");
        let mut related = Vec::new();
        for a in page.select("#th-sloupec .oddil a") {
            if let Some(href) = a.value().attr("href")
                && href.starts_with("/id/")
            {
                related.push(
                    json!({"title": node_text(Some(a), &[]), "url": format!("{BASE}{href}")}),
                );
            }
        }
        Ok(json!({
            "url": format!("{BASE}/id/{code}/"),
            "author": one(".th-autor"),
            "type": one(".typ-prace"),
            "title": one(".th-title"),
            "title_translated": one(".transl"),
            "abstract_cs": anot.first().map(|a| node_text(Some(*a), &[])).unwrap_or_default(),
            "abstract_en": anot.get(1).map(|a| node_text(Some(*a), &[])).unwrap_or_default(),
            "keywords": kw,
            "defense": defense.into_iter().filter(|d| !d.is_empty()).collect::<Vec<_>>(),
            "language": servis_get("Jazyk práce"),
            "submitted": servis_get("Datum vytvoření / odevzdání či podání práce"),
            "school": node_text(page.select_one("#th-sloupec .oddil"), &[]),
            "fulltext_access": node_text(plny, &[]),
            "archive_url": ext.and_then(|e| e.value().attr("href")),
            "related": related,
        }))
    }

    /// Describe `url` if it serves a file rather than a landing page.
    ///
    /// theses.cz is inconsistent about what it links to: VŠE points at an HTML page that
    /// lists downloads, MENDELU points straight at the PDF. Sniff the first bytes instead of
    /// fetching a multi-megabyte file and trying to parse it as HTML.
    pub fn as_document(&self, url: &str) -> Option<Obj> {
        let mut resp = None;
        for _ in 0..4 {
            let r = self.http.get(&Request::get(url, 25).probe()).ok()?;
            let head: String =
                String::from_utf8_lossy(&r.body[..r.body.len().min(400)]).into_owned();
            let delay = stub_delay(&head);
            resp = Some(r);
            match delay {
                None => break,
                Some(d) => self.http.sleep(Duration::from_secs(d)),
            }
        }
        let r = resp?;
        let (_, ext) = MAGIC.iter().find(|(m, _)| r.body.starts_with(m))?;
        let name = match r
            .disposition
            .as_deref()
            .and_then(|d| DISPOSITION.captures(d))
        {
            Some(c) => c[1].to_string(),
            None => format!("{}{ext}", last_segment(url.trim_end_matches('/'))),
        };
        let mut doc = Obj::new();
        doc.insert("label".into(), json!("Plný text práce"));
        doc.insert("filename".into(), json!(name));
        doc.insert("url".into(), json!(url));
        doc.insert("confirmed".into(), json!(true));
        Some(doc)
    }

    /// Collect downloadable files from a school repository page.
    ///
    /// Extensions in the href are the easy case (is.muni.cz). Others hide the file behind an
    /// extensionless URL and put the name in the link text, so the label is parsed too, and
    /// anything still ambiguous is confirmed with a ranged GET.
    pub fn documents(&self, page: &Page, base: &str) -> Vec<Obj> {
        let mut out: Vec<Obj> = Vec::new();
        let mut probe: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for a in page.select("a[href]") {
            let href = urljoin(base, a.value().attr("href").unwrap_or(""));
            if seen.contains(&href) || href.starts_with("mailto:") {
                continue;
            }
            let label = node_text(Some(a), &[]);
            let named = DOC_RE.captures(&label).map(|c| c[1].to_string());
            let by_href = has_doc_ext(href.split('?').next().unwrap_or(""));
            if by_href || named.is_some() {
                seen.insert(href.clone());
                // STAG names files with spaces and makes the label nothing else, while VŠE
                // wraps the name in "Hlavní práce … 1.5 MB Stáhnout". A label that ends at
                // the extension is the whole filename.
                let whole = has_doc_ext(&label);
                let tail = last_segment(&href).to_string();
                let cleaned = STAHNOUT.replace(&label, "").into_owned();
                let mut f = Obj::new();
                f.insert(
                    "label".into(),
                    json!(if cleaned.is_empty() {
                        tail.clone()
                    } else {
                        cleaned
                    }),
                );
                f.insert(
                    "filename".into(),
                    json!(if whole {
                        label.clone()
                    } else {
                        named.unwrap_or(tail)
                    }),
                );
                f.insert("url".into(), json!(href));
                out.push(f);
            } else if DOC_LABEL.is_match(&label) {
                seen.insert(href.clone());
                probe.push((label, href));
            }
        }
        for f in out.iter_mut() {
            let url = f["url"].as_str().unwrap_or("").to_string();
            if has_doc_ext(url.split('?').next().unwrap_or("")) {
                continue;
            }
            f.insert("confirmed".into(), json!(self.as_document(&url).is_some()));
        }
        for (label, href) in probe {
            if let Some(mut doc) = self.as_document(&href) {
                doc.insert("label".into(), json!(label));
                out.push(doc);
            }
        }
        out
    }

    fn dspace7(&self, host: &str, title: &str) -> Result<Option<(String, Vec<Obj>)>> {
        let api = repository_api(host);
        let req = Request::with_params(
            &format!("https://{api}/server/api/discover/search/objects"),
            &[
                ("query", title.to_string()),
                ("dsoType", "item".into()),
                ("size", "5".into()),
            ],
            30,
        );
        let body = self.http.get(&req)?.json()?;
        let objects = get_obj(
            get_obj(
                get_obj(get_obj(&body, "_embedded")?, "searchResult")?,
                "_embedded",
            )?,
            "objects",
        )?;
        for o in objects
            .as_array()
            .ok_or_else(|| Error::new("TypeError", "objects"))?
        {
            let item = o
                .get("_embedded")
                .and_then(|e| e.get("indexableObject"))
                .cloned()
                .unwrap_or(json!({}));
            if norm_json(item.get("name"))? != norm(title) {
                continue;
            }
            let uuid = py_str(get_obj(&item, "uuid")?);
            let page_url = format!("https://{host}/items/{uuid}");
            let soup = self.get_page(&page_url)?;
            let files = self.documents(&soup, &soup.url);
            return Ok(Some((page_url, files)));
        }
        Ok(None)
    }

    /// DSpace 6: the REST API hands over the bitstreams directly. Only the ORIGINAL bundle is
    /// wanted; DSpace also stores an extracted-text copy of every PDF and a thumbnail.
    fn dspace6(&self, host: &str, title: &str) -> Result<Option<(String, Vec<Obj>)>> {
        let req = Request::with_params(
            &format!("https://{host}/rest/filtered-items"),
            &[
                ("query_field[]", "dc.title".into()),
                ("query_op[]", "contains".into()),
                ("query_val[]", title.to_string()),
                ("limit", "5".into()),
                ("expand", "bitstreams".into()),
            ],
            30,
        )
        .json();
        let body = self.http.get(&req)?.json()?;
        let root = body
            .as_object()
            .ok_or_else(|| Error::new("AttributeError", "list has no get"))?;
        let items = root.get("items").cloned().unwrap_or(json!([]));
        for item in items
            .as_array()
            .ok_or_else(|| Error::new("TypeError", "items"))?
        {
            if norm_json(item.get("name"))? != norm(title) {
                continue;
            }
            let mut files: Vec<Obj> = Vec::new();
            let streams = item
                .get("bitstreams")
                .filter(|v| truthy(v))
                .cloned()
                .unwrap_or(json!([]));
            for b in streams
                .as_array()
                .ok_or_else(|| Error::new("TypeError", "bitstreams"))?
            {
                if b.get("bundleName") != Some(&json!("ORIGINAL"))
                    || b.get("mimeType") == Some(&json!("text/plain"))
                {
                    continue;
                }
                let url = format!("https://{host}{}", py_str(get_obj(b, "retrieveLink")?));
                let name = b.get("name").cloned().unwrap_or(json!(""));
                let mut f = self.as_document(&url).unwrap_or_else(|| {
                    let mut o = Obj::new();
                    o.insert("label".into(), name.clone());
                    o.insert("filename".into(), name.clone());
                    o.insert("url".into(), json!(url));
                    o.insert("confirmed".into(), json!(false));
                    o
                });
                let label = match b.get("description") {
                    Some(d) if truthy(d) => d.clone(),
                    _ => name,
                };
                f.insert("label".into(), label);
                files.push(f);
            }
            let handle = py_str(item.get("handle").unwrap_or(&Value::Null));
            return Ok(Some((format!("https://{host}/handle/{handle}"), files)));
        }
        Ok(None)
    }

    /// DSpace 5: no searchable REST API, but the XMLUI search page is server-rendered. The
    /// search lives at /discover; /simple-search answers 200 with a login page and no results.
    fn dspace5(&self, host: &str, title: &str) -> Result<Option<(String, Vec<Obj>)>> {
        let req = Request::with_params(
            &format!("https://{host}/discover"),
            &[("query", title.to_string())],
            30,
        );
        let resp = self.http.get(&req)?;
        let listing = Page::parse(&resp.text(), &resp.url);
        for a in listing.select("a[href*='/handle/']") {
            if norm(&node_text(Some(a), &[])) != norm(title) {
                continue;
            }
            let page_url = urljoin(&format!("https://{host}/"), &attr(a, "href")?);
            let soup = self.get_page(&page_url)?;
            let files = self.documents(&soup, &soup.url);
            return Ok(Some((page_url, files)));
        }
        Ok(None)
    }

    /// Invenio: VŠCHT runs one of these instead of a DSpace. Records list a cover image
    /// alongside the thesis, so keep only real documents.
    fn invenio(&self, host: &str, title: &str) -> Result<Option<(String, Vec<Obj>)>> {
        let req = Request::with_params(
            &format!("https://{host}/api/theses"),
            &[("q", title.to_string()), ("size", "5".into())],
            30,
        );
        let body = self.http.get(&req)?.json()?;
        let hits = get_obj(get_obj(&body, "hits")?, "hits")?;
        for hit in hits
            .as_array()
            .ok_or_else(|| Error::new("TypeError", "hits"))?
        {
            let meta = hit.get("metadata").filter(|v| truthy(v)).unwrap_or(hit);
            let mut name = meta.get("title").cloned();
            if let Some(Value::Object(o)) = &name {
                name = o
                    .get("cs")
                    .filter(|v| truthy(v))
                    .cloned()
                    .or_else(|| o.values().next().cloned())
                    .or(Some(json!("")));
            }
            if norm_json(name.as_ref())? != norm(title) {
                continue;
            }
            let links = get_obj(hit, "links")?;
            let listing = py_str(get_obj(links, "files")?);
            let entries = self
                .http
                .get(&Request::get(listing.clone(), 25))?
                .json()?
                .as_object()
                .ok_or_else(|| Error::new("AttributeError", "list has no get"))?
                .get("entries")
                .cloned()
                .unwrap_or(json!([]));
            let mut files: Vec<Obj> = Vec::new();
            for entry in entries
                .as_array()
                .ok_or_else(|| Error::new("TypeError", "entries"))?
            {
                let key = py_str(entry.get("key").unwrap_or(&json!("")));
                if !has_doc_ext(&key) {
                    continue;
                }
                let url = format!("{listing}/{key}/content");
                let mut f = self.as_document(&url).unwrap_or_else(|| {
                    let mut o = Obj::new();
                    o.insert("filename".into(), json!(key));
                    o.insert("url".into(), json!(url));
                    o.insert("confirmed".into(), json!(false));
                    o
                });
                f.insert("label".into(), json!(key));
                files.push(f);
            }
            // the f-string default is built even when self_html exists
            let id = py_str(get_obj(hit, "id")?);
            let page = match links.get("self_html") {
                Some(v) => py_str(v),
                None => format!("https://{host}/theses/{id}"),
            };
            return Ok(Some((page, files)));
        }
        Ok(None)
    }

    /// Find the thesis in the school's own repository and list its files. Only an exact title
    /// match is accepted, because a near miss would attach someone else's PDF to this record.
    pub fn repository_lookup(&self, school: &str, title: &str) -> Vec<Obj> {
        let hay = norm(school);
        let Some((_, host, flavour)) = REPOSITORY.iter().find(|(k, _, _)| hay.contains(&norm(k)))
        else {
            return Vec::new();
        };
        if title.is_empty() {
            return Vec::new();
        }
        let found = match *flavour {
            "dspace7" => self.dspace7(host, title),
            "dspace6" => self.dspace6(host, title),
            "dspace5" => self.dspace5(host, title),
            _ => self.invenio(host, title),
        };
        let Ok(Some((page, mut files))) = found else {
            return Vec::new();
        };
        for f in files.iter_mut() {
            f.insert("source".into(), json!(page));
        }
        files
    }

    /// Files theses.cz serves itself, from the file manager on the record page.
    fn hosted_files(page: &Page, code: &str) -> Vec<Obj> {
        let prefix = format!("/id/{code}/");
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for a in page.select("a[href]") {
            let href = a.value().attr("href").unwrap_or("");
            if !href.starts_with(&prefix) {
                continue;
            }
            let name = last_segment(href);
            if !has_doc_ext(name) || seen.contains(href) {
                continue;
            }
            seen.insert(href.to_string());
            let label = node_text(Some(a), &[]);
            let label = if label.is_empty() {
                name.to_string()
            } else {
                label
            };
            let stripped = Regex::new(&format!(r"\s*{}\s*$", regex::escape(name)))
                .map(|r| r.replace(&label, "").into_owned())
                .unwrap_or_else(|_| label.clone());
            let mut f = Obj::new();
            f.insert(
                "label".into(),
                json!(if stripped.is_empty() {
                    name.to_string()
                } else {
                    stripped
                }),
            );
            f.insert("filename".into(), json!(name));
            f.insert("url".into(), json!(format!("{BASE}{href}")));
            out.push(f);
        }
        out
    }

    /// Files behind a record in a STAG portal. The record URL answers with an empty shell and
    /// fills the file list in afterwards; the list comes from a portlet that answers a plain GET.
    pub fn stag_files(&self, archive: &str) -> Vec<Obj> {
        let Some(thesis) = STAG_ID.captures(archive) else {
            return Vec::new();
        };
        let parts = urlsplit(archive, "");
        let root = format!("{}://{}", parts.scheme, parts.netloc);
        let mut out: Vec<Obj> = Vec::new();
        let mut seen = HashSet::new();
        for (page, extra) in STAG_PAGES {
            let mut params: Vec<(&str, String)> = vec![
                ("pp_locale", "cs".into()),
                ("pp_reqType", "render".into()),
                ("pp_ajaxCall", "1".into()),
                ("pp_portlet", "souboryStudentuPagesPortlet".into()),
                ("pp_page", page.into()),
                // any namespace works, it only prefixes the ids in the markup
                ("pp_nameSpace", "G1".into()),
                ("sou_adipidno", thesis[1].to_string()),
            ];
            if let Some(app) = extra {
                params.push(("sou_aplikace", app.into()));
            }
            let req = Request::with_params(
                &format!("{root}/StagPortletsJSR168/PagesDispatcherServlet"),
                &params,
                45,
            );
            // one dead block must not lose the other three
            let Ok(r) = self.http.get(&req) else { continue };
            if r.status != 200 {
                continue;
            }
            let block = Page::parse(&r.text(), &r.url);
            for f in self.documents(&block, &root) {
                let url = f["url"].as_str().unwrap_or("").to_string();
                if seen.insert(url) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// The school repository link for a record. Usually it sits on the record page; VŠB and
    /// UHK records do not carry it there, so look the thesis up by title and read the link off
    /// its own result row.
    fn archive_url(&self, page: &Page, code: &str) -> Result<Option<String>> {
        let link = page
            .select_one(".plny_text_ext .ext_prez a[href]")
            .or_else(|| page.select_one(".plny_text_ext a[href]"));
        if let Some(l) = link {
            return Ok(Some(attr(l, "href")?));
        }
        let title = node_text(page.select_one(".th-title"), &[]);
        if title.is_empty() {
            return Ok(None);
        }
        let listing = self.get_page(&format!(
            "{BASE}/vyhledavani/?search={}&start=1",
            quote(&title)
        ))?;
        let prefix = format!("/id/{code}/");
        for it in listing.select(".vyh_polozka") {
            if let Some(a) = it.select(&sel("h4 a")).next()
                && attr(a, "href")?.starts_with(&prefix)
            {
                let ext: Vec<String> = it
                    .select(&sel("a[href^='http']"))
                    .filter_map(|x| x.value().attr("href"))
                    .filter(|h| !h.contains("theses.cz"))
                    .map(String::from)
                    .collect();
                if let Some(first) = ext.into_iter().next() {
                    return Ok(Some(first));
                }
            }
        }
        Ok(None)
    }

    pub fn fulltext(&self, id_or_url: &str) -> Result<Value> {
        let code = record_code(id_or_url);
        let page = self.get_page(&format!("{BASE}/id/{code}/"))?;
        if page.select_one("#metadata").is_none() {
            return Ok(Self::record_error(&page, &code));
        }
        let access: Vec<String> = page
            .select("#th-obsah li")
            .into_iter()
            .map(|li| node_text(Some(li), &[]))
            .collect();
        let mut archive = self.archive_url(&page, &code)?;
        let url = format!("{BASE}/id/{code}/");

        let finish = |files: Vec<Obj>,
                      archive: &Option<String>,
                      note: Option<String>,
                      pending: bool,
                      served_by: Option<&str>| {
            let mut res = Obj::new();
            res.insert("url".into(), json!(url));
            res.insert("access".into(), json!(access));
            res.insert("archive_url".into(), json!(archive));
            res.insert("files".into(), json!(files));
            if let Some(s) = served_by {
                res.insert("served_by".into(), json!(s));
            }
            if let Some(n) = note {
                res.insert("note".into(), json!(n));
            }
            if pending {
                res.insert("pending_defense".into(), json!(true));
            }
            Ok(Value::Object(res))
        };

        let hosted = Self::hosted_files(&page, &code);
        if !hosted.is_empty() {
            // the registry's own copy needs no trip to the school at all
            return finish(hosted, &archive, None, false, Some("theses.cz"));
        }

        let mut files: Vec<Obj> = Vec::new();
        let mut note: Option<String> = None;
        let mut school_page: Option<Page> = None;
        if let Some(arch) = archive.clone() {
            let stag = self.stag_files(&arch);
            if !stag.is_empty() {
                // a STAG portal never puts its files on the page itself
                return finish(stag, &archive, None, false, None);
            }
            if let Some(direct) = self.as_document(&arch) {
                // the link is the file itself (MENDELU and other IS /zp/ hosts)
                return finish(vec![direct], &archive, None, false, None);
            }
            match self.get_page(&arch) {
                Ok(p) => {
                    files = self.documents(&p, &p.url);
                    school_page = Some(p);
                }
                // SSL/DNS: some school systems are simply broken
                Err(e) => note = Some(format!("school repository unreachable: {}", e.class)),
            }
        }

        let mut searched: Option<&str> = None;
        if files.is_empty() {
            // last resort: the school's own digital library
            let school = node_text(page.select_one("#th-sloupec .oddil"), &[]);
            searched = REPOSITORY
                .iter()
                .find(|(k, _, _)| norm(&school).contains(&norm(k)))
                .map(|(_, h, _)| *h);
            let mut found =
                self.repository_lookup(&school, &node_text(page.select_one(".th-title"), &[]));
            if !found.is_empty() {
                if let Some(src) = found[0].shift_remove("source") {
                    archive = src.as_str().map(String::from);
                }
                for f in found.iter_mut() {
                    f.shift_remove("source");
                }
                files = found;
                note = None;
            }
        }

        if !files.is_empty()
            && files
                .iter()
                .all(|f| f.get("confirmed") == Some(&json!(false)))
        {
            // TUL answers its own bitstream URLs with 410; the listing is real, the files are not
            note = Some("repository lists these files but will not serve them anonymously".into());
        }

        let mut pending = false;
        if files.is_empty() && note.is_none() {
            match &school_page {
                None => {
                    // do not blame a missing link when the repository was searched and came up empty
                    note = Some(match searched {
                        Some(h) => format!(
                            "the record links nowhere, and {h} holds no thesis under this title"
                        ),
                        None => "no school repository link in the record".into(),
                    });
                }
                Some(p) => {
                    let host = urlsplit(archive.as_deref().unwrap_or(""), "").netloc;
                    let body = p.text();
                    let wall = is_gated(&body);
                    let hint = if self.is_authenticated(&host) {
                        String::new()
                    } else {
                        format!(" — set THESES_COOKIES for {host} to use your own login")
                    };
                    // "světu" means theses.cz publishes this to the world; a CAPTCHA in front
                    // of it is the school's own doing, not a restriction on the thesis
                    let public = access.iter().any(|a| a == "světu");
                    if NOT_YET.is_match(&body) {
                        note = Some(
                            "the thesis is not defended yet, so the school publishes no files"
                                .into(),
                        );
                        pending = true;
                    } else if wall {
                        note = Some(format!(
                            "{}repository gates anonymous access with a CAPTCHA/login{hint}",
                            if public {
                                "thesis is public, but the "
                            } else {
                                ""
                            }
                        ));
                    } else {
                        note = Some("no public files listed; access is likely restricted".into());
                    }
                }
            }
        }
        finish(files, &archive, note, pending, None)
    }
}
