"""E2E for plans/0007: theme-driven syntax colors in the preview pane
(settings theme switch restyles cached blobs) and vim-style
find-in-file (␣ /, live chips, n/N with wrap, line-anchored yank)."""

from pathlib import Path

from conftest import Tui
from test_wiring import launch

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


def open_main_rs(tui: Tui) -> str:
    """Search alpha, open its tree: main.rs selected and previewed."""
    tui.type_query("alpha")
    tui.key("ENTER")
    tui.expect("local/alpha")
    tui.key("ENTER")
    return tui.expect("fn main() {")


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


def test_find_in_file_highlights_steps_and_yanks_match_line(tmp_path: Path) -> None:
    clip = tmp_path / "clip.txt"
    root = make_root(tmp_path)
    tui = launch(tmp_path, root, {"ROOTLE_CLIPBOARD": str(clip)})
    try:
        screen = open_main_rs(tui)
        assert "rust · 8 lines" in screen  # footer metadata
        assert "1/8" in screen  # plain readout before find

        tui.send(" ")
        tui.send("/")
        tui.expect("FIND")
        tui.type_query("render")
        tui.expect("1/2 · 2/8")  # live jump to the first match
        tui.expect("/render")  # query rides the preview title

        tui.key("ENTER")  # commit: chips stay, back to browse
        tui.expect("BROWSE")
        tui.expect("1/2 · 2/8")
        # n/N stepping: poll the readout after each key — a raw
        # screen() snapshot races the PTY render on slow runners.
        tui.send("n")
        tui.expect("2/2 · 6/8")
        tui.send("n")  # wraps to the first match
        tui.expect("1/2 · 2/8")
        tui.send("N")  # and back again
        tui.expect("2/2 · 6/8")

        tui.send(" ")
        tui.send("y")  # yank anchors at the match line
        tui.expect("yanked")
        assert clip.read_text().endswith("#L6"), clip.read_text()

        tui.key("ESC")  # :nohlsearch — chips clear, cursor stays
        tui.expect_gone("2/2 ·")
    finally:
        tui.stop()
