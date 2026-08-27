"""E2E for the v0.2 global search view (plans/0002-v0.2.md) — against
the REAL path: an fs stdio provider (offline, deterministic). Covers
file find, grep with folded match regions + count badges, the scope
radio popup, extension filtering, the / results filter, and editor
open on a hit."""

from conftest import dismiss_launch_popup, open_fs_repo
from tui import Tui


def test_file_find_over_provider_tree(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)  # local/alpha open → scope waterfalls to repo

    tui.send(" ")
    tui.send("f")
    screen = tui.expect("find file")
    assert "repo:local/alpha" in screen

    tui.type_query("main")
    tui.key("ENTER")
    screen = tui.expect("src/main.rs")
    assert "fn main() {" in screen  # blob head preview via provider
    assert "BROWSE" in screen


def test_grep_folds_regions_with_count_badge(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("g")
    tui.type_query("render")
    tui.key("ENTER")
    # The result block renders progressively and a PTY diff can tear
    # mid-draw — poll for each string rather than asserting on one
    # captured frame.
    tui.expect("2 matches")
    tui.expect("⋮")
    tui.expect("local/alpha/src/main.rs")
    tui.expect("fn render() -> &'static str {")


def test_extension_field_narrows_over_provider(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("g")
    tui.type_query("render")
    tui.key("TAB")  # → scope
    tui.key("TAB")  # → extension
    tui.type_query("rs")
    tui.key("ENTER")
    screen = tui.expect("local/alpha/src/main.rs")
    assert "README.md" not in screen  # ext:rs drops the markdown hit


def test_grep_grammar_quotes_negation_language(provider_tui: Tui) -> None:
    """plans/0012 M1: quoted literals, negation, and language: against
    the fs reference adapter (it post-filters; rootle's own filter is
    a no-op net over its sets)."""
    tui = provider_tui
    open_fs_repo(tui)  # local/alpha — repo scope

    tui.send(" ")
    tui.send("g")
    # Negation: -docs drops the README hit (its content says "docs").
    tui.type_query("render -docs")
    tui.key("ENTER")
    screen = tui.expect("local/alpha/src/main.rs")
    assert "README.md" not in screen, f"negated hit still shown: {screen}"

    # Quoted literal: one needle, not two terms.
    tui.send(" ")
    tui.send("g")
    tui.type_query('"fn render"')
    tui.key("ENTER")
    screen = tui.expect("local/alpha/src/main.rs")
    assert "README.md" not in screen, f"quoted literal split: {screen}"

    # language:markdown keeps only the README.
    tui.send(" ")
    tui.send("g")
    tui.type_query("render language:markdown")
    tui.key("ENTER")
    screen = tui.expect("README.md")
    assert "main.rs" not in screen, f"language filter failed: {screen}"


def test_grep_view_scope_radio_popup(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("g")
    tui.expect("grep")

    tui.key("TAB")  # query → scope
    tui.key("ENTER")  # open radio popup
    screen = tui.expect("all of github")
    assert "(•) current repo" in screen

    tui.send("j")  # radio follows the cursor immediately
    screen = tui.expect("(•) current org")
    tui.key("ENTER")  # …Enter just closes the popup
    tui.expect("grep · org:local")


def test_results_slash_filter_and_editor_open(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("g")
    tui.type_query("render")
    tui.key("ENTER")
    tui.expect("local/alpha/src/main.rs")

    # Transient / filter narrows hits incrementally.
    tui.send("/")
    tui.type_query("main.rs")
    screen = tui.expect("SEARCH")
    assert "README.md" not in screen
    tui.key("ESC")  # cancel → full list
    tui.expect("README.md")

    # Enter expands the hit into the whole file (plans/0012 M2); a
    # second Enter opens the editor (VISUAL=true: no-op suspend/
    # resume) — the blob comes through the provider. The fixture's
    # hit order is README.md, src/main.rs — j lands on main.rs.
    tui.send("j")
    tui.key("ENTER")
    tui.expect("local/alpha/src/main.rs:2")
    tui.key("ENTER")
    screen = tui.expect("local/alpha/src/main.rs")
    assert "grep" in screen


def test_enter_expands_hit_into_full_file_pane(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("g")
    tui.type_query("render")
    tui.key("ENTER")
    tui.expect("local/alpha/src/main.rs")
    tui.send("j")  # fixture order: README.md, src/main.rs

    # Enter expands the results area into the hit's whole file,
    # anchored at the match line — the lazy context already warmed
    # the blob, so the pane lands filled.
    tui.key("ENTER")
    # PTY diffs can tear mid-draw — poll each marker, never assert on
    # one captured frame.
    tui.expect("local/alpha/src/main.rs:2")
    tui.expect("fn main() {")  # the whole file, from the top
    tui.expect("2/8")  # cursor on the anchor line

    # j/k walk the file; the readout follows.
    tui.send("j")
    tui.expect("3/8")

    # Esc folds back to the results list — selection intact, no
    # lingering file content.
    tui.key("ESC")
    screen = tui.expect("2 matches")
    assert "local/alpha/src/main.rs" in screen
    assert "3/8" not in screen

    # h collapses too, and the expand survives a second round.
    tui.key("ENTER")
    tui.expect("local/alpha/src/main.rs:2")
    tui.send("h")
    tui.expect("2 matches")


def test_closing_view_restores_browser(provider_tui: Tui) -> None:
    tui = provider_tui
    open_fs_repo(tui)

    tui.send(" ")
    tui.send("f")
    tui.expect("find file")
    tui.key("ESC")  # query: INSERT → NORMAL
    tui.key("ESC")  # NORMAL → close view
    screen = tui.expect_gone("find file")
    assert "README.md" in screen  # browser back where we left it
