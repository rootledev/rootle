"""Headless-tier e2e for the v0.2 global search view (plans/0002-v0.2)
— against the REAL path: an fs stdio provider (offline,
deterministic). Covers file find, grep with folded match regions +
count badges, the scope radio popup, extension filtering, the /
results filter, and editor open on a hit. plans/0023: scripted keys +
frame/state dumps, no PTY."""

from __future__ import annotations

from headless import frames, fs_config, run_headless, states

# Search alpha, open its tree: lands browsing local/alpha — the scope
# waterfalls down to the repo.
OPEN = "keys alpha\nkeys <cr>\nsettle\nkeys <cr>\nsettle\n"


def test_file_find_over_provider_tree(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>f\n"
        + "frame\n"
        + "keys main\n"
        + "keys <cr>\n"  # jump
        + "settle\n"  # tree select + lazy blob preview land
        + "state\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "find file" in f[0]
    assert "repo:local/alpha" in f[0]
    assert "fn main() {" in f[1]  # the blob-head preview gates the jump
    assert "src/main.rs" in f[1]
    assert states(out)[0]["mode"] == "BROWSE"


def test_grep_folds_regions_with_count_badge(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "keys render\n"
        + "keys <cr>\n"
        + "settle\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    (f,) = frames(out)
    assert "2 matches" in f
    assert "\u22ee" in f  # folded region
    assert "local/alpha/src/main.rs" in f
    assert "fn render() -> &'static str {" in f


def test_extension_field_narrows_over_provider(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "keys render\n"
        + "keys <tab><tab>\n"  # query → scope → extension
        + "keys rs\n"
        + "keys <cr>\n"
        + "settle\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    (f,) = frames(out)
    assert "local/alpha/src/main.rs" in f
    assert "README.md" not in f  # ext:rs drops the markdown hit


def test_grep_grammar_quotes_negation_language(tmp_path, binary):
    """plans/0012 M1: quoted literals, negation, and language: against
    the fs reference adapter (it post-filters; rootle's own filter is
    a no-op net over its sets)."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        # Negation: -docs drops the README hit (its content says "docs").
        + "keys <space>g\nkeys render -docs\nkeys <cr>\nsettle\nframe\n"
        # Quoted literal: one needle, not two terms.
        + 'keys <space>g\nkeys "fn render"\nkeys <cr>\nsettle\nframe\n'
        # language:markdown keeps only the README.
        + "keys <space>g\nkeys render language:markdown\nkeys <cr>\nsettle\nframe\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "local/alpha/src/main.rs" in f[0]
    assert "README.md" not in f[0], f"negated hit still shown: {f[0]}"
    assert "local/alpha/src/main.rs" in f[1]
    assert "README.md" not in f[1], f"quoted literal split: {f[1]}"
    assert "README.md" in f[2]
    assert "main.rs" not in f[2], f"language filter failed: {f[2]}"


def test_grep_view_scope_radio_popup(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "frame\n"
        + "keys <tab><cr>\n"  # query → scope, open the radio popup
        + "settle\n"
        + "frame\n"
        + "keys j\n"  # the radio follows the cursor immediately
        + "frame\n"
        + "keys <cr>\n"  # …Enter just closes the popup
        + "settle\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "grep" in f[0]
    assert "all of github" in f[1]
    assert "(•) current repo" in f[1]
    assert "(•) current org" in f[2]
    assert "grep · org:local" in f[3]


def test_results_slash_filter_and_editor_open(tmp_path, binary):
    """The / filter narrows hits transiently; Enter expands a hit into
    the whole file and a second Enter opens the editor. Headless never
    suspends: the launch is recorded in `state` editor_jobs (the real
    suspend/resume path is PTY-only)."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "keys render\n"
        + "keys <cr>\n"
        + "settle\n"
        + "keys /\n"  # transient results filter
        + "keys main.rs\n"
        + "state\n"
        + "frame\n"
        + "keys <esc>\n"  # cancel → full list
        + "frame\n"
        # The fixture's hit order is README.md, src/main.rs — j lands
        # on main.rs. Enter expands; a second Enter opens the editor.
        + "keys j\n"
        + "keys <cr>\n"
        + "settle\n"
        + "frame\n"
        + "keys <cr>\n"
        + "settle\n"
        + "state\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert s[0]["mode"] == "SEARCH"
    assert "README.md" not in f[0]
    assert "README.md" in f[1]
    assert "local/alpha/src/main.rs:2" in f[2]
    # The editor launch was recorded, not run: hermetic env resolves
    # $VISUAL to `true`, and the materialized blob carries the repo
    # coords and the rust extension.
    (job,) = s[1]["editor_jobs"]
    assert job.startswith("true ")
    assert "/local/alpha/" in job
    assert job.endswith(".rs")
    # The view stays on the expanded hit — the blob came through the
    # provider.
    assert "local/alpha/src/main.rs" in f[3]
    assert "grep" in f[3]


def test_enter_expands_hit_into_full_file_pane(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "keys render\n"
        + "keys <cr>\n"
        + "settle\n"
        + "keys j\n"  # fixture order: README.md, src/main.rs
        # Enter expands the results area into the hit's whole file,
        # anchored at the match line — the lazy context already warmed
        # the blob, so the pane lands filled.
        + "keys <cr>\n"
        + "settle\n"
        + "frame\n"
        + "keys j\n"  # j/k walk the file; the readout follows
        + "frame\n"
        + "keys <esc>\n"  # folds back to the list, selection intact
        + "frame\n"
        + "keys <cr>\n"  # the expand survives a second round
        + "settle\n"
        + "frame\n"
        + "keys h\n"  # h collapses too
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    assert "local/alpha/src/main.rs:2" in f[0]
    assert "fn main() {" in f[0]  # the whole file, from the top
    assert "2/8" in f[0]  # cursor on the anchor line
    assert "3/8" in f[1]
    assert "2 matches" in f[2]
    assert "local/alpha/src/main.rs" in f[2]
    assert "3/8" not in f[2]  # no lingering file content
    assert "local/alpha/src/main.rs:2" in f[3]
    assert "2 matches" in f[4]


def test_facet_chips_narrow_and_restore(tmp_path, binary):
    """plans/0012 M3: chips appear from the accumulated hits, Enter on
    one commits it as a local filter, Enter again restores the set."""
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>g\n"
        + "keys fn\n"
        + "keys <tab>\n"  # query → scope
        + "keys jj\n"  # repo → org → global: alpha and nested both hit
        + "keys <tab>\n"  # scope → extension (no facets yet: skipped)
        + "keys <cr>\n"  # submit from the extension field
        + "settle\n"
        + "frame\n"
        # Tab to the chip row (results → query → scope → extension →
        # facets); cursor 0 is the alpha chip. Enter commits it.
        + "keys <tab><tab><tab><tab>\n"
        + "keys <cr>\n"
        + "settle\n"
        + "frame\n"
        + "keys <cr>\n"  # Enter on the active chip restores the set
        + "settle\n"
        + "frame\n"
        # Esc from the chip row clears nothing (no committed filter)
        # and closes the view.
        + "keys <esc>\n"
        + "state\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    # Chips over the accumulated set: both repos, rust only ("fn" is
    # in main.rs and lib.rs, not README/notes).
    assert "facets" in f[0]
    assert "local/alpha·1" in f[0]
    assert "local/nested·1" in f[0], f"deep repo chip: {f[0]}"
    assert "rust·2" in f[0]
    assert "local/alpha/src/main.rs" in f[1]
    assert "lib.rs" not in f[1], f"facet should drop the deep hit: {f[1]}"
    assert "lib.rs" in f[2]
    assert states(out)[0]["search_view"] is False
    assert "facets" not in f[3]  # no chip residue in the browser
    assert "README.md" in f[3]


def test_closing_view_restores_browser(tmp_path, binary):
    config = fs_config(tmp_path)
    out = run_headless(
        binary,
        OPEN
        + "keys <space>f\n"
        + "frame\n"
        + "keys <esc>\n"  # query: INSERT → NORMAL
        + "state\n"
        + "keys <esc>\n"  # NORMAL → close view
        + "state\n"
        + "frame\n",
        "--config",
        str(config),
        home=tmp_path / "home",
        cols=110,
    )
    f = frames(out)
    s = states(out)
    assert "find file" in f[0]
    assert s[1]["search_view"] is False
    assert s[1]["mode"] == "BROWSE"
    assert "find file" not in f[1]
    assert "README.md" in f[1]  # browser back where we left it
