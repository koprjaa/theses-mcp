// Project: theses-mcp
// File: src/dom.rs
// Description: HTML page wrapper and the text extraction the Python server did with BeautifulSoup.
// Author: Jan Alexandr Kopřiva
// License: MIT

use crate::util::squash;
use scraper::{ElementRef, Html, Node, Selector};

pub fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("static selector")
}

/// A parsed page and the URL it was finally served from (after redirects).
pub struct Page {
    pub html: Html,
    pub url: String,
}

impl Page {
    pub fn parse(text: &str, url: &str) -> Page {
        Page {
            html: Html::parse_document(text),
            url: url.to_string(),
        }
    }

    pub fn select_one(&self, css: &str) -> Option<ElementRef<'_>> {
        self.html.select(&sel(css)).next()
    }

    pub fn select(&self, css: &str) -> Vec<ElementRef<'_>> {
        self.html.select(&sel(css)).collect()
    }

    /// `soup.get_text()`: every text node, no separator. Script and style text is left out,
    /// as BeautifulSoup does.
    pub fn text(&self) -> String {
        let mut out = String::new();
        collect(
            self.html.root_element(),
            &[],
            &mut |t| out.push_str(t),
            false,
        );
        out
    }
}

fn skipped(el: &ElementRef, drop: &[Selector]) -> bool {
    matches!(el.value().name(), "script" | "style" | "template")
        || drop.iter().any(|d| d.matches(el))
}

fn collect(el: ElementRef, drop: &[Selector], sink: &mut dyn FnMut(&str), strip: bool) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let s: &str = t;
                let s = if strip { s.trim() } else { s };
                if !s.is_empty() {
                    sink(s);
                }
            }
            Node::Element(_) => {
                if let Some(e) = ElementRef::wrap(child)
                    && !skipped(&e, drop)
                {
                    collect(e, drop, sink, strip);
                }
            }
            _ => {}
        }
    }
}

/// `_txt(node, *drop)`: text of an element without expand/collapse links and section labels,
/// pieces joined by one space and whitespace collapsed.
pub fn node_text(node: Option<ElementRef>, drop: &[&str]) -> String {
    let Some(el) = node else {
        return String::new();
    };
    let mut selectors: Vec<Selector> = ["a.rozbal", "a.sbal", "h5", "h3"]
        .iter()
        .map(|s| sel(s))
        .collect();
    selectors.extend(drop.iter().map(|s| sel(s)));
    let mut pieces: Vec<String> = Vec::new();
    collect(el, &selectors, &mut |t| pieces.push(t.to_string()), true);
    squash(&pieces.join(" ")).trim().to_string()
}

/// `_txt` of a bare string: collapse whitespace, then strip spaces, nbsp and brackets.
pub fn string_text(s: &str) -> String {
    squash(s)
        .trim_matches(|c| matches!(c, ' ' | '\u{a0}' | '(' | ')'))
        .to_string()
}

/// `_txt(node.next_sibling)`: the sibling may be text, an element or a comment.
pub fn sibling_text(el: ElementRef) -> String {
    match el.next_sibling() {
        None => String::new(),
        Some(n) => match n.value() {
            Node::Text(t) => string_text(t),
            Node::Comment(c) => string_text(c),
            Node::Element(_) => node_text(ElementRef::wrap(n), &[]),
            _ => string_text(""),
        },
    }
}

/// `tag.get_text(strip=True)`: every string stripped and joined without a separator.
pub fn compact_text(el: ElementRef) -> String {
    let mut out = String::new();
    collect(el, &[], &mut |t| out.push_str(t), true);
    out
}
