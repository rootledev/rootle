"""Deterministic screenshots for doc/getting-started.md: the pyte
screen is rendered to PNG cell-by-cell (fg/bg per cell, bold face).
Driven by expect(), so every frame captures exactly the state it
names — no PTY/VHS timing races.

    cd e2e && uv run python shots.py     # writes ../doc/img/*.png
"""

import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from conftest import FS_PROVIDER, make_fs_root, open_fs_repo
from tui import ROOT, Tui, build

IMG = ROOT / "doc" / "img"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
FONT_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
# Fallback for glyphs missing from the Mono faces (▌ ⋮ ● ○ ␣ ▸ …).
FONT_FALLBACK = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
FONT_FALLBACK_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"


def covered_chars(path: str) -> set[int]:
    from fontTools.ttLib import TTFont

    with TTFont(path) as tt:
        return {cp for table in tt["cmap"].tables for cp in table.cmap}

# Mocha defaults for unstyled cells.
DEFAULT_FG = (0xCD, 0xD6, 0xF4)
DEFAULT_BG = (0x1E, 0x1E, 0x2E)

NAMED = {
    "black": (0, 0, 0), "red": (0xF3, 0x8B, 0xA8), "green": (0xA6, 0xE3, 0xA1),
    "yellow": (0xF9, 0xE2, 0xAF), "blue": (0x89, 0xB4, 0xFA),
    "magenta": (0xCB, 0xA6, 0xF7), "cyan": (0x94, 0xE2, 0xD5),
    "white": (0xCD, 0xD6, 0xF4),
    "brightblack": (0x6C, 0x70, 0x86), "brightred": (0xF3, 0x8B, 0xA8),
    "brightgreen": (0xA6, 0xE3, 0xA1), "brightyellow": (0xF9, 0xE2, 0xAF),
    "brightblue": (0x89, 0xB4, 0xFA), "brightmagenta": (0xCB, 0xA6, 0xF7),
    "brightcyan": (0x94, 0xE2, 0xD5), "brightwhite": (0xFF, 0xFF, 0xFF),
}

# xterm-256 palette (colors 16-231 cube + 232-255 grayscale).
CUBE = [0, 95, 135, 175, 215, 255]


def color(value: str, default: tuple) -> tuple:
    """pyte color: 'default', named, '0'-'255' (xterm cube), or 6-hex."""
    if not value or value == "default":
        return default
    if value in NAMED:
        return NAMED[value]
    # xterm-256 indexes are ≤ 255; a 6-digit all-digit string is an
    # RGB hex that happens to be digits (e.g. "313244" = surface0).
    if value.isdigit() and int(value) <= 255:
        n = int(value)
        if n < 8:
            return NAMED[["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"][n]]
        if n < 16:
            return NAMED[["brightblack", "brightred", "brightgreen", "brightyellow",
                          "brightblue", "brightmagenta", "brightcyan", "brightwhite"][n - 8]]
        if n < 232:
            n -= 16
            return (CUBE[n // 36], CUBE[(n // 6) % 6], CUBE[n % 6])
        gray = 8 + (n - 232) * 10
        return (gray, gray, gray)
    if len(value) == 6:
        return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4))
    return default


class Renderer:
    def __init__(self, cols: int, rows: int, cell_px: int = 18) -> None:
        self.font = ImageFont.truetype(FONT, cell_px)
        self.font_bold = ImageFont.truetype(FONT_BOLD, cell_px)
        self.fallback = ImageFont.truetype(FONT_FALLBACK, cell_px)
        self.fallback_bold = ImageFont.truetype(FONT_FALLBACK_BOLD, cell_px)
        # Codepoint coverage of each face — missing glyphs draw as tofu
        # without the fallback.
        self.mono_cover = covered_chars(FONT) | covered_chars(FONT_BOLD)
        self.fallback_cover = covered_chars(FONT_FALLBACK) | covered_chars(
            FONT_FALLBACK_BOLD
        )
        # Cell metrics from real font metrics: glyphs (box chars,
        # descenders, bold) must never bleed into neighboring cells.
        ascent, descent = self.font.getmetrics()
        self.ch = ascent + descent - 1  # 1px overlap: box chars connect
        self.cw = int(
            max(self.font.getlength("M"), self.font_bold.getlength("M"))
        )
        self.cols, self.rows = cols, rows
        self.img = Image.new("RGB", (cols * self.cw, rows * self.ch), DEFAULT_BG)
        self.draw = ImageDraw.Draw(self.img)

    def render(self, tui: Tui, path: Path) -> None:
        # Fresh canvas per frame — skipped (space) cells must not keep
        # the previous frame's glyphs.
        self.img = Image.new("RGB", (self.cols * self.cw, self.rows * self.ch), DEFAULT_BG)
        self.draw = ImageDraw.Draw(self.img)
        screen = tui._screen
        for y in range(self.rows):
            for x in range(self.cols):
                c = screen.buffer[y][x]
                fg = color(c.fg, DEFAULT_FG)
                bg = color(c.bg, DEFAULT_BG)
                if c.reverse:
                    fg, bg = bg, fg
                px, py = x * self.cw, y * self.ch
                if bg != DEFAULT_BG:
                    self.draw.rectangle([px, py, px + self.cw, py + self.ch], fill=bg)
                if c.data != " ":
                    font = self.font_bold if c.bold else self.font
                    if ord(c.data) not in self.mono_cover and ord(c.data) in self.fallback_cover:
                        font = self.fallback_bold if c.bold else self.fallback
                    self.draw.text((px, py), c.data, font=font, fill=fg)
        self.img.save(path)


