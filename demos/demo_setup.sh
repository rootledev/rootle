#!/bin/sh
# Demo fixture for demos/demo.tape: a two-repo "code root" served by the
# fs stdio provider, plus the provider config. Idempotent.
set -eu

DEMO=/tmp/rootle-demo
rm -rf "$DEMO"
mkdir -p "$DEMO/code/alpha/src" "$DEMO/code/beta"

cat > "$DEMO/code/alpha/README.md" <<'EOF'
# alpha
render docs
EOF
cat > "$DEMO/code/beta/notes.txt" <<'EOF'
nothing to see
EOF

# Plain files always (the VHS container has python3 but may lack git —
# the config write below must never be skipped).
cat > "$DEMO/code/alpha/src/main.rs" <<'EOF'
fn main() {
    let view = render();
    println!("{view}");
}

fn render() -> &'static str {
    "rootle"
}
EOF
cat > "$DEMO/code/alpha/src/render.rs" <<'EOF'
//! Rendering pipeline.
pub fn render(view: &View) -> Frame {
    let frame = Frame::new();
    view.draw(&frame);
    frame
}
EOF

# alpha becomes a git worktree when git exists (protocol v1.5): two
# commits on main, a feature branch that grows main.rs, a tag on main
# — fixed authors and dates so the lenses render deterministically.
if command -v git >/dev/null 2>&1; then
(
    src="$DEMO/code/alpha"
    cd "$src"
    git init -q -b main
    git config user.email "demo@rootle.dev"
    git config user.name "Tarek"
    export GIT_AUTHOR_DATE=2026-08-01T10:00:00Z GIT_COMMITTER_DATE=2026-08-01T10:00:00Z
    git add .
    git commit -qm "feat: initial render pipeline"
    export GIT_AUTHOR_DATE=2026-08-09T10:00:00Z GIT_COMMITTER_DATE=2026-08-09T10:00:00Z
    git config user.name "Mira"
    git config user.email "mira@rootle.dev"
    cat >> src/render.rs <<'EOF'

pub fn default_frame() -> Frame {
    Frame::new()
}
EOF
    git commit -qam "fix(render): a default frame constructor"
    git checkout -qb feature/boxed-results
    export GIT_AUTHOR_DATE=2026-08-14T10:00:00Z GIT_COMMITTER_DATE=2026-08-14T10:00:00Z
    cat >> src/main.rs <<'EOF'

fn banner(view: &str) -> String {
    format!("── {view} ──")
}
EOF
    git commit -qam "feat: boxed result banner"
    git checkout -q main
    git tag v0.1.0
)
fi

cat > "$DEMO/provider.toml" <<EOF
[provider]
kind = "stdio"
command = ["python3", "$PWD/examples/providers/fs_provider.py", "$DEMO/code"]
EOF

cat >> "$DEMO/provider.toml" <<EOF
[ui]
# The demo renders with the vendored Nerd Font Mono — show the full
# powerline modeline (arrows + forge icons), not the unicode fallback.
nerd_font = true
EOF

# The tape launches the release binary. Releases ship as a tarball
# (rootle-<v>-<target>.tar.gz) — ALWAYS re-extract: a previously
# extracted copy goes stale against a fresh tarball (it did — the
# render silently showed a weeks-old build).
tarball="$(ls -t dist/rootle-*-x86_64-unknown-linux-musl.tar.gz 2>/dev/null | head -1)"
if [ -n "$tarball" ]; then
    tmp="$(mktemp -d)"
    tar -xzf "$tarball" -C "$tmp"
    install -m755 "$tmp"/*/rootle dist/rootle-linux-x86_64-musl
    rm -rf "$tmp"
else
    cargo build --quiet
    mkdir -p dist
    cp target/debug/rootle dist/rootle-linux-x86_64-musl
fi

echo "demo fixture ready: $DEMO"
