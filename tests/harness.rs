// Project: theses-mcp
// File: tests/harness.rs
// Description: Tests for the helpers that decide which school a search result belongs to.
// Author: Jan Alexandr Kopřiva
// License: MIT

//! Port of the Python `test_harness.py`.
//!
//! The Python coverage harness produced the school-by-school numbers; a bug there does not
//! fail loudly, it reports a school as unreachable instead. That is what happened: one header
//! shape parsed to the faculty alone, and 16 institutions carried a verdict they had not
//! earned. The harness itself needed live traffic against every school and is not part of
//! this crate; the three helpers that carry the matching logic are kept here with their tests.
//!
//! The headers below are copied from live theses.cz search results.

use regex::Regex;
use std::sync::LazyLock;

static YEAR_IN_HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",\s*(\d{4})\b").unwrap());

/// The school and faculty, which always sit just before the year of defense.
///
/// Two header shapes exist, and only the year separates them reliably:
///
///     Diplomová práce, Univerzita Jana Amose Komenského Praha, 2013
///     Diplomová práce: <title> (<author>) Masarykova univerzita, Filozofická fakulta, 2024
///
/// The second puts the title and the author first. A pattern anchored on "práce," swallows
/// all of it and returns the faculty alone.
fn school_of(header: &str) -> String {
    let Some(year) = YEAR_IN_HEADER.find_iter(header).last() else {
        return String::new();
    };
    let before = &header[..year.start()];
    // the author parenthesis ends the title, and the school starts right after it
    let start = match before.rfind(')') {
        Some(i) => i + 1,
        None => match before.find("práce,") {
            Some(i) => i + "práce,".len(),
            None => 0,
        },
    };
    before[start..].to_string()
}

/// Lowercase, accents removed, anything but letters, digits and spaces dropped.
fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let plain = match c {
            'á' | 'ä' => 'a',
            'č' | 'ç' => 'c',
            'ď' => 'd',
            'é' | 'ě' | 'ë' => 'e',
            'í' => 'i',
            'ň' => 'n',
            'ó' | 'ö' => 'o',
            'ř' => 'r',
            'š' => 's',
            'ť' => 't',
            'ú' | 'ů' | 'ü' => 'u',
            'ý' => 'y',
            'ž' => 'z',
            other => other,
        };
        if plain.is_ascii_lowercase() || plain.is_ascii_digit() || plain == ' ' {
            out.push(plain);
        }
    }
    out
}

/// Words worth matching on: drop the ones half the country shares.
fn distinctive(name: &str) -> Vec<String> {
    const NOISE: [&str; 16] = [
        "v",
        "a",
        "praze",
        "brne",
        "sro",
        "ops",
        "as",
        "zu",
        "vysoka",
        "skola",
        "univerzita",
        "univerzity",
        "vyssi",
        "odborna",
        "ceske",
        "ceskych",
    ];
    fold(name)
        .split_whitespace()
        .filter(|w| !NOISE.contains(w) && w.chars().count() > 3)
        .map(String::from)
        .collect()
}

// "práce," then the school
const FLAT: &str = "Diplomová práce, Univerzita Jana Amose Komenského Praha, 2013 \
Pedagogika / Andragogika Příbuzné soubory: th76_1201110365.pdf";

// "práce:" then the title and the author, and only then the school
const TITLED: &str = "Diplomová práce: Univerzita jako sbírkotvorná instituce. Umělecká sbírka \
Masarykovy univerzity (Simona Tichá) Masarykova univerzita, \
Filozofická fakulta, 2024";

#[test]
fn the_school_follows_the_word_prace() {
    assert_eq!(
        fold(&school_of(FLAT)).trim(),
        "univerzita jana amose komenskeho praha"
    );
}

#[test]
fn the_school_follows_the_author_when_a_title_comes_first() {
    // an anchor on "práce," swallows the title and returns the faculty alone
    assert_eq!(
        fold(&school_of(TITLED)).trim(),
        "masarykova univerzita filozoficka fakulta"
    );
}

#[test]
fn the_title_is_left_out_even_when_it_names_another_school() {
    // the title here says "Masarykovy univerzity" and the thesis is not from Brno
    let header = "Diplomová práce: Sbírka Masarykovy univerzity (Jan Novák) Univerzita Karlova, Filozofická fakulta, 2024";
    let got = fold(&school_of(header));
    assert!(got.contains("karlova"));
    assert!(!got.contains("masarykovy"));
}

#[test]
fn a_school_written_in_capitals_still_matches() {
    let header = "Bakalářská práce, OSTRAVSKÁ UNIVERZITA , Přírodovědecká fakulta, 2022";
    let school = fold(&school_of(header));
    assert!(
        distinctive("Ostravská univerzita")
            .iter()
            .all(|w| school.contains(w.as_str()))
    );
}

#[test]
fn a_header_with_no_year_names_no_school() {
    assert_eq!(school_of("Diplomová práce: Něco o něčem (Jan Novák)"), "");
}

#[test]
fn distinctive_keeps_the_words_that_identify_a_school() {
    assert_eq!(distinctive("Univerzita Karlova"), ["karlova"]);
    assert_eq!(
        distinctive("Vysoká škola ekonomická v Praze"),
        ["ekonomicka"]
    );
    assert_eq!(
        distinctive("Univerzita Tomáše Bati ve Zlíně"),
        ["tomase", "bati", "zline"]
    );
}