def main() -> None:
    IMG.mkdir(exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="ghx-shots-"))
    root = make_fs_root(tmp)
    # Extra content so the grep demo shows two regions + a markdown hit.
    (root / "alpha" / "src" / "render.rs").write_text(
        "//! Rendering pipeline.\n"
        "pub fn render(view: &View) -> Frame {\n"
        "    let frame = Frame::new();\n"
        "    view.draw(&frame);\n"
        "    frame\n"
        "}\n"
    )
    config = tmp / "provider.toml"
    config.write_text(
        f'[provider]\nkind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )

    tui = Tui(build(), cols=100, rows=28, args=["--config", str(config)]).start()
    r = Renderer(100, 28)

    def shot(name: str) -> None:
        r.render(tui, IMG / name)
        print(f"  {name}")

    try:
        tui.expect("search github")
        shot("01-launch-search.png")

        tui.type_query("alpha")
        tui.key("ENTER")
        tui.expect("local/alpha")
        shot("02-repo-results.png")
        tui.key("ENTER")
        tui.expect("render.rs")

        tui.send("l")  # into src/, preview follows
        tui.expect("fn render")
        shot("03-browse.png")

        tui.send(" ")
        tui.send("g")
        tui.type_query("render")
        tui.key("ENTER")
        tui.expect("matches")
        shot("04-grep.png")

        tui.send("/")
        tui.type_query("main")
        tui.expect("SEARCH")
        shot("05-results-filter.png")
        tui.key("ESC")

        # Scope popup: results focus → Tab×2 → scope → Enter.
        tui.key("TAB")
        tui.key("TAB")
        tui.key("ENTER")
        tui.expect("current repo")
        shot("06-scope-popup.png")
        tui.key("ESC")  # close popup
        tui.key("ESC")  # close view → browser

        tui.send(" ")
        tui.send("y")
        tui.expect("yanked")
        shot("07-yank.png")

        tui.send("?")
        tui.expect("keybindings")
        shot("08-keybinds.png")
        tui.key("ESC")

        tui.send(":")
        tui.type_query("settings")
        tui.key("ENTER")
        tui.expect("read_only")
        shot("09-settings.png")
        tui.send("l")
        tui.expect("catppuccin-mocha")
        shot("10-settings-theme.png")
        tui.key("ESC")

        # VISUAL marks: drill out to orgs, open the org so both repos
        # list, then mark both on the repos level.
        tui.send("h")
        tui.send("h")
        tui.send("h")
        tui.send("l")  # org → repos (loads the level)
        tui.expect("beta/")
        tui.send("v")
        tui.send(" ")
        tui.send("j")
        tui.send(" ")
        tui.expect("●")
        shot("11-visual.png")
        tui.send("v")

        tui.send(":")
        tui.type_query("clone")
        tui.key("ENTER")
        tui.expect("1/3 repos")
        shot("12-clone-repos.png")
        tui.key("TAB")
        tui.key("ENTER")
        tui.expect("2/3 destination")
        shot("13-clone-destination.png")
        tui.key("TAB")
        tui.key("ENTER")
        tui.expect("3/3 summary")
        shot("14-clone-summary.png")
        tui.key("ESC")
        tui.send("q")
    finally:
        tui.stop()
    print(f"done → {IMG}")


if __name__ == "__main__":
    main()
