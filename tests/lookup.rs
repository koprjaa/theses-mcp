// Project: theses-mcp
// File: tests/lookup.rs
// Description: Regressions for record codes, search parsing and school repository lookups.
// Author: Jan Alexandr Kopřiva
// License: MIT

mod common;

use common::{Mock, reply};
use theses_mcp::dom::Page;
use theses_mcp::error::Error;
use theses_mcp::theses::record_code;

#[test]
fn a_pasted_url_with_query_or_fragment_still_names_the_record() {
    assert_eq!(
        record_code("https://theses.cz/id/pl09jx/?lang=en"),
        "pl09jx"
    );
    assert_eq!(record_code("/id/pl09jx/#abstract"), "pl09jx");
    assert_eq!(record_code("pl09jx"), "pl09jx");
}

#[test]
fn a_results_line_without_a_count_and_a_hit_without_a_link_do_not_fail_the_search() {
    let page = r#"<p>Výsledky 1 – 10 z  hledání</p>
        <div class="vyh_polozka" data-agenda="T"><h4><a>No link</a></h4></div>
        <div class="vyh_polozka" data-agenda="T"><h4><a href="/id/abc123/x">Title</a></h4> Author</div>"#;
    let t = common::app_with(Mock::new(move |_| Ok(reply(page.as_bytes(), "text/html"))));
    let r = t.search("x", 10).unwrap();
    assert_eq!(r["returned"], 1);
    assert_eq!(r["results"][0]["url"], "https://theses.cz/id/abc123/");
}

#[test]
fn a_failing_repository_is_an_error_not_an_empty_answer() {
    let t = common::app_with(Mock::new(|_| Err(Error::new("ReadTimeout", "timed out"))));
    let e = t
        .repository_lookup("České vysoké učení technické v Praze", "Some title")
        .unwrap_err();
    assert_eq!(e.class, "ReadTimeout");
}

#[test]
fn a_title_without_latin_letters_is_never_matched() {
    let mock = Mock::new(|_| Err(Error::new("ConnectionError", "must not be called")));
    let t = common::app_with(mock.clone());
    let found = t
        .repository_lookup("Univerzita Karlova", "Диссертация")
        .unwrap();
    assert!(found.is_empty() && mock.calls().is_empty());
}

#[test]
fn a_page_of_many_links_is_not_probed_link_by_link() {
    let html: String = (0..100)
        .map(|i| format!(r#"<a href="/f/{i}">Review {i}</a>"#))
        .collect();
    let mock = Mock::new(|_| Ok(reply(b"<html></html>", "text/html")));
    let t = common::app_with(mock.clone());
    let page = Page::parse(&html, "https://school.cz/");
    t.documents(&page, "https://school.cz/");
    assert!(mock.calls().len() <= 25, "{}", mock.calls().len());
}
