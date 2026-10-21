// Project: theses-mcp
// File: tests/parsing.rs
// Description: Parsing, document-link matching, STAG portlets and session detection, without a network.
// Author: Jan Alexandr Kopřiva
// License: MIT

//! Port of the Python `test_parsing.py`, plus the table of Python answers in
//! `fixtures/unit.json` for the helpers whose edge cases are easy to get subtly wrong.
//!
//! The markup below reproduces structures seen on the real sites, with names replaced.
//! `documents` asks the server about anything it cannot name from the page, so those tests
//! give it a transport that answers every probe the same way.

mod common;

use common::{Mock, app_with, reply};
use scraper::Html;
use serde_json::{Value, json};
use theses_mcp::dom::{Page, node_text, sel, sibling_text};
use theses_mcp::error::Error;
use theses_mcp::session::{is_gated, is_signed_in};
use theses_mcp::theses::{BASE, stub_delay};
use theses_mcp::util::{DOC_LABEL, DOC_RE, NOT_YET, norm, quote, urljoin, urlsplit};

fn unit() -> Value {
    serde_json::from_str(include_str!("fixtures/unit.json")).unwrap()
}

fn page(markup: &str) -> Page {
    Page::parse(markup, "")
}

fn txt(markup: &str, css: &str, drop: &[&str]) -> String {
    let doc = Html::parse_document(markup);
    node_text(doc.select(&sel(css)).next(), drop)
}

// --- the Python answers, replayed --------------------------------------------

#[test]
fn norm_matches_python() {
    for c in unit()["norm"].as_array().unwrap() {
        assert_eq!(norm(c["in"].as_str().unwrap_or("")), c["out"], "{c}");
    }
}

#[test]
fn urljoin_matches_python() {
    for c in unit()["urljoin"].as_array().unwrap() {
        let got = urljoin(c["base"].as_str().unwrap(), c["href"].as_str().unwrap());
        assert_eq!(got, c["out"].as_str().unwrap(), "{c}");
    }
}

#[test]
fn urlparse_matches_python() {
    for c in unit()["urlparse"].as_array().unwrap() {
        let p = urlsplit(c["in"].as_str().unwrap(), "");
        assert_eq!(
            (p.scheme.as_str(), p.netloc.as_str()),
            (c["scheme"].as_str().unwrap(), c["netloc"].as_str().unwrap()),
            "{c}"
        );
    }
}

#[test]
fn quote_matches_python() {
    for c in unit()["quote"].as_array().unwrap() {
        assert_eq!(
            quote(c["in"].as_str().unwrap()),
            c["out"].as_str().unwrap(),
            "{c}"
        );
    }
}

#[test]
fn doc_patterns_match_python() {
    for c in unit()["doc_re"].as_array().unwrap() {
        let got = DOC_RE
            .captures(c["in"].as_str().unwrap())
            .map(|m| m[1].to_string());
        assert_eq!(json!(got), c["out"], "{c}");
    }
    for c in unit()["doc_label"].as_array().unwrap() {
        assert_eq!(
            json!(DOC_LABEL.is_match(c["in"].as_str().unwrap())),
            c["out"],
            "{c}"
        );
    }
    for c in unit()["not_yet"].as_array().unwrap() {
        assert_eq!(
            json!(NOT_YET.is_match(c["in"].as_str().unwrap())),
            c["out"],
            "{c}"
        );
    }
}

#[test]
fn refresh_delay_matches_python() {
    for c in unit()["refresh"].as_array().unwrap() {
        let text = c["in"].as_str().unwrap();
        let head: String = text.chars().take(400).collect();
        // Python read the number; the server clamps it to 1..=3 seconds
        let got = stub_delay(&head).map(|d| d.to_string());
        let want = c["out"].as_i64().map(|n| n.clamp(1, 3).to_string());
        assert_eq!(got, want, "{c}");
    }
}

#[test]
fn session_detection_matches_python() {
    for c in unit()["session"].as_array().unwrap() {
        let t = c["in"].as_str().unwrap();
        assert_eq!(json!(is_signed_in(t)), c["signed_in"], "signed_in {c}");
        assert_eq!(json!(is_gated(t)), c["gated"], "gated {c}");
    }
}

