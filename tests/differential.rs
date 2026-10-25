// Project: theses-mcp
// File: tests/differential.rs
// Description: Replay what the Python server did on recorded responses and compare, request by request.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
//! Every file in `tests/fixtures/scenarios` holds a sequence of tool calls, the HTTP responses
//! they saw, and what the Python server answered, asked, and wrote to disk when given exactly
//! those responses. `live_main.json` was recorded against theses.cz and two school systems, then
//! every name, title, id and token in it was replaced by a placeholder; the rest are hand-built
//! to reach the branches that live session did not.
//!
//! A scenario may carry `intentional`: Rust is deliberately different, and the reason is
//! there. Then the expectation is `fixed`, which is Python with the same one-line change.

mod common;

use common::{Fixture, args_of};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use theses_mcp::Theses;
use theses_mcp::dispatch::call_blocking;

fn scenarios() -> Vec<(String, Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scenarios");
    let mut out: Vec<(String, Value)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                serde_json::from_str(&text).unwrap(),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Replaces the temporary home with `<HOME>`, also in its JSON-escaped form,
/// and writes the rest of such a path with `/` so Windows paths compare equal.
fn mask_home(text: &str, home: &str) -> String {
    let escaped = serde_json::to_string(home).unwrap();
    let escaped = &escaped[1..escaped.len() - 1];
    let text = text.replace(escaped, "<HOME>").replace(home, "<HOME>");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("<HOME>") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail.find(['"', '\n', ' ']).unwrap_or(tail.len());
        out.push_str(&tail[..end].replace("\\\\", "/").replace('\\', "/"));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

struct Observed {
    results: Vec<(bool, String)>,
    requests: Vec<String>,
    saved_after: Value,
    authenticated: Vec<String>,
}

fn run(sc: &Value) -> Observed {
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".theses-mcp");
    if !sc["saved"].is_null() {
        fs::create_dir_all(&store).unwrap();
        fs::write(store.join("cookies.json"), sc["saved"].to_string()).unwrap();
    }
    let fixture = Fixture::new(sc["responses"].as_array().unwrap());
    let env = sc["env"]
        .get("THESES_COOKIES")
        .and_then(Value::as_str)
        .map(String::from);
    let app = Theses::new(fixture.clone(), store.clone(), env.as_deref());
    let home_text = home.path().display().to_string();
    let results = sc["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let reply = call_blocking(&app, c["tool"].as_str().unwrap(), &args_of(&c["args"]));
            (reply.is_error, mask_home(&reply.text, &home_text))
        })
        .collect();
    let saved_after = fs::read_to_string(store.join("cookies.json"))
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or(Value::Null);
    let requests = fixture.log.lock().unwrap().clone();
    Observed {
        results,
        requests,
        saved_after,
        authenticated: app.authenticated_hosts(),
    }
}

#[test]
fn every_scenario_matches_python() {
    let all = scenarios();
    assert!(all.len() >= 30, "scenarios went missing: {}", all.len());
    let mut calls = 0;
    for (name, sc) in &all {
        let expected = if sc.get("intentional").is_some() {
            assert!(
                !sc["intentional"].as_str().unwrap().is_empty(),
                "{name}: say why"
            );
            assert_ne!(
                sc["fixed"], sc["expected"],
                "{name}: marked intentional but identical"
            );
            &sc["fixed"]
        } else {
            &sc["expected"]
        };
        let got = run(sc);
        assert_eq!(
            got.requests,
            expected["requests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            "{name}: requests differ"
        );
        let want = expected["results"].as_array().unwrap();
        assert_eq!(got.results.len(), want.len(), "{name}");
        for (i, ((is_error, text), w)) in got.results.iter().zip(want).enumerate() {
            assert_eq!(
                *is_error,
                w["error"].as_bool().unwrap(),
                "{name}[{i}] error flag: {text}"
            );
            if !is_error {
                // compare the text itself, so key order and number formatting count too
                assert_eq!(text, w["text"].as_str().unwrap(), "{name}[{i}]");
            }
            calls += 1;
        }
        assert_eq!(
            got.saved_after, expected["saved_after"],
            "{name}: cookies.json"
        );
        assert_eq!(
            json!(got.authenticated),
            expected["authenticated"],
            "{name}: hosts with a session"
        );
    }
    assert!(calls >= 40, "{calls}");
}

#[test]
fn only_the_listed_scenarios_differ_from_python_on_purpose() {
    let intentional: Vec<String> = scenarios()
        .into_iter()
        .filter(|(_, s)| s.get("intentional").is_some())
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        intentional,
        [
            "audit_detail_odd",
            "challenge_stub_page_shim_off",
            "fulltext_dspace7_badjson"
        ]
    );
}

#[test]
fn the_recorded_challenge_page_is_what_python_returned_unretried() {
    let (_, sc) = scenarios()
        .into_iter()
        .find(|(n, _)| n == "challenge_stub_page_shim_off")
        .unwrap();
    // Python took the placeholder for the answer: no results at all
    assert!(
        sc["expected"]["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"returned\": 0")
    );
    // Rust waits and asks again, and gets the listing
    assert!(
        sc["fixed"]["results"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"returned\": 5")
    );
}
