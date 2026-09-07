//! Clipboard (plans/0003 §1): OSC 52 first — works over SSH and tmux
//! with zero deps — plus a best-effort local tool fallback (wayland/X/
//! macOS). `ROOTLE_CLIPBOARD=<path>` redirects to a file for e2e/CI
//! (no clipboard exists there).

use std::io::Write;

/// Copy `text`. Never fails the caller: every path is best-effort.
pub fn copy(text: &str) {
    if let Ok(path) = std::env::var("ROOTLE_CLIPBOARD") {
        let outcome = std::fs::write(path, text);
        rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
            serde_json::json!({"operation":"clipboard_file", "phase":"finished", "bytes":text.len(),
                "success":outcome.is_ok(),
                "error_kind":outcome.as_ref().err().map(|error|format!("{:?}",error.kind()))})
        });
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
    let written = write!(out, "\x1b]52;c;{encoded}\x07");
    let flushed = out.flush();
    rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
        serde_json::json!({"operation":"clipboard_osc52", "phase":"written", "bytes":text.len(),
            "success":written.is_ok() && flushed.is_ok(),
            "error_kind":written.as_ref().err().or_else(||flushed.as_ref().err()).map(|error|format!("{:?}",error.kind()))})
    });
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
        let outcome = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut child| {
                let written = child.stdin.as_mut().expect("piped").write_all(text.as_bytes());
                rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
                    serde_json::json!({"operation":"clipboard_tool", "program":program, "phase":"input",
                        "bytes":text.len(), "success":written.is_ok(),
                        "error_kind":written.as_ref().err().map(|error|format!("{:?}",error.kind()))})
                });
                child.wait()
            });
        rootle_trace::record_with(rootle_trace::EventKind::ExternalCommand, || {
            serde_json::json!({"operation":"clipboard_tool", "program":program, "phase":"finished",
                "success":outcome.as_ref().is_ok_and(|status|status.success()),
                "exit_code":outcome.as_ref().ok().and_then(|status|status.code()),
                "error_kind":outcome.as_ref().err().map(|error|format!("{:?}",error.kind()))})
        });
        if outcome.is_ok() {
            return; // first tool that runs wins
        }
    }
}
