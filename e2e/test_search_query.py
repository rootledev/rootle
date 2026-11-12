"""Pure reference-provider query contracts; no TUI process required."""

from pathlib import Path
import importlib

import pytest


@pytest.fixture
def search(monkeypatch):
    examples = Path(__file__).resolve().parents[1] / "crates/stdio/examples"
    monkeypatch.syspath_prepend(str(examples))
    return importlib.import_module("fs_search")


@pytest.mark.parametrize("alias", ["cpp", "c++", "cxx"])
def test_language_aliases_share_local_and_progressive_selection(search, tmp_path, alias):
    repo = tmp_path / "example"
    repo.mkdir()
    for name in ["unit.cpp", "unit.cp", "unit.hxx", "unit.tpp", "unit.rs", "unit.h"]:
        (repo / name).write_text("needle\n")
    query = f"needle language:'{alias}' -extension:tpp"
    expected = ["unit.cp", "unit.cpp", "unit.hxx"]
    items, clipped = search.search_code(str(tmp_path), query, None)
    batches, streamed_clipped = search.search_code_batches(str(tmp_path), query, None)
    assert sorted(item["path"] for item in items) == expected
    assert sorted(item["path"] for batch in batches for item in batch) == expected
    assert not clipped and not streamed_clipped


def test_quotes_paths_repeated_families_and_content_and(search):
    query = search.parse_query(r'''"language:cpp" 'two words' language:rs language:'c++' -path:vendor''')
    assert query["terms"] == ["language:cpp", "two words"]
    assert search.file_in_scope("src/a.cp", "language:cpp two words", query) is not None
    assert search.file_in_scope("src/a.rs", "language:cpp two words", query) is not None
    assert search.file_in_scope("vendor/a.cp", "language:cpp two words", query) is None
    assert search.file_in_scope("src/a.cp", "language:cpp", query) is None
    escaped = search.parse_query(r'''"a\"b" 'C:\temp' ''')
    assert escaped["terms"] == ['a"b', r"C:\temp"]
    # Filename scope is not a content needle (and smart case is intentional).
    paths = search.parse_query('path:"Source Files" -path:vendor')
    assert search.file_in_scope("Source Files/a.cpp", "unrelated", paths) == []
    assert search.file_in_scope("source files/a.cpp", "Source Files", paths) is None
    assert search.file_in_scope("other.cpp", "Source Files", paths) is None


@pytest.mark.parametrize("query", [
    "language:cp", "language:", "extension:.", 'language:"cpp',
    '"unfinished', "glob:*.rs", "regex:needle", "needle OR other", "NOT",
])
def test_invalid_queries_fail_in_both_provider_paths(search, tmp_path, query):
    for run in (search.search_code, search.search_code_batches):
        with pytest.raises(ValueError):
            run(str(tmp_path), query, None)


def test_colon_literals_and_case_sensitive_language_membership(search):
    parsed = search.parse_query(r"std::vector https://example.test C:\temp x:y")
    assert parsed["terms"] == ["std::vector", "https://example.test", r"C:\temp", "x:y"]
    assert search.parse_query("path::suffix")["path"] == [":suffix"]
    language = search.parse_query("language:cpp")
    assert search.file_in_scope("a.cpp", "", language) == []
    assert search.file_in_scope("a.CPP", "", language) is None
    assert search.file_in_scope("folder.cpp/README", "", language) is None
    assert search.file_in_scope(".rs", "", search.parse_query("language:rs")) == []
    for incomplete in ["NOT NOT", "-"]:
        with pytest.raises(ValueError):
            search.parse_query(incomplete)
