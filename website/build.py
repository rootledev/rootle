#!/usr/bin/env python3
"""Assemble the rootle website into public/.

Single source of truth: the landing page is website/index.html, and the
docs pages are converted from doc/*.md at build time — editing a doc in
the repo is all it takes to update the site (pages.yml redeploys on
doc/** changes). Every page shares one chrome: a left rail (brand, site
nav, on-this-page TOC on docs) and a content column — the same markup
website/index.html carries by hand.

    uv run --with markdown python website/build.py    # → public/
"""

from __future__ import annotations

import re
import shutil
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "public"

REPO = "https://github.com/tknawara/rootle"

# Docs mirrored onto the site: url-slug -> (source file, nav label).
PAGES: dict[str, tuple[str, str]] = {
    "settings": ("doc/settings.md", "settings"),
    # Trimmed site version; doc/provider-protocol.md stays the full spec.
    "provider-protocol": ("website/providers.md", "providers"),
}

# Links inside the mirrored docs that point at files we do NOT mirror:
# send them to the blob/tree on GitHub instead.
GITHUB_LINKS: dict[str, str] = {
    "themes.md": f"{REPO}/blob/main/doc/themes.md",
    "development.md": f"{REPO}/blob/main/doc/development.md",
    "house-style.md": f"{REPO}/blob/main/doc/house-style.md",
    "../skills/rootle-provider/SKILL.md": f"{REPO}/tree/main/skills/rootle-provider",
    "../examples/providers/fs_provider.py": f"{REPO}/blob/main/examples/providers/fs_provider.py",
}

# Doc-local images that are not screenshots: copied alongside img/.
DOC_ASSETS = {"architecture.svg"}

# Palette roles in doc/logo.svg -> site CSS vars, so the inline logo
# re-themes with the palette picker. The mole's illustration colors
# (fur shading, whiskers, eye highlights) are character colors and stay
# hardcoded; mode_leader is warm orange in every palette, so mapping
# fur to --peach keeps the mole a mole.
LOGO_VARS = {
    "#11111b": "var(--bg)",            # crust — backdrop, eyes
    "#1e1e2e": "var(--card)",          # base — terminal card
    "#181825": "var(--deep)",          # mantle — modeline bar
    "#313244": "var(--border)",        # surface0 — selection, scrollbar track
    "#45475a": "color-mix(in srgb, var(--border) 55%, var(--border-strong))",  # surface1
    "#585b70": "var(--border-strong)",  # surface2 — unfocused borders
    "#6c7086": "var(--faint)",         # overlay0 — modeline hints
    "#a6adc8": "var(--dim)",           # subtext0 — entries, code lines
    "#cdd6f4": "var(--text)",          # wordmark
    "#89b4fa": "var(--blue)",          # border_focused
    "#a6e3a1": "var(--green)",         # mode_browse
    "#f9e2af": "var(--yellow)",        # mode_search
    "#fab387": "var(--peach)",         # mode_leader — mole fur
    "#f38ba8": "var(--red)",           # error
    "#cba6f7": "var(--mauve)",         # mode_visual
    "#94e2d5": "var(--teal)",          # mode_insert
}


def themed_svg(path: Path) -> str:
    """An SVG with palette hexes swapped for site CSS vars — inline it so
    the palette picker re-themes it (an <img> can't inherit page CSS)."""
    svg = path.read_text()
    for hex_color, var in LOGO_VARS.items():
        svg = svg.replace(hex_color, var)
    return svg.strip()


def themed_logo() -> str:
    return themed_svg(ROOT / "doc" / "logo.svg")


RAIL_LINKS = [("index.html", "home")] + [
    (f"docs/{slug}.html", label) for slug, (_, label) in PAGES.items()
]


# Palette dots in the docs rail: name + accent color (border_focused),
# mirroring the landing's picker so themes are switchable from any page.
PALETTE_DOTS = [
    ("catppuccin-mocha", "#89b4fa"),
    ("dracula", "#bd93f9"),
    ("gruvbox-dark", "#83a598"),
    ("nord", "#88c0d0"),
    ("one-dark", "#61afef"),
    ("solarized-dark", "#268bd2"),
    ("tokyo-night", "#7aa2f7"),
]


def rail(active: str, toc: list[tuple[str, str]]) -> str:
    links = "".join(
        f'    <a{" class=\"active\"" if href == active else ""} href="../{href}">{label}</a>\n'
        for href, label in RAIL_LINKS
    )
    toc_html = "".join(f'    <a href="#{anchor}">{label}</a>\n' for label, anchor in toc)
    toc_block = (
        f'  <span class="rail-head">on this page</span>\n  <div class="rail-toc">\n{toc_html}  </div>\n'
        if toc_html
        else ""
    )
    dots = "".join(
        f'    <button data-set-palette="{name}" title="{name}" '
        f'style="--sw:{color}" aria-label="{name}"></button>\n'
        for name, color in PALETTE_DOTS
    )
    return f"""<aside class="rail">
  <a class="brand" href="../index.html">
    <img src="../assets/icon.svg" alt="rootle icon"><span class="wordmark">rootle</span>
  </a>
  <span class="rail-head">menu</span>
  <nav>
{links}    <a class="gh" href="{REPO}">github ↗</a>
  </nav>
{toc_block}  <span class="rail-head">theme</span>
  <div class="rail-palettes">
{dots}  </div>
</aside>"""


