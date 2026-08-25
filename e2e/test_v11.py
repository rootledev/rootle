"""E2E for the v1.1 protocol slice (plans/0006): line-anchored yank via
the preview line cursor, lazy per-hit context past the eager preview
cap (PREVIEW_CAP=8), and provider tolerance of advisory cancels during
rapid hit movement."""

from pathlib import Path

from test_wiring import launch


def make_big_root(tmp: Path) -> Path:
    """One repo with 12 files matching 'needle'; f09 carries a unique
    marker so its lazy context is recognizable (nothing else shows it)."""
    root = tmp / "code"
    repo = root / "big"
    repo.mkdir(parents=True)
    for i in range(1, 13):
        text = f"// file {i}\nlet marker = needle;\n"
        if i == 9:
            text += "needle uniq9 marker\n"
        (repo / f"f{i:02}.rs").write_text(text)
    return root


def test_preview_cursor_anchors_yank(tmp_path: Path) -> None:
    """J/J move the preview line cursor; ␣ y yanks the file URL with a
    #L<line> fragment for the cursor line (fs provider: file://…#L<n>)."""
    clip = tmp_path / "clip.txt"
    root = tmp_path / "code"
    (root / "alpha").mkdir(parents=True)
    (root / "alpha" / "main.rs").write_text(
        "fn main() {\n"
        "    let view = render();\n"
        "    println!(\"{view}\");\n"
        "}\n"
        "\n"
        "fn render() -> &'static str {\n"
        "    \"rootle\"\n"
        "}\n"
    )
    tui = launch(tmp_path, root, {"ROOTLE_CLIPBOARD": str(clip)})
    try:
        tui.type_query("alpha")
        tui.key("ENTER")
        tui.expect("local/alpha")
        tui.key("ENTER")
        tui.expect("main.rs")  # tree loaded, main.rs selected+previewed
        tui.expect("fn main() {")
        tui.expect("1/8")  # preview readout: cursor line 1 of 8
        tui.send("J")
        tui.send("J")
        tui.expect("3/8")
        tui.send(" ")
        tui.send("y")
        tui.expect("yanked")
        assert clip.read_text().endswith("#L3"), clip.read_text()
    finally:
        tui.stop()


def test_lazy_hit_context_and_cancel_tolerance(tmp_path: Path) -> None:
    """Hit 9 is past the eager preview cap — bare until the cursor lands
    on it. Rapid moves across bare hits fire advisory cancels between
    superseded fetches; the provider must keep answering afterwards."""
    root = make_big_root(tmp_path)
    tui = launch(tmp_path, root)
    try:
        tui.type_query("big")
        tui.key("ENTER")
        tui.expect("local/big")
        tui.key("ENTER")
        tui.expect("f01.rs")

        tui.send(" ")
        tui.send("g")
        tui.type_query("needle")
        tui.key("ENTER")
        screen = tui.expect("results — 12")
        assert "uniq9" not in screen  # f09 bare: blob not fetched yet

        # Rapid selection moves over bare hits (cancel supersession).
        for _ in range(8):
            tui.send("j")
        tui.expect("uniq9")  # lazy context landed on the 9th hit

        # The provider survived the cancel storm: a fresh search works.
        tui.key("TAB")  # results → query field
        for _ in range(6):
            tui.key("BACKSPACE")
        tui.type_query("marker")
        tui.key("ENTER")
        tui.expect("results — 12")  # same set, provider still alive
    finally:
        tui.stop()
