#
# Project: theses-mcp
# File:    test_harness.py
#
# Description:
# Tests for the coverage harness, which decides which school a search result belongs to.
#
# Author:
# Jan Alexandr Kopřiva
# jan.alexandr.kopriva@gmail.com
#
# License: MIT
#

"""Tests for tools/e2e_schools.py.

The harness produces the coverage numbers in the README, so a bug here does not
fail loudly. It reports a school as unreachable instead. That is what happened:
one header shape parsed to the faculty alone, and 16 institutions carried a
verdict they had not earned.

The headers below are copied from live theses.cz search results.
"""

import pathlib
import sys

import pytest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "tools"))

from e2e_schools import distinctive, fold, school_of

# "práce," then the school
FLAT = ("Diplomová práce, Univerzita Jana Amose Komenského Praha, 2013 "
        "Pedagogika / Andragogika Příbuzné soubory: th76_1201110365.pdf")

# "práce:" then the title and the author, and only then the school
TITLED = ("Diplomová práce: Univerzita jako sbírkotvorná instituce. Umělecká sbírka "
          "Masarykovy univerzity (Simona Tichá) Masarykova univerzita, "
          "Filozofická fakulta, 2024")


def test_the_school_follows_the_word_prace():
    assert fold(school_of(FLAT)).strip() == "univerzita jana amose komenskeho praha"


def test_the_school_follows_the_author_when_a_title_comes_first():
    """An anchor on "práce," swallows the title and returns the faculty alone."""
    assert fold(school_of(TITLED)).strip() == "masarykova univerzita filozoficka fakulta"


def test_the_title_is_left_out_even_when_it_names_another_school():
    """The title here says "Masarykovy univerzity" and the thesis is not from Brno."""
    header = ("Diplomová práce: Sbírka Masarykovy univerzity (Jan Novák) "
              "Univerzita Karlova, Filozofická fakulta, 2024")
    got = fold(school_of(header))
    assert "karlova" in got
    assert "masarykovy" not in got


def test_a_school_written_in_capitals_still_matches():
    header = "Bakalářská práce, OSTRAVSKÁ UNIVERZITA , Přírodovědecká fakulta, 2022"
    assert all(w in fold(school_of(header)) for w in distinctive("Ostravská univerzita"))


def test_a_header_with_no_year_names_no_school():
    assert school_of("Diplomová práce: Něco o něčem (Jan Novák)") == ""


@pytest.mark.parametrize("name,want", [
    ("Univerzita Karlova", ["karlova"]),
    ("Vysoká škola ekonomická v Praze", ["ekonomicka"]),
    ("Univerzita Tomáše Bati ve Zlíně", ["tomase", "bati", "zline"]),
])
def test_distinctive_keeps_the_words_that_identify_a_school(name, want):
    assert distinctive(name) == want
