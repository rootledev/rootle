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
    for _ in range(24):
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
    screen = tui.expect("editor")  # settings popup, editor section
    assert "theme" in screen and "cache" in screen

    tui.key("TAB")  # editor → theme section
    screen = tui.expect("catppuccin-mocha")
    assert "\u25cf" in screen  # current theme is a filled radio dot

    # The theme radio is inline: 7 embedded palettes sit between the
    # `name` row and `path` — walk past all of them.
    for _ in range(7):
        tui.send("j")
    tui.key("ENTER")  # edit the path field
    screen = tui.expect("INSERT")
    tui.key("ESC")  # stop editing
    tui.key("ESC")  # close settings (clean → no save)
    tui.expect_gone("settings")


def test_visual_mode_and_clone_wizard(provider_tui: Tui) -> None:
    tui = provider_tui
    # Deterministic fs provider: load the org's repos, mark both on
    # the repos level.
    tui.type_query("zzz")  # no repo match → org listed
    tui.key("ENTER")
    tui.expect("local")
    tui.key("ENTER")
    tui.expect("beta/")

    tui.send("v")
    screen = tui.expect("\u25cb")
    assert "VISUAL" in screen
    tui.send(" ")  # mark alpha
    tui.send("j")
    tui.send(" ")  # mark beta
    tui.expect("\u25cf")

    # ':' opens straight from VISUAL (marks persist) — :clone walks
    # the marked repos through the wizard.
    tui.send(":")
    tui.type_query("clone")
    tui.key("ENTER")
    screen = tui.expect("clone — 1/3 repos")
    assert "local/alpha" in screen and "local/beta" in screen

    tui.key("TAB")
    tui.key("ENTER")  # → destination
    tui.expect("2/3 destination")
    tui.key("TAB")
    tui.key("ENTER")  # → summary
    screen = tui.expect("3/3 summary")
    assert "git clone" in screen

    tui.key("ESC")  # cancel the whole wizard (mode is still VISUAL)
    tui.expect_gone("clone —")
    assert "VISUAL" in tui.screen()

    tui.send("v")  # exit visual; marks stay visible
    tui.expect("BROWSE")
    assert "\u25cf" in tui.screen()
    tui.send(" ")
    tui.send("c")  # ␣ c clears the marks
    tui.expect_gone("\u25cf")
