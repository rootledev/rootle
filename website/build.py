#!/usr/bin/env python3
"""Assemble the rootle website into public/.

Single source of truth: the landing page is website/index.html, and the
docs pages are converted from doc/*.md at build time — editing a doc in
the repo is all it takes to update the site (pages.yml redeploys on
doc/** changes).

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

# Docs mirrored onto the site: url-slug -> source file (relative to ROOT).
PAGES: dict[str, str] = {
    "getting-started": "doc/getting-started.md",
    "settings": "doc/settings.md",
}

# Links inside the mirrored docs that point at files we do NOT mirror:
# send them to the blob/tree on GitHub instead.
GITHUB_LINKS: dict[str, str] = {
    "provider-protocol.md": f"{REPO}/blob/main/doc/provider-protocol.md",
    "development.md": f"{REPO}/blob/main/doc/development.md",
    "house-style.md": f"{REPO}/blob/main/doc/house-style.md",
    "../skills/rootle-provider/SKILL.md": f"{REPO}/tree/main/skills/rootle-provider",
}


def nav(prefix: str, active: str) -> str:
    def link(target: str, label: str) -> str:
        slug = target.rsplit("/", 1)[-1].removesuffix(".html")
        cls = ' class="active"' if slug == active else ""
        return f'<a{cls} href="{prefix}{target}">{label}</a>'

    home_cls = ' class="active"' if active == "index" else ""
    return f"""
<header>
  <nav>
    <img class="icon" src="{prefix}assets/icon.svg" alt="rootle icon">
    <span class="wordmark">rootle</span>
    <span class="links">
      <a{home_cls} href="{prefix}index.html">home</a>
      {link("docs/getting-started.html", "getting started")}
      {link("docs/settings.html", "settings")}
    </span>
    <span class="spacer"></span>
    <a class="github" href="{REPO}">github ↗</a>
  </nav>
</header>"""


def footer() -> str:
    return f"""
<footer>
  <span>MIT license</span>
  <a href="{REPO}">source</a>
  <a href="{REPO}/blob/main/doc/getting-started.md">docs</a>
  <span style="margin-left:auto">a ratatui TUI · Catppuccin Mocha</span>
</footer>"""


def page(title: str, body: str, prefix: str, active: str) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="rootle — a modal terminal UI for browsing remote source-control systems.">
<link rel="icon" href="{prefix}assets/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="{prefix}assets/site.css">
</head>
<body class="docs">
<div class="wrap">
{nav(prefix, active)}
{body}
{footer()}
</div>
</body>
</html>
"""


def rewrite(html: str, slugs: set[str]) -> str:
    """Fix links/images in converted doc HTML for their new home."""

    # Screenshots: img/NN-*.png -> ../assets/img/NN-*.png
    html = re.sub(r'src="img/', 'src="../assets/img/', html)

    # Sibling docs that ARE mirrored -> their site page.
    for slug in slugs:
        html = html.replace(f'href="{slug}.md"', f'href="./{slug}.html"')

    # Docs we don't mirror -> GitHub.
    for src, dst in GITHUB_LINKS.items():
        html = html.replace(f'href="{src}"', f'href="{dst}"')

    return html


def build_docs() -> None:
    slugs = set(PAGES)
    for slug, src in PAGES.items():
        text = (ROOT / src).read_text()
        body = markdown.markdown(text, extensions=["fenced_code", "tables"])
        body = rewrite(body, slugs)
        title = re.match(r"# (.+)", text).group(1).strip()
        article = f'<article class="md">\n{body}\n</article>'
        dst = OUT / "docs" / f"{slug}.html"
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(page(f"rootle — {title.lower()}", article, "../", f"docs/{slug}.html"))
        print(f"built docs/{slug}.html from {src}")


def assemble() -> None:
    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / "assets" / "img").mkdir(parents=True)
    (OUT / "docs").mkdir(parents=True)

    shutil.copy(ROOT / "website" / "index.html", OUT / "index.html")
    for name in ("icon.svg", "favicon.svg", "site.css"):
        shutil.copy(ROOT / "website" / "assets" / name, OUT / "assets" / name)
    shutil.copy(ROOT / "doc" / "logo.svg", OUT / "assets" / "logo.svg")
    shutil.copy(ROOT / "doc" / "demo.gif", OUT / "assets" / "demo.gif")
    for img in (ROOT / "doc" / "img").glob("*.png"):
        shutil.copy(img, OUT / "assets" / "img" / img.name)
    print(f"copied landing page + {len(list((ROOT / 'doc' / 'img').glob('*.png')))} screenshots")


if __name__ == "__main__":
    assemble()
    build_docs()
