//! Clipboard (plans/0003 §1): OSC 52 first — works over SSH and tmux
//! with zero deps — plus a best-effort local tool fallback (wayland/X/
//! macOS). `GHX_CLIPBOARD=<path>` redirects to a file for e2e/CI
//! (no clipboard exists there).

use std::io::Write;

/// Copy `text`. Never fails the caller: every path is best-effort.
pub fn copy(text: &str) {
    if let Ok(path) = std::env::var("GHX_CLIPBOARD") {
        let _ = std::fs::write(path, text);
        return;
    }
    osc52(text);
    local_tool(text);
}

/// OSC 52: the terminal itself copies to the user's clipboard.
/// Writing this control sequence mid-frame is safe — it moves no
/// cursor and prints nothing.
fn osc52(text: &str) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{encoded}\x07");
    let _ = out.flush();
}

/// Wayland/X/macOS clipboard tools, when present. Errors are ignored —
/// OSC 52 already tried, and the toast told the user what was yanked.
fn local_tool(text: &str) {
    use std::process::{Command, Stdio};
    for (program, flag) in [
        ("wl-copy", None),
        ("xclip", Some("-selection")),
        ("xsel", Some("--clipboard")),
        ("pbcopy", None),
    ] {
        let mut cmd = Command::new(program);
        if let Some(flag) = flag {
            cmd.arg(flag);
            if program == "xclip" {
                cmd.arg("clipboard");
            }
        }
        if cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut child| {
                let _ = child
                    .stdin
                    .as_mut()
                    .expect("piped")
                    .write_all(text.as_bytes());
                child.wait()
            })
            .is_ok()
        {
            return; // first tool that runs wins
        }
    }
}
