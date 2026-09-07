//! Child process spawn (plans/0005): argv in, piped NDJSON-RPC
//! transport out. stdin/stdout are always pipes; stderr follows the
//! configured policy (plans/0008 §4). rootle holds the handle for
//! Drop, so the child dies with the app (protocol v1.2 restart
//! obligations).

use rootle_provider::{ProviderError, ProviderResult};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

/// Child stderr policy (plans/0008 §4).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StderrMode {
    Null,
    Inherit,
}

pub(super) struct Process {
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
}

impl Process {
    pub(super) fn terminate(&mut self) {
        crate::trace::lifecycle("terminate", |_| {});
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Spawn the child and split its pipes for the process/reader halves.
/// The third return is the child's stderr, present only when a
/// Full-content trace replaces the null sink with a capture pipe —
/// explicit inherit is never diverted (plans/0030).
pub(super) fn spawn_process(
    command: &[String],
    env: &[(&str, &str)],
    stderr_mode: StderrMode,
) -> ProviderResult<(Process, ChildStdout, Option<ChildStderr>)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| ProviderError::other("empty provider command"))?;
    let capture_stderr = stderr_mode == StderrMode::Null
        && rootle_trace::enabled()
        && rootle_trace::capture_content();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if capture_stderr {
            Stdio::piped()
        } else {
            match stderr_mode {
                StderrMode::Null => Stdio::null(),
                StderrMode::Inherit => Stdio::inherit(),
            }
        });
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProviderError::other(format!("spawn {program}: {e}")))?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = capture_stderr.then(|| child.stderr.take()).flatten();
    Ok((Process { child, stdin }, stdout, stderr))
}
