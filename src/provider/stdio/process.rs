//! Child process spawn (plans/0005): argv in, piped NDJSON-RPC
//! transport out. stdin/stdout are always pipes; stderr follows the
//! configured policy (plans/0008 §4). rootle holds the handle for
//! Drop, so the child dies with the app (protocol v1.2 restart
//! obligations).

use crate::provider::{ProviderError, ProviderResult};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Child stderr policy (plans/0008 §4).
#[derive(Clone, Copy)]
pub(super) enum StderrMode {
    Null,
    Inherit,
}

pub(super) struct Process {
    pub(super) child: Child,
    pub(super) stdin: ChildStdin,
}

/// Spawn the child and split its pipes for the process/reader halves.
pub(super) fn spawn_process(
    command: &[String],
    env: &[(&str, &str)],
    stderr_mode: StderrMode,
) -> ProviderResult<(Process, ChildStdout)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| ProviderError::other("empty provider command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(match stderr_mode {
            StderrMode::Null => Stdio::null(),
            StderrMode::Inherit => Stdio::inherit(),
        });
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProviderError::other(format!("spawn {program}: {e}")))?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    Ok((Process { child, stdin }, stdout))
}
