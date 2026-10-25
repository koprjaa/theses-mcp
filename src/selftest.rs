// Project: theses-mcp
// File: src/selftest.rs
// Description: Live check against theses.cz and school repositories; the parsers break when site markup changes.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use crate::Theses;
use serde_json::Value;

type Check<T> = std::result::Result<T, String>;

fn ensure(cond: bool, what: &str, got: &Value) -> Check<()> {
    if cond {
        Ok(())
    } else {
        Err(format!("{what}: {got}"))
    }
}

fn files(v: &Value) -> Vec<&Value> {
    v["files"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn all_confirmed(v: &Value) -> bool {
    files(v).iter().all(|f| f["confirmed"] == Value::Bool(true))
}

fn archive_has(v: &Value, host: &str) -> bool {
    v["archive_url"].as_str().is_some_and(|u| u.contains(host))
}

/// Mirrors the Python `_selftest`. It makes dozens of requests, so it is for a person to run
/// now and then, not for CI.
pub fn run() -> Check<String> {
    let t = Theses::from_env();
    let e = |x: crate::error::Error| x.to_string();
    let r = t.search("midjourney", 12).map_err(e)?;
    ensure(r["total"].as_i64().unwrap_or(0) > 100, "total", &r["total"])?;
    let results = r["results"].as_array().cloned().unwrap_or_default();
    ensure(
        results.len() == 12,
        "pagination via start=11",
        &r["returned"],
    )?;
    ensure(
        results
            .iter()
            .all(|x| x["title"].as_str().is_some_and(|s| !s.is_empty())),
        "titles",
        &r,
    )?;
    ensure(
        results.iter().any(|x| {
            x["kind"] == "fulltext" && x["pdf_url"].as_str().is_some_and(|s| !s.is_empty())
        }),
        "a fulltext hit",
        &r,
    )?;
    let rec = results
        .iter()
        .find(|x| x["kind"] == "record")
        .ok_or("no record hit")?;
    let d = t.detail(rec["url"].as_str().unwrap_or("")).map_err(e)?;
    for k in ["author", "title", "abstract_cs"] {
        ensure(d[k].as_str().is_some_and(|s| !s.is_empty()), k, &d)?;
    }
    ensure(
        d["keywords"].as_object().is_some_and(|o| !o.is_empty()),
        "keywords",
        &d,
    )?;
    ensure(d["archive_url"].is_string(), "archive_url", &d)?;
    ensure(
        d["related"].as_array().is_some_and(|a| !a.is_empty()),
        "related",
        &d,
    )?;
    let d2 = t.detail("pl09jx").map_err(e)?; // unclosed <li> must not collapse into one entry
    ensure(
        d2["defense"]
            == serde_json::json!([
                "Obhajoba proběhla 12. 6. 2023",
                "Vedoucí: Jiří Korčák",
                "Oponent: Tomáš Sigmund"
            ]),
        "defense",
        &d2["defense"],
    )?;
    let public = t.fulltext("7lfo74").map_err(e)?; // public thesis -> real PDF in the school repository
    ensure(
        public["access"] == serde_json::json!(["světu"]),
        "access",
        &public["access"],
    )?;
    ensure(
        files(&public)
            .iter()
            .any(|f| f["url"].as_str().is_some_and(|u| u.ends_with(".pdf"))),
        "pdf",
        &public,
    )?;
    // restricted on theses.cz, but VŠE serves the PDF from an extensionless /zp/ URL
    let vse = t.fulltext("pl09jx").map_err(e)?;
    let main = files(&vse)
        .into_iter()
        .find(|f| f["filename"].as_str().is_some_and(|n| n.ends_with(".pdf")));
    ensure(
        main.is_some_and(|f| f["confirmed"] == true),
        "VŠE main file",
        &vse,
    )?;
    // VŠB: archive link missing from the record page, and a handle redirect to DSpace
    let vsb = t.fulltext("xggdtq").map_err(e)?;
    ensure(
        vsb["archive_url"].is_string(),
        "archive link not recovered from the search listing",
        &vsb,
    )?;
    ensure(
        files(&vsb).iter().any(|f| f["confirmed"] == true),
        "VŠB files",
        &vsb,
    )?;
    let men = t.fulltext("x52k58").map_err(e)?; // MENDELU: the archive link IS the PDF
    ensure(
        files(&men).len() == 1 && all_confirmed(&men),
        "MENDELU",
        &men,
    )?;
    let czu = t.fulltext("wjep6w").map_err(e)?; // ČZU: downloads labelled, no filename in URL or text
    ensure(files(&czu).len() == 3 && all_confirmed(&czu), "ČZU", &czu)?;
    let cvut = t.fulltext("dvopnp").map_err(e)?; // ČVUT: record links nowhere, files live in its own DSpace
    ensure(
        archive_has(&cvut, "dspace.cvut.cz") && files(&cvut).len() == 4 && all_confirmed(&cvut),
        "ČVUT",
        &cvut,
    )?;
    let jcu = t.fulltext("tfmf5y").map_err(e)?; // JČU: DSpace 6 REST, school spelled in capitals
    ensure(
        archive_has(&jcu, "dspace.jcu.cz") && !files(&jcu).is_empty() && all_confirmed(&jcu),
        "JČU",
        &jcu,
    )?;
    let zcu = t.fulltext("b1tsqt").map_err(e)?; // ZČU: interface and REST backend on different hosts
    ensure(
        archive_has(&zcu, "dspace.zcu.cz") && !files(&zcu).is_empty() && all_confirmed(&zcu),
        "ZČU",
        &zcu,
    )?;
    // UTB is checked at the lookup, not through a record: its DSpace 5 has no searchable API
    let utb = t
        .repository_lookup(
            "Univerzita Tomáše Bati ve Zlíně",
            "Korupce a její vliv na společnost",
        )
        .map_err(e)?;
    ensure(utb.len() == 3, "UTB", &serde_json::json!(utb.len()))?;
    let vscht = t
        .repository_lookup(
            "Vysoká škola chemicko-technologická v Praze",
            "Wikipedia v chemii, chemie na Wikipedii",
        )
        .map_err(e)?;
    ensure(
        !vscht.is_empty() && vscht.iter().all(|f| f["confirmed"] == true),
        "VŠCHT",
        &serde_json::json!(vscht.len()),
    )?;
    Ok(format!(
        "OK {} hits | {} | {} | {} + {} + {} + {} files",
        r["total"],
        d["author"].as_str().unwrap_or(""),
        d["type"].as_str().unwrap_or(""),
        files(&public).len(),
        files(&vse).len(),
        files(&vsb).len(),
        files(&men).len(),
    ))
}