def page(title: str, body: str, active: str, toc: list[tuple[str, str]]) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="rootle — a modal terminal UI for browsing remote source-control systems.">
<link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="../assets/site.css">
<script src="../assets/site.js" defer></script>
</head>
<body class="docs">
<div class="shell">
{rail(active, toc)}
<main class="content">
<article class="md">
{body}
</article>
<footer>
  <span>MIT license</span>
  <a href="{REPO}">source</a>
  <a href="../index.html">home</a>
  <span style="margin-left:auto">a ratatui TUI · seven palettes</span>
</footer>
</main>
</div>
</body>
</html>
"""


def extract_toc(md_body: str) -> list[tuple[str, str]]:
    """(label, anchor) for every h2 the toc extension stamped with an id.
    Labels keep their HTML entities — they are inserted as HTML."""
    toc = []
    for m in re.finditer(r'<h2 id="([^"]+)">(.*?)</h2>', md_body):
        label = re.sub(r"<[^>]+>", "", m.group(2))
        toc.append((label, m.group(1)))
    return toc


def rewrite(body: str, slugs: set[str]) -> str:
    """Fix links/images in converted doc HTML for their new home."""

    # Doc-local assets (diagrams): architecture.svg -> ../assets/…
    for name in DOC_ASSETS:
        body = body.replace(f'src="{name}"', f'src="../assets/{name}"')

    # Sibling docs that ARE mirrored -> their site page.
    for slug in slugs:
        body = body.replace(f'href="{slug}.md"', f'href="./{slug}.html"')

    # Files we don't mirror -> GitHub.
    for src, dst in GITHUB_LINKS.items():
        body = body.replace(f'href="{src}"', f'href="{dst}"')

    return body


def build_docs() -> None:
    slugs = set(PAGES)
    for slug, (src, _) in PAGES.items():
        text = (ROOT / src).read_text()
        body = markdown.markdown(
            text, extensions=["fenced_code", "tables", "toc", "sane_lists"]
        )
        body = rewrite(body, slugs)
        # Inline + theme doc-local SVG diagrams (they live behind <img>
        # otherwise, which can't inherit the page's palette vars).
        for name in DOC_ASSETS:
            if name.endswith(".svg"):

                def inline_svg(m: re.Match, name: str = name) -> str:
                    alt = re.search(r'alt="([^"]*)"', m.group(0))
                    label = alt.group(1) if alt else name
                    return (
                        f'<div class="diagram" role="img" aria-label="{label}">'
                        f"{themed_svg(ROOT / 'doc' / name)}</div>"
                    )

                body = re.sub(
                    rf'<img [^>]*src="\.\./assets/{re.escape(name)}"[^>]*>',
                    inline_svg,
                    body,
                )
        title = re.match(r"# (.+)", text).group(1).strip()
        toc = extract_toc(body)
        dst = OUT / "docs" / f"{slug}.html"
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(
            page(f"rootle — {title.lower()}", body, f"docs/{slug}.html", toc)
        )
        print(f"built docs/{slug}.html from {src} ({len(toc)} toc entries)")


def assemble() -> None:
    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / "assets" / "img").mkdir(parents=True)
    (OUT / "docs").mkdir(parents=True)

    index = (ROOT / "website" / "index.html").read_text()
    assert "<!--LOGO-->" in index, "index.html lost its <!--LOGO--> placeholder"
    (OUT / "index.html").write_text(index.replace("<!--LOGO-->", themed_logo()))
    # Served at tknawara.github.io/rootle/install.sh — the curl-pipe-sh installer.
    shutil.copy(ROOT / "install.sh", OUT / "install.sh")
    for name in ("icon.svg", "favicon.svg", "site.css", "site.js"):
        shutil.copy(ROOT / "website" / "assets" / name, OUT / "assets" / name)
    shutil.copy(ROOT / "doc" / "logo.svg", OUT / "assets" / "logo.svg")
    shutil.copy(ROOT / "doc" / "demo.gif", OUT / "assets" / "demo.gif")
    for name in DOC_ASSETS:
        shutil.copy(ROOT / "doc" / name, OUT / "assets" / name)
    for gif in (ROOT / "doc" / "img").glob("*.gif"):
        shutil.copy(gif, OUT / "assets" / "img" / gif.name)
    print(f"copied landing page + {len(list((ROOT / 'doc' / 'img').glob('*.gif')))} themed demo GIFs")


if __name__ == "__main__":
    assemble()
    build_docs()
