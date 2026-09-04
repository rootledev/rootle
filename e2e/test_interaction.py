"""Headless-tier e2e for the v0.3/v0.4 mock flows (plans/0003,
plans/0004): ? keybinds popup, : command line + settings popup, v
VISUAL multi-select, :clone wizard. plans/0023: scripted keys +
frame/state dumps, no PTY — trigger points and screens only."""

from __future__ import annotations

from headless import frames, fs_config, run_headless, states

# Load the org's repos: no repo matches "zzz", so the org itself is
# listed; Enter submits, Enter selects — lands on the repos level.
OPEN_ORG = "keys zzz\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"


def test_keybinds_popup_scrolls_and_closes(tmp_path, binary):
    out = run_headless(
        binary,
        "keys <esc><esc>\n"  # close the launch popup (INSERT→NORMAL→close)
        "keys ?\n"
        "frame\n"
        + "keys " + "j" * 24 + "\n"  # scroll the binding rows
        + "frame\n"
        "keys <esc>\n"
        "state\n"
        "frame\n",
        home=tmp_path / "home",
        cols=120,
        rows=36,
    )
    f = frames(out)
    assert "keybindings" in f[0]
    assert "BROWSE" in f[0]
    # The mode sidebar stays put through the scroll.
    assert "VISUAL" in f[1]
    assert "LEADER" in f[1]
    assert states(out)[0]["help"] is False
    assert "keybindings" not in f[2]


def test_command_line_filters_and_opens_settings(tmp_path, binary):
    out = run_headless(
        binary,
        "keys <esc><esc>\n"
        "keys :\n"
        "frame\n"  # both commands listed
        "keys set\n"
        "frame\n"  # filtered to settings
        "keys <cr>\n"
        "settle\n"  # settings popup opens
        "frame\n"
        "keys <tab>\n"  # editor → theme section
        "frame\n"
        # The theme radio is inline: 11 embedded palettes sit between
        # the `name` row and `path` — walk past all of them.
        + "keys " + "j" * 11 + "\n"
        + "keys <cr>\n"  # edit the path field
        "state\n"  # INSERT
        "keys <esc>\n"  # stop editing
        "keys <esc>\n"  # close settings (clean → no save)
        "state\n"
        "frame\n",
        home=tmp_path / "home",
        cols=120,
        rows=36,
    )
    f = frames(out)
    s = states(out)
    assert "clone" in f[0]  # both commands listed
    assert "settings" in f[0]
    assert "settings" in f[1]
    assert "clone the selected" not in f[1]  # filtered out
    assert "editor" in f[2]  # settings popup, editor section
    assert "theme" in f[2] and "cache" in f[2]
    assert "catppuccin-mocha" in f[3]
    assert "\u25cf" in f[3]  # current theme is a filled radio dot
    assert s[0]["mode"] == "INSERT"
    assert s[1]["settings"] is False
    assert "settings" not in f[4]


def test_visual_mode_and_clone_wizard(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN_ORG
        + "frame\n"  # org repos level loaded
        + "keys v\n"  # VISUAL over the repos
        + "state\n"
        + "frame\n"  # empty checkboxes
        + "keys <space>j<space>\n"  # mark alpha, move, mark beta
        + "frame\n"
        # ':' opens straight from VISUAL (marks persist) — :clone
        # walks the marked repos through the wizard.
        + "keys :clone<cr>\n"
        + "settle\n"
        + "state\n"  # the wizard is up; it owns the modeline as BROWSE
        + "frame\n"
        + "keys <tab><cr>\n"  # → destination
        + "frame\n"
        + "keys <tab><cr>\n"  # → summary
        + "frame\n"
        + "keys <esc>\n"  # cancel the whole wizard
        + "state\n"
        + "frame\n"
        + "keys v\n"  # exit visual; marks stay visible
        + "state\n"
        + "frame\n"
        + "keys <space>c\n"  # ␣ c clears the marks
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert "beta/" in f[0]
    assert s[0]["mode"] == "VISUAL"
    assert "\u25cb" in f[1]
    assert "\u25cf" in f[2]
    assert s[1]["wizard"] is True  # up while the wizard shows
    assert "clone — 1/3 repos" in f[3]
    assert "local/alpha" in f[3] and "local/beta" in f[3]
    assert "2/3 destination" in f[4]
    assert "3/3 summary" in f[5]
    assert "git clone" in f[5]
    # Cancelled: the wizard is gone but the mode is still VISUAL.
    assert s[2]["wizard"] is False
    assert s[2]["mode"] == "VISUAL"
    assert "clone —" not in f[6]
    assert s[3]["mode"] == "BROWSE"
    assert "\u25cf" in f[7]  # marks survive leaving VISUAL
    assert "\u25cf" not in f[8]  # …and ␣ c clears them
