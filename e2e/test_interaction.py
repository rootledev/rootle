"""E2E for the v0.3/v0.4 mock flows (plans/0003, plans/0004): ?
keybinds popup, : command line + settings popup, v VISUAL
multi-select, :clone wizard. Mock stage: trigger points and screens
only, no functionality wiring."""

from conftest import dismiss_launch_popup
from tui import Tui


def test_keybinds_popup_scrolls_and_closes(tui: Tui) -> None:
    dismiss_launch_popup(tui)
    tui.send("?")
    screen = tui.expect("keybindings")
    assert "BROWSE" in screen
    for _ in range(20):
        tui.send("j")
    screen = tui.expect("VISUAL")
    assert "LEADER" in screen
    tui.key("ESC")
    tui.expect_gone("keybindings")


def test_command_line_filters_and_opens_settings(tui: Tui) -> None:
    dismiss_launch_popup(tui)
    tui.send(":")
    screen = tui.expect("clone")  # both commands listed
    assert "settings" in screen

    tui.type_query("set")
    screen = tui.expect("settings")
    assert "clone the selected" not in screen  # filtered out

    tui.key("ENTER")
    screen = tui.expect("editor")  # settings popup, first tab
    assert "theme" in screen and "cache" in screen

    tui.key("TAB")  # editor → theme tab
    tui.expect("catppuccin-mocha")

    tui.key("ENTER")  # edit the theme name field
    screen = tui.expect("INSERT")
    tui.key("ESC")  # stop editing
    tui.key("ESC")  # close settings
    tui.expect_gone("settings")


def test_visual_mode_and_clone_wizard(tui: Tui) -> None:
    dismiss_launch_popup(tui)
    tui.send("v")
    screen = tui.expect("○")
    assert "VISUAL" in screen

    # The launch flow sits at the orgs level; drill into repos first.
    tui.key("ENTER")  # org → repos (visual keeps h/l; enter drills)
    tui.send(" ")
    screen = tui.expect("●")

    tui.send("v")  # exit visual
    tui.expect_gone("●")
    tui.expect("BROWSE")

    # :clone picks up the marks.
    tui.send(":")
    tui.type_query("clone")
    tui.key("ENTER")
    screen = tui.expect("clone — 1/3 repos")
    assert "next" in screen

    tui.key("TAB")  # list → buttons
    tui.key("ENTER")  # next → destination
    screen = tui.expect("clone — 2/3 destination")
    assert "dest:" in screen

    tui.key("TAB")
    tui.key("ENTER")  # next → summary
    screen = tui.expect("clone — 3/3 summary")
    assert "git clone" in screen

    tui.key("ESC")  # cancel the whole wizard
    tui.expect_gone("clone —")
