"""Embedded palettes (plans: theme.rs builtins): the settings radio
lists every embedded theme; --theme applies one on launch."""

from pathlib import Path

from conftest import Tui, dismiss_launch_popup, make_fs_root
from test_wiring import launch


def test_embedded_themes_in_settings_radio(tmp_path: Path) -> None:
    root = make_fs_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        dismiss_launch_popup(tui)
        tui.send(":")
        tui.type_query("settings")
        tui.key("ENTER")
        tui.expect("settings")
        tui.key("TAB")  # editor → theme section
        tui.key("ENTER")  # open the theme radio on [theme].name
        screen = tui.expect("catppuccin-mocha")
        for name in ("dracula", "gruvbox-dark", "nord", "one-dark",
                     "solarized-dark", "tokyo-night",
                     "catppuccin-latte", "github-light", "one-light",
                     "solarized-light"):
            assert name in screen, f"{name} missing from the radio"
    finally:
        tui.stop()
