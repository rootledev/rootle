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
    screen = tui.expect("local/alpha/src/main.rs")
    assert "2 matches" in screen  # render on lines 2 and 6, folded
    assert "⋮" in screen  # region separator between the two matches
    assert "fn render() -> &'static str {" in screen


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

    # Enter on a hit opens the editor (VISUAL=true: no-op suspend/
    # resume) — the blob comes through the provider.
    tui.key("ENTER")
    screen = tui.expect("local/alpha/src/main.rs")
    assert "grep" in screen


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
