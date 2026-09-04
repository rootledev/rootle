"""E2E for the v1.1 protocol slice (plans/0006), headless tier:
line-anchored yank via the preview line cursor, lazy per-hit context past
the eager preview cap (PREVIEW_CAP=8), and provider tolerance of advisory
cancels during rapid hit movement — asserted as the durable outcome
(results keep arriving after the storm), not cancel machinery internals."""

from pathlib import Path

from headless import frames, fs_config, run_headless, states


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


def test_preview_cursor_anchors_yank(tmp_path, binary):
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
    config = fs_config(tmp_path, root=root)
    out = run_headless(
        binary,
        "keys alpha\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"  # tree loaded, main.rs selected+previewed
        "settle\n"
        "frame\n"
        "keys J\n"
        "keys J\n"
        "frame\n"
        "keys <space>\n"
        "keys y\n"
        "state\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        env_extra={"ROOTLE_CLIPBOARD": str(clip)},
    )
    before, after = frames(out)
    (state,) = states(out)
    assert "1/8" in before  # preview readout: cursor line 1 of 8
    assert "3/8" in after  # two J moves
    assert (state["status"] or "").startswith("yanked")
    assert state["yanks"][0].endswith("#L3")
    assert clip.read_text().endswith("#L3")


def test_lazy_hit_context_and_cancel_tolerance(tmp_path, binary):
    """Hit 9 is past the eager preview cap — bare until the cursor lands
    on it. Rapid moves across bare hits fire advisory cancels between
    superseded fetches; the provider must keep answering afterwards
    (no wedge: the lazy context lands and a fresh search still returns)."""
    root = make_big_root(tmp_path)
    config = fs_config(tmp_path, root=root)
    out = run_headless(
        binary,
        "keys big\n"
        "keys <cr>\n"
        "settle\n"
        "keys <cr>\n"  # open the repo
        "settle\n"
        "frame\n"
        "keys <space>\n"
        "keys g\n"
        "settle\n"
        "keys needle\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n"
        "keys jjjjjjjj\n"  # rapid selection moves over bare hits
        "settle\n"
        "frame\n"
        "keys <tab>\n"  # results → query field
        "keys <bs><bs><bs><bs><bs><bs>\n"
        "keys marker\n"
        "keys <cr>\n"
        "settle\n"
        "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
    )
    tree, results, landed, researched = frames(out)
    assert "f01.rs" in tree

    assert "results — 12" in results
    assert "uniq9" not in results  # f09 bare: blob not fetched yet

    assert "uniq9" in landed  # lazy context landed on the 9th hit

    # The provider survived the cancel storm: a fresh search works.
    assert "results — 12" in researched
