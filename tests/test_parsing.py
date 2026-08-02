"""Tests for the theses.cz and repository parsing.

No network. The markup below reproduces the structures seen on the real sites,
with names replaced. The repository shapes are taken from two live pages:

  is.muni.cz    puts the extension in the href
  vskp.vse.cz   serves an extensionless /zp/<id> URL and names the file in the
                link text, for example "Hlavní práce 73157_vala07.pdf, 1.5 MB
                Stáhnout"

_documents asks the server about anything it cannot name from the page, so those
tests replace _as_document.
"""

import pytest
from bs4 import BeautifulSoup

import theses_mcp
from theses_mcp import DOC_LABEL, DOC_RE, _documents, _norm, _txt


def soup(markup):
    return BeautifulSoup(markup, "lxml")


@pytest.fixture
def no_probe(monkeypatch):
    """Fail loudly if a test reaches the network instead of the parser."""
    def unexpected(url):
        pytest.fail(f"_as_document called for {url}")

    monkeypatch.setattr(theses_mcp, "_as_document", unexpected)


@pytest.fixture
def probe_says_pdf(monkeypatch):
    """Every probed URL answers as a PDF."""
    monkeypatch.setattr(
        theses_mcp,
        "_as_document",
        lambda url: {
            "label": "Plný text práce",
            "filename": url.rstrip("/").rsplit("/", 1)[-1] + ".pdf",
            "url": url,
            "confirmed": True,
        },
    )


@pytest.fixture
def probe_says_no(monkeypatch):
    monkeypatch.setattr(theses_mcp, "_as_document", lambda url: None)


# --- _txt -------------------------------------------------------------------


def test_txt_of_a_missing_node_is_empty():
    assert _txt(None) == ""


def test_txt_collapses_runs_of_whitespace():
    assert _txt(soup("<p>Vysoká   škola\n\n ekonomická</p>").p) == "Vysoká škola ekonomická"


def test_txt_drops_the_expand_and_collapse_links():
    node = soup('<div>Anotace <a class="rozbal">více</a><a class="sbal">méně</a></div>').div
    assert _txt(node) == "Anotace"


def test_txt_drops_section_headings():
    node = soup("<div><h3>Klíčová slova</h3><h5>cs</h5>právo, smlouva</div>").div
    assert _txt(node) == "právo, smlouva"


def test_txt_drops_extra_selectors_passed_in():
    node = soup('<div>keep<span class="skip">drop</span></div>').div
    assert _txt(node, "span.skip") == "keep"


def test_txt_of_a_bare_string_strips_brackets_and_nbsp():
    """search() reads the author from the text node that follows the h4."""
    node = soup("<div><h4>Title</h4>\xa0(Bc. Jan Novák)</div>").select_one("h4").next_sibling
    assert _txt(node) == "Bc. Jan Novák"


# --- document link patterns -------------------------------------------------


@pytest.mark.parametrize(
    "label",
    [
        "Hlavní práce 73157_vala07.pdf, 1.5 MB Stáhnout",
        "Oponentura 69167_Havelka.pdf, 500 kB Stáhnout",
        "priloha.docx",
        "data.zip",
        "text.rtf",
    ],
)
def test_doc_re_finds_a_filename_in_a_label(label):
    assert DOC_RE.search(label)


@pytest.mark.parametrize("label", ["Stáhnout", "Zpět na výpis", "1.5 MB"])
def test_doc_re_ignores_a_label_with_no_filename(label):
    assert DOC_RE.search(label) is None


@pytest.mark.parametrize(
    "label",
    ["Plný text práce", "Final thesis", "Posudek vedoucího", "Supervisor's review", "Příloha"],
)
def test_doc_label_recognizes_a_download_without_a_filename(label):
    assert DOC_LABEL.search(label)


# --- _documents -------------------------------------------------------------


MUNI = """
<div>
  <a href="Novak_Bakalarska_Prace_Final.pdf">Plný text práce</a>
  <a href="posudek_vedouciho_Svoboda.pdf">Posudek vedoucího</a>
  <a href="/th/avwwh/">Zpět</a>
</div>
"""

