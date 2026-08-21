#!/bin/sh
# Demo fixture for doc/demo.tape: a two-repo "code root" served by the
# fs stdio provider, plus the provider config. Idempotent.
set -eu

DEMO=/tmp/rootle-demo
rm -rf "$DEMO"
mkdir -p "$DEMO/code/alpha/src" "$DEMO/code/beta"

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
cat > "$DEMO/code/alpha/README.md" <<'EOF'
# alpha
render docs
EOF
cat > "$DEMO/code/beta/notes.txt" <<'EOF'
nothing to see
EOF

cat > "$DEMO/provider.toml" <<EOF
[provider]
kind = "stdio"
command = ["python3", "$PWD/examples/providers/fs_provider.py", "$DEMO/code"]
EOF

# The tape launches the release binary; fall back to a debug build.
if [ ! -x dist/rootle-linux-x86_64-musl ]; then
    cargo build --quiet
    mkdir -p dist
    cp target/debug/rootle dist/rootle-linux-x86_64-musl
fi

echo "demo fixture ready: $DEMO"
