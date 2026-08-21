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
    "getting-started": ("doc/getting-started.md", "getting started"),
    "settings": ("doc/settings.md", "settings"),
    "provider-protocol": ("doc/provider-protocol.md", "providers"),
}

# Links inside the mirrored docs that point at files we do NOT mirror:
# send them to the blob/tree on GitHub instead.
GITHUB_LINKS: dict[str, str] = {
    "development.md": f"{REPO}/blob/main/doc/development.md",
    "house-style.md": f"{REPO}/blob/main/doc/house-style.md",
    "../skills/rootle-provider/SKILL.md": f"{REPO}/tree/main/skills/rootle-provider",
    "../examples/providers/fs_provider.py": f"{REPO}/blob/main/examples/providers/fs_provider.py",
}

# Doc-local images that are not screenshots: copied alongside img/.
DOC_ASSETS = {"architecture.svg"}

RAIL_LINKS = [("index.html", "home")] + [
    (f"docs/{slug}.html", label) for slug, (_, label) in PAGES.items()
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
    return f"""<aside class="rail">
  <a class="brand" href="../index.html">
    <img src="../assets/icon.svg" alt="rootle icon"><span class="wordmark">rootle</span>
  </a>
  <span class="rail-head">menu</span>
  <nav>
{links}    <a class="gh" href="{REPO}">github ↗</a>
  </nav>
{toc_block}</aside>"""


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
  <span style="margin-left:auto">a ratatui TUI · Catppuccin Mocha</span>
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

    # Screenshots: img/NN-*.png -> ../assets/img/NN-*.png
    body = re.sub(r'src="img/', 'src="../assets/img/', body)

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

    shutil.copy(ROOT / "website" / "index.html", OUT / "index.html")
    # Served at tknawara.github.io/rootle/install.sh — the curl-pipe-sh installer.
    shutil.copy(ROOT / "install.sh", OUT / "install.sh")
    for name in ("icon.svg", "favicon.svg", "site.css"):
        shutil.copy(ROOT / "website" / "assets" / name, OUT / "assets" / name)
    shutil.copy(ROOT / "doc" / "logo.svg", OUT / "assets" / "logo.svg")
    shutil.copy(ROOT / "doc" / "demo.gif", OUT / "assets" / "demo.gif")
    for name in DOC_ASSETS:
        shutil.copy(ROOT / "doc" / name, OUT / "assets" / name)
    for img in (ROOT / "doc" / "img").glob("*.png"):
        shutil.copy(img, OUT / "assets" / "img" / img.name)
    print(f"copied landing page + {len(list((ROOT / 'doc' / 'img').glob('*.png')))} screenshots")


if __name__ == "__main__":
    assemble()
    build_docs()