#[test]
fn text_extraction_matches_python() {
    for c in unit()["txt"].as_array().unwrap() {
        let drop: Vec<&str> = c["drop"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        let got = txt(
            c["html"].as_str().unwrap(),
            c["sel"].as_str().unwrap(),
            &drop,
        );
        assert_eq!(got, c["out"].as_str().unwrap(), "{c}");
    }
}

// --- _txt ---------------------------------------------------------------------

#[test]
fn txt_of_a_missing_node_is_empty() {
    assert_eq!(node_text(None, &[]), "");
}

#[test]
fn txt_collapses_runs_of_whitespace() {
    assert_eq!(
        txt("<p>Vysoká   škola\n\n ekonomická</p>", "p", &[]),
        "Vysoká škola ekonomická"
    );
}

#[test]
fn txt_drops_the_expand_and_collapse_links() {
    let m = r#"<div>Anotace <a class="rozbal">více</a><a class="sbal">méně</a></div>"#;
    assert_eq!(txt(m, "div", &[]), "Anotace");
}

#[test]
fn txt_drops_section_headings() {
    assert_eq!(
        txt(
            "<div><h3>Klíčová slova</h3><h5>cs</h5>právo, smlouva</div>",
            "div",
            &[]
        ),
        "právo, smlouva"
    );
}

#[test]
fn txt_drops_extra_selectors_passed_in() {
    assert_eq!(
        txt(
            r#"<div>keep<span class="skip">drop</span></div>"#,
            "div",
            &["span.skip"]
        ),
        "keep"
    );
}

#[test]
fn txt_of_a_bare_string_strips_brackets_and_nbsp() {
    // search() reads the author from the text node that follows the h4
    let doc = Html::parse_document("<div><h4>Title</h4>\u{a0}(Bc. Jan Novák)</div>");
    let h4 = doc.select(&sel("h4")).next().unwrap();
    assert_eq!(sibling_text(h4), "Bc. Jan Novák");
}

// --- document link patterns ---------------------------------------------------

#[test]
fn doc_re_finds_a_filename_in_a_label() {
    for label in [
        "Hlavní práce 73157_vala07.pdf, 1.5 MB Stáhnout",
        "Oponentura 69167_Havelka.pdf, 500 kB Stáhnout",
        "priloha.docx",
        "data.zip",
        "text.rtf",
    ] {
        assert!(DOC_RE.is_match(label), "{label}");
    }
}

#[test]
fn doc_re_ignores_a_label_with_no_filename() {
    for label in ["Stáhnout", "Zpět na výpis", "1.5 MB"] {
        assert!(!DOC_RE.is_match(label), "{label}");
    }
}

#[test]
fn doc_label_recognizes_a_download_without_a_filename() {
    for label in [
        "Plný text práce",
        "Final thesis",
        "Posudek vedoucího",
        "Supervisor's review",
        "Příloha",
    ] {
        assert!(DOC_LABEL.is_match(label), "{label}");
    }
}

// --- documents ----------------------------------------------------------------

const MUNI: &str = r#"
<div>
  <a href="Novak_Bakalarska_Prace_Final.pdf">Plný text práce</a>
  <a href="posudek_vedouciho_Svoboda.pdf">Posudek vedoucího</a>
  <a href="/th/avwwh/">Zpět</a>
</div>"#;

const VSE: &str = r#"
<div>
  <a href="https://insis.vse.cz/zp/73157">Hlavní práce 73157_novak.pdf, 1.5 MB Stáhnout</a>
  <a href="https://insis.vse.cz/zp/73157/posudek/oponent/69167">Oponentura 69167_Svoboda.pdf, 500 kB Stáhnout</a>
</div>"#;

fn no_probe() -> theses_mcp::Theses {
    app_with(Mock::new(|r| panic!("a probe was sent for {}", r.url)))
}

/// Every probed URL answers as a PDF.
fn probe_says_pdf() -> theses_mcp::Theses {
    app_with(Mock::new(|_| Ok(reply(b"%PDF-1.7", "application/pdf"))))
}

fn probe_says_no() -> theses_mcp::Theses {
    app_with(Mock::new(|_| Ok(reply(b"<html></html>", "text/html"))))
}

fn names(files: &[serde_json::Map<String, Value>], key: &str) -> Vec<String> {
    files
        .iter()
        .map(|f| f[key].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn an_extension_in_the_href_is_enough() {
    let files = no_probe().documents(&page(MUNI), "https://is.muni.cz/th/avwwh/");
    assert_eq!(
        names(&files, "filename"),
        [
            "Novak_Bakalarska_Prace_Final.pdf",
            "posudek_vedouciho_Svoboda.pdf"
        ]
    );
    assert_eq!(
        files[0]["url"],
        "https://is.muni.cz/th/avwwh/Novak_Bakalarska_Prace_Final.pdf"
    );
}

#[test]
fn a_link_with_neither_a_filename_nor_a_download_label_is_ignored() {
    let files = no_probe().documents(&page(MUNI), "https://is.muni.cz/th/avwwh/");
    assert!(
        files
            .iter()
            .all(|f| !f["label"].as_str().unwrap().contains("Zpět"))
    );
}

#[test]
fn a_filename_in_the_label_is_used_when_the_url_has_no_extension() {
    let files = probe_says_pdf().documents(&page(VSE), "https://vskp.vse.cz/100002");
    assert_eq!(
        names(&files, "filename"),
        ["73157_novak.pdf", "69167_Svoboda.pdf"]
    );
}

#[test]
fn the_download_word_is_stripped_from_the_label() {
    let files = probe_says_pdf().documents(&page(VSE), "https://vskp.vse.cz/100002");
    assert_eq!(files[0]["label"], "Hlavní práce 73157_novak.pdf, 1.5 MB");
}

#[test]
fn an_extensionless_url_is_confirmed_with_the_server() {
    let files = probe_says_pdf().documents(&page(VSE), "https://vskp.vse.cz/100002");
    assert!(files.iter().all(|f| f["confirmed"] == true));
}

#[test]
fn an_extensionless_url_that_serves_no_file_is_marked_unconfirmed() {
    let files = probe_says_no().documents(&page(VSE), "https://vskp.vse.cz/100002");
    assert!(files.iter().all(|f| f["confirmed"] == false));
}

const FINAL_THESIS: &str =
    r#"<div><a href="https://example.edu/download/1">Final thesis</a></div>"#;

#[test]
fn a_label_only_download_is_probed_and_kept() {
    // ČZU and Škoda Auto name neither the URL nor the text
    let files = probe_says_pdf().documents(&page(FINAL_THESIS), "https://example.edu/");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["label"], "Final thesis");
    assert_eq!(files[0]["confirmed"], true);
}

#[test]
fn a_label_only_link_that_serves_html_is_dropped() {
    assert!(
        probe_says_no()
            .documents(&page(FINAL_THESIS), "https://example.edu/")
            .is_empty()
    );
}

#[test]
fn a_mailto_link_is_skipped() {
    let m = r#"<div><a href="mailto:a@b.cz">thesis.pdf</a></div>"#;
    assert!(
        no_probe()
            .documents(&page(m), "https://example.edu/")
            .is_empty()
    );
}

#[test]
fn the_same_url_is_listed_once() {
    let m = r#"<div><a href="a.pdf">Plný text</a><a href="a.pdf">Plný text práce</a></div>"#;
    assert_eq!(
        no_probe().documents(&page(m), "https://example.edu/").len(),
        1
    );
}

#[test]
fn a_relative_href_resolves_against_the_page() {
    let m = r#"<div><a href="files/a.pdf">Plný text</a></div>"#;
    let files = no_probe().documents(&page(m), "https://is.muni.cz/th/avwwh/");
    assert_eq!(files[0]["url"], "https://is.muni.cz/th/avwwh/files/a.pdf");
}

#[test]
fn a_page_with_no_downloads_returns_nothing() {
    assert!(
        no_probe()
            .documents(
                &page("<div><a href='/help'>Nápověda</a></div>"),
                "https://x.cz/"
            )
            .is_empty()
    );
}

// --- norm ---------------------------------------------------------------------

#[test]
fn norm_strips_punctuation_and_spaces() {
    assert_eq!(
        norm("Vliv AI na autorské právo: analýza"),
        "vlivainaautorsképrávoanalýza"
    );
}

#[test]
fn norm_lowercases_czech_letters() {
    assert_eq!(norm("ŠKODA Čeština"), norm("škoda čeština"));
}

#[test]
fn norm_makes_two_titles_that_differ_only_in_punctuation_equal() {
    assert_eq!(norm("Analýza dat, 2. vydání"), norm("Analýza dat 2 vydání"));
}

// --- STAG portlet -------------------------------------------------------------

const STAG_HREF: &str = "/StagPortletsJSR168/PagesDispatcherServlet?pp_locale=cs&amp;\
pp_portlet=souboryStudentuPagesPortlet&amp;\
pp_page=souboryStudentuDownloadPage&amp;pp_nameSpace=G15790&amp;soubidno=80686";

fn stag_block() -> String {
    format!(
        r#"<div id='G1validation_errors' class="xgt_validationErrors"></div>
<table><tr><td></td><td>
  <a class="xg_stag_a_in" href="{STAG_HREF}"> Atestacni prace Novak.pdf </a> (749 KB)
</td></tr></table>"#
    )
}

// TUL keeps the file in DSpace and names neither the URL nor the link
const STAG_ELSEWHERE: &str = r#"<table><tr><td>
  <a class="xg_stag_a_out" href="https://dspace.tul.cz/bitstream/308a2f21/download">
  Zde k dispozici</a></td></tr></table>"#;

const STAG_URL: &str = "https://stagweb.example.cz/StagPortletsJSR168/CleanUrl?urlid=prohlizeni-prace-detail&praceIdno=10787";

/// Answer every portlet call with `block`, sniff any download URL as a PDF.
fn stag_serves(
    block: String,
) -> std::sync::Arc<
    Mock<
        impl Fn(&theses_mcp::http::Request) -> Result<theses_mcp::http::Response, Error> + Send + Sync,
    >,
> {
    Mock::new(move |r| {
        if r.probe {
            Ok(reply(b"%PDF-1.7", "application/pdf"))
        } else {
            Ok(reply(block.as_bytes(), "text/html"))
        }
    })
}

fn portlet_calls<F>(m: &Mock<F>) -> Vec<theses_mcp::http::Request>
where
    F: Fn(&theses_mcp::http::Request) -> Result<theses_mcp::http::Response, Error> + Send + Sync,
{
    m.calls().into_iter().filter(|r| !r.probe).collect()
}

#[test]
fn stag_reads_the_file_out_of_the_portlet() {
    let files = app_with(stag_serves(stag_block())).stag_files(STAG_URL);
    assert!(
        files[0]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://stagweb.example.cz/StagPortletsJSR168/")
    );
    assert!(files[0]["url"].as_str().unwrap().contains("soubidno=80686"));
}

#[test]
fn stag_asks_the_portlet_for_the_thesis_id_in_the_record_url() {
    let m = stag_serves(stag_block());
    app_with(m.clone()).stag_files(STAG_URL);
    let calls = portlet_calls(&m);
    assert_eq!(calls.len(), 4);
    for c in calls {
        assert!(c.url.contains("sou_adipidno=10787"), "{}", c.url);
        assert!(
            c.url.contains("pp_portlet=souboryStudentuPagesPortlet"),
            "{}",
            c.url
        );
    }
}

#[test]
fn stag_lists_a_file_once_even_though_four_blocks_are_asked() {
    // the four blocks are separate pages, and more than one can name the same file
    let m = stag_serves(stag_block());
    assert_eq!(app_with(m.clone()).stag_files(STAG_URL).len(), 1);
    assert_eq!(portlet_calls(&m).len(), 4);
}

#[test]
fn a_label_that_ends_at_the_extension_is_the_whole_filename() {
    // STAG names files with spaces, and DOC_RE would keep only the last word
    let files = app_with(stag_serves(stag_block())).stag_files(STAG_URL);
    assert_eq!(files[0]["filename"], "Atestacni prace Novak.pdf");
}

#[test]
fn stag_follows_a_file_the_portal_keeps_somewhere_else() {
    let files = app_with(stag_serves(STAG_ELSEWHERE.to_string())).stag_files(STAG_URL);
    assert_eq!(
        files[0]["url"],
        "https://dspace.tul.cz/bitstream/308a2f21/download"
    );
    assert_eq!(files[0]["label"], "Zde k dispozici");
}

#[test]
fn a_url_with_no_thesis_id_is_not_a_stag_record() {
    let m = stag_serves(stag_block());
    assert!(
        app_with(m.clone())
            .stag_files("https://is.muni.cz/th/avwwh/")
            .is_empty()
    );
    assert!(m.calls().is_empty());
}

// --- what search promises -----------------------------------------------------

#[test]
fn a_record_url_is_the_record_page_not_whatever_the_title_linked_to() {
    // theses.cz answered /id/gt2ymz/94924_bezm05.pdf with a stub and never with the file,
    // while search reported that URL as the record. The record page is always /id/<code>/.
    let listing = r#"<p>Výsledky 1 – 1 z 1</p><div class="vyh_polozka" data-agenda="T">
        <h4><a href="/id/gt2ymz/94924_bezm05.pdf">A thesis</a></h4></div>"#;
    let m = Mock::new(move |_| Ok(reply(listing.as_bytes(), "text/html; charset=utf-8")));
    let out = app_with(m).search("a thesis", 10).unwrap();
    assert_eq!(out["results"][0]["url"], format!("{BASE}/id/gt2ymz/"));
    assert_eq!(out["results"][0]["url"], "https://theses.cz/id/gt2ymz/");
}

#[test]
fn a_refresh_stub_is_read_for_the_delay_it_asks_for() {
    // _as_document used to sleep a fixed 1.5 s and give up after three tries
    let stub = "<!DOCTYPE HTML>\n<html>\n<head>\n<meta http-equiv=\"refresh\" content=\"2\">";
    assert_eq!(stub_delay(stub), Some(2));
}

// --- not defended yet is not the same as restricted ---------------------------

#[test]
fn a_thesis_awaiting_its_defense_is_recognized() {
    for body in [
        "Soubory ke stažení Soubory budou k dispozici až po obhajobě práce.",
        "Files will be available after the defence of the thesis.",
    ] {
        assert!(NOT_YET.is_match(body), "{body}");
    }
}

#[test]
fn an_ordinary_restriction_is_not_read_as_a_pending_defense() {
    for body in [
        "Plný text práce je přístupný pouze autentizovaným uživatelům.",
        "Soubory ke stažení: prace.pdf",
        "Pro ověření opište prosíme tento kód",
    ] {
        assert!(!NOT_YET.is_match(body), "{body}");
    }
}

// --- session detection --------------------------------------------------------

fn chrome() -> String {
    "Theses.cz Odevzdej.cz PravyDiplom.cz Repozitar.cz Vyhledávání Nápověda \
     Zapojené školy Správci zapojených škol Soukromí Přístupnost Provozuje \
     Fakulta informatiky MU Potřebujete poradit Často kladené dotazy \
     Vysokoškolské kvalifikační práce Domů Odhlášení ze systému "
        .repeat(2)
}

#[test]
fn a_logout_link_does_not_mean_you_are_logged_in() {
    // the IS template ships "Odhlášení ze systému" to anonymous visitors too
    assert!(!is_signed_in(&format!(
        "{}Přihlásit se Přihlásit se (EduID)",
        chrome()
    )));
}

#[test]
fn a_page_with_no_invitation_to_log_in_counts_as_signed_in() {
    assert!(is_signed_in(&format!("{}Moje studium Jan Novák", chrome())));
}

#[test]
fn a_page_that_says_nothing_is_not_a_session() {
    // theses.cz/auth/ answers with an empty body, which once passed as a valid login
    for body in ["", "   ", "<html><body></body></html>", "404 Not Found"] {
        assert!(!is_signed_in(body), "{body:?}");
    }
}

#[test]
fn a_captcha_page_is_gated_and_not_signed_in() {
    // it carries no login prompt either, so the CAPTCHA has to be checked first
    let captcha = "Závěrečné práce Pro ověření opište prosíme tento kód Odeslat IS VŠFS";
    assert!(is_gated(captcha));
    assert!(!is_signed_in(captcha));
}

#[test]
fn the_javascript_challenge_theses_cz_serves_now_is_a_placeholder_too() {
    // recorded from the live site: no meta refresh, a proof-of-work script that reloads the page
    let scenario: Value = serde_json::from_str(include_str!(
        "fixtures/scenarios/challenge_stub_page_shim_off.json"
    ))
    .unwrap();
    let challenge = scenario["responses"][0]["body"].as_str().unwrap();
    let head: String = challenge.chars().take(400).collect();
    assert_eq!(stub_delay(&head), Some(3));
    assert_eq!(
        stub_delay("<html><body>Výsledky 1 – 10 z 610</body></html>"),
        None
    );
}
