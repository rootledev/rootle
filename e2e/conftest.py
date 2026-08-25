"""Shared fixtures: build the binary once, one hermetic Tui per test,
plus the fs stdio provider (plans/0005) for offline provider-path e2e."""

from pathlib import Path

import pytest

from tui import ROOT, Tui, build


@pytest.fixture(scope="session")
def binary():
    return build()


@pytest.fixture
def tui(binary):
    with Tui(binary) as t:
        yield t


def dismiss_launch_popup(tui: Tui) -> None:
    """rootle opens on the repo search popup; close it (INSERT→NORMAL→close).
    ESCs go one call at a time — merged bytes parse as Alt+<key>."""
    tui.expect("search github")
    tui.key("ESC")
    tui.key("ESC")
    tui.expect_gone("search github")


# --- fs stdio provider (plans/0005) --------------------------------------

FS_PROVIDER = ROOT / "examples" / "providers" / "fs_provider.py"


def make_fs_root(tmp: Path) -> Path:
    """Two repos: alpha (rust, mentions render), beta (notes only)."""
    root = tmp / "code"
    alpha = root / "alpha"
    (alpha / "src").mkdir(parents=True)
    (alpha / "src" / "main.rs").write_text(
        "fn main() {\n"
        "    let view = render();\n"
        "    println!(\"{view}\");\n"
        "}\n"
        "\n"
        "fn render() -> &'static str {\n"
        "    \"rootle\"\n"
        "}\n"
    )
    (alpha / "README.md").write_text("# alpha\nrender docs\n")
    beta = root / "beta"
    # Nested repo: group/subgroup/project shape (multi-slash ids).
    deep = root / "nested" / "sub" / "deep"
    deep.mkdir(parents=True)
    (deep / "lib.rs").write_text("pub fn deep_fn() -> u32 {\n    42\n}\n")
    beta.mkdir()
    (beta / "notes.txt").write_text("nothing to see\n")
    return root


@pytest.fixture
def provider_tui(tmp_path, binary):
    """rootle on the fs stdio provider over a temp root, launch popup open."""
    root = make_fs_root(tmp_path)
    config = tmp_path / "provider.toml"
    config.write_text(
        "[provider]\n"
        'kind = "stdio"\n'
        f'command = ["python3", "{FS_PROVIDER}", "{root}"]\n'
    )
    t = Tui(binary, cols=110, rows=30, args=["--config", str(config)]).start()
    yield t
    t.stop()


def open_fs_repo(tui: Tui) -> None:
    """Search alpha, open its tree: lands browsing local/alpha."""
    tui.type_query("alpha")
    tui.key("ENTER")
    tui.expect("local/alpha")
    tui.key("ENTER")
    tui.expect("README.md")