VSE = """
<div>
  <a href="https://insis.vse.cz/zp/73157">Hlavní práce 73157_novak.pdf, 1.5 MB Stáhnout</a>
  <a href="https://insis.vse.cz/zp/73157/posudek/oponent/69167">Oponentura 69167_Svoboda.pdf, 500 kB Stáhnout</a>
</div>
"""


def test_an_extension_in_the_href_is_enough(no_probe):
    files = _documents(soup(MUNI), "https://is.muni.cz/th/avwwh/")
    assert [f["filename"] for f in files] == [
        "Novak_Bakalarska_Prace_Final.pdf",
        "posudek_vedouciho_Svoboda.pdf",
    ]
    assert files[0]["url"] == "https://is.muni.cz/th/avwwh/Novak_Bakalarska_Prace_Final.pdf"


def test_a_link_with_neither_a_filename_nor_a_download_label_is_ignored(no_probe):
    files = _documents(soup(MUNI), "https://is.muni.cz/th/avwwh/")
    assert all("Zpět" not in f["label"] for f in files)


def test_a_filename_in_the_label_is_used_when_the_url_has_no_extension(probe_says_pdf):
    files = _documents(soup(VSE), "https://vskp.vse.cz/82000")
    assert [f["filename"] for f in files] == ["73157_novak.pdf", "69167_Svoboda.pdf"]


def test_the_download_word_is_stripped_from_the_label(probe_says_pdf):
    files = _documents(soup(VSE), "https://vskp.vse.cz/82000")
    assert files[0]["label"] == "Hlavní práce 73157_novak.pdf, 1.5 MB"


def test_an_extensionless_url_is_confirmed_with_the_server(probe_says_pdf):
    files = _documents(soup(VSE), "https://vskp.vse.cz/82000")
    assert all(f["confirmed"] is True for f in files)


def test_an_extensionless_url_that_serves_no_file_is_marked_unconfirmed(probe_says_no):
    files = _documents(soup(VSE), "https://vskp.vse.cz/82000")
    assert all(f["confirmed"] is False for f in files)


def test_a_label_only_download_is_probed_and_kept(probe_says_pdf):
    """ČZU and Škoda Auto name neither the URL nor the text."""
    markup = '<div><a href="https://example.edu/download/1">Final thesis</a></div>'
    files = _documents(soup(markup), "https://example.edu/")
    assert len(files) == 1
    assert files[0]["label"] == "Final thesis"
    assert files[0]["confirmed"] is True


def test_a_label_only_link_that_serves_html_is_dropped(probe_says_no):
    markup = '<div><a href="https://example.edu/download/1">Final thesis</a></div>'
    assert _documents(soup(markup), "https://example.edu/") == []


def test_a_mailto_link_is_skipped(no_probe):
    markup = '<div><a href="mailto:a@b.cz">thesis.pdf</a></div>'
    assert _documents(soup(markup), "https://example.edu/") == []


def test_the_same_url_is_listed_once(no_probe):
    markup = (
        '<div><a href="a.pdf">Plný text</a>'
        '<a href="a.pdf">Plný text práce</a></div>'
    )
    assert len(_documents(soup(markup), "https://example.edu/")) == 1


def test_a_relative_href_resolves_against_the_page(no_probe):
    markup = '<div><a href="files/a.pdf">Plný text</a></div>'
    files = _documents(soup(markup), "https://is.muni.cz/th/avwwh/")
    assert files[0]["url"] == "https://is.muni.cz/th/avwwh/files/a.pdf"


def test_a_page_with_no_downloads_returns_nothing(no_probe):
    assert _documents(soup("<div><a href='/help'>Nápověda</a></div>"), "https://x.cz/") == []


# --- _norm ------------------------------------------------------------------


def test_norm_strips_punctuation_and_spaces():
    assert _norm("Vliv AI na autorské právo: analýza") == "vlivainaautorsképrávoanalýza"


def test_norm_lowercases_czech_letters():
    assert _norm("ŠKODA Čeština") == _norm("škoda čeština")


def test_norm_of_none_is_empty():
    assert _norm(None) == ""


def test_norm_makes_two_titles_that_differ_only_in_punctuation_equal():
    assert _norm("Analýza dat, 2. vydání") == _norm("Analýza dat 2 vydání")
