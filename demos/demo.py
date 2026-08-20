"""Demo capture: drives ghx through the golden path over the fs stdio
provider and records an asciinema v2 cast to dist/demo.cast.

    cd demos && uv run --project ../e2e python demo.py   # writes demos/demo.cast
    # replay: asciinema play demo.cast
    # GIF for the README (doc/demo.gif):
    # (cast rendering with agg collapses full-screen diffs — use the
    # VHS tape instead; see doc/demo.tape / demos/demo.tape header.)

A VHS tape (doc/demo.tape) renders the same flow to GIF/MP4 later —
`vhs doc/demo.tape` — without needing a live environment.
"""

import tempfile
import time
from pathlib import Path

from conftest import FS_PROVIDER, make_fs_root
from tui import ROOT, Tui, build

# dist/ is owned by the docker release job in CI sandboxes —
# the cast lands next to the harness instead.
DIST = ROOT / "e2e"


def main() -> None:
    DIST.mkdir(exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="ghx-demo-"))
    root = make_fs_root(tmp)
    # A little more content so previews look real.
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
    tui.record()
    try:
        # 1. Launch → search popup; find the repo.
        tui.expect("search github")
        time.sleep(0.4)
        tui.type_query("alpha", settle=0.15)
        tui.key("ENTER")
        tui.expect("local/alpha")
        time.sleep(0.4)

        # 2. Open it: miller columns + preview.
        tui.key("ENTER")
        tui.expect("render.rs")
        time.sleep(0.6)
        tui.send("j")
        time.sleep(0.4)
        tui.send("j")
        time.sleep(0.4)
        tui.send("l")  # into src/, preview follows
        time.sleep(0.6)

        # 3. Global grep: match chips + folded regions.
        tui.send(" ")
        tui.send("g")
        tui.expect("repo:local/alpha")
        time.sleep(0.3)
        tui.type_query("render", settle=0.15)
        tui.key("ENTER")
        tui.expect("matches")
        time.sleep(1.0)
        tui.send("j")
        time.sleep(0.5)
        tui.key("ESC")
        tui.key("ESC")

        # 4. Keybinds popup.
        tui.send("?")
        tui.expect("keybindings")
        time.sleep(0.8)
        tui.key("ESC")

        # 5. Quit.
        time.sleep(0.3)
        tui.send("q")
    finally:
        tui.save_recording(DIST / "demo.cast")
        tui.stop()
    print(f"wrote {DIST / 'demo.cast'}")


if __name__ == "__main__":
    main()
