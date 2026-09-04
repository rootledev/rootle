"""E2E for plans/0007, headless tier: vim-style find-in-file (␣ /, live
chips, n/N with wrap, line-anchored yank) driven via `rootle --headless -`
on the fs stdio provider.

The theme-switch recolor test stays on the PTY tier (bottom of this
file): it asserts cell-level 24-bit fg colors, which the headless frame —
deliberately text-only — cannot carry."""

from pathlib import Path

from conftest import FS_PROVIDER, Tui
from headless import frames, fs_config, run_headless, states
from tui import build

MAIN_RS = (
    "fn main() {\n"
    "    let view = render();\n"
    "    println!(\"{view}\");\n"
    "}\n"
    "\n"
    "fn render() -> &'static str {\n"
    "    \"rootle\"\n"
    "}\n"
)


def make_root(tmp: Path) -> Path:
    root = tmp / "code"
    (root / "alpha").mkdir(parents=True)
    (root / "alpha" / "main.rs").write_text(MAIN_RS)
    return root


def test_find_in_file_highlights_steps_and_yanks_match_line(tmp_path, binary):
    clip = tmp_path / "clip.txt"
    config = fs_config(tmp_path, root=make_root(tmp_path))
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"  # open the tree: main.rs selected and previewed
        "settle\n"
        "frame\n"
        "keys <space>\n"
        "keys /\n"  # FIND opens over the preview
        "frame\n"
        "keys render\n"  # live: chips + jump while typing
        "frame\n"
        "keys <cr>\n"  # commit: chips stay, back to browse
        "frame\n"
        "state\n"
        "keys n\n"
        "frame\n"
        "keys n\n"  # wraps to the first match
        "frame\n"
        "keys N\n"  # and back again
        "frame\n"
        "keys <space>\n"
        "keys y\n"  # yank anchors at the match line
        "state\n"
        "keys <esc>\n"  # :nohlsearch — chips clear, cursor stays
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        env_extra={"ROOTLE_CLIPBOARD": str(clip)},
    )
    (
        opened,
        find,
        live,
        browse,
        after_n,
        after_wrap,
        after_big_n,
        cleared,
    ) = frames(out)
    committed, yanked = states(out)

    assert "rust · 8 lines" in opened  # footer metadata
    assert "1/8" in opened  # plain readout before find

    assert "FIND" in find

    assert "1/2 · 2/8" in live  # live jump to the first match
    assert "/render" in live  # query rides the preview title

    assert committed["mode"] == "BROWSE"
    assert "1/2 · 2/8" in browse

    assert "2/2 · 6/8" in after_n
    assert "1/2 · 2/8" in after_wrap
    assert "2/2 · 6/8" in after_big_n

    assert (yanked["status"] or "").startswith("yanked")
    assert yanked["yanks"][0].endswith("#L6")  # the match line, not line 1
    assert clip.read_text().endswith("#L6")

    assert "2/2 ·" not in cleared  # chips cleared…
    assert "6/8" in cleared  # …while the cursor stays on the match line


# --- PTY leftover: cell-level style assertions --------------------------------
#
# The headless frame is text-only by design (plans/0023), so a test that
# must read a cell's 24-bit fg color — theme-driven syntax highlighting —
# has no headless expression. It stays on the Tui/pyte tier.


def launch(tmp_path: Path, root: Path, env_extra: dict[str, str] | None = None) -> Tui:
    """Inline PTY launcher (test_wiring's `launch` retired with its port)."""
    config = tmp_path / "provider.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    return Tui(
        build(), cols=110, rows=30, args=["--config", str(config)], env_extra=env_extra
    ).start()


def open_main_rs(tui: Tui) -> str:
    """Search alpha, open its tree: main.rs selected and previewed."""
    tui.type_query("alpha")
    tui.key("ENTER")
    tui.expect("local/alpha")
    tui.key("ENTER")
    # The blob loads over stdio after the meta placeholder ("loading…",
    # readout 1/3) — the footer is the load-complete signal, not the
    # first line of content (a CI-slow runner can snapshot mid-load).
    return tui.expect("rust · 8 lines")


def fg_of(tui: Tui, needle: str) -> str:
    """24-bit fg (rrggbb) of the first cell of `needle` on screen."""
    lines = tui.screen().split("\n")
    y = next(i for i, line in enumerate(lines) if needle in line)
    x = lines[y].index(needle)
    return tui._screen.buffer[y][x].fg


def test_theme_switch_recolors_preview_code(tmp_path: Path) -> None:
    root = make_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        open_main_rs(tui)
        assert fg_of(tui, "fn main()") == "cba6f7"  # mocha mauve keyword

        tui.send(":")
        tui.type_query("settings")
        tui.key("ENTER")
        tui.expect("editor")
        tui.key("TAB")  # → theme section
        tui.send("j")
        tui.send("j")  # mocha → latte → dracula
        tui.send(" ")  # select dracula (live preview)
        tui.key("ESC")  # close: saves + applies
        tui.expect("settings saved")
        assert fg_of(tui, "fn main()") == "ff79c6"  # dracula pink keyword
    finally:
        tui.stop()
