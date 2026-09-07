//! Resolve native log paths before startup and finalize every command outcome.

use crate::cli::{Cli, ProviderCommand};
use rootle_trace::{ContentPolicy, EventKind, TraceOptions, TraceSession};
use serde_json::json;
use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Session {
    trace: TraceSession,
    path: PathBuf,
}

impl Session {
    pub fn start(cli: &Cli) -> io::Result<Option<Self>> {
        let path = if let Some(path) = &cli.log_file {
            Some(path.clone())
        } else if let Some(selection) = &cli.log {
            Some(if selection == OsStr::new("ALL") {
                automatic_path()?
            } else {
                PathBuf::from(selection)
            })
        } else {
            std::env::var_os("ROOTLE_TRACE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let Some(path) = path else {
            if cli.log_content {
                return Err(io::Error::other(
                    "--log-content requires --log, --log-file or ROOTLE_TRACE",
                ));
            }
            return Ok(None);
        };
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        let content = if cli.log_content {
            ContentPolicy::Full
        } else {
            ContentPolicy::Metadata
        };
        let options = TraceOptions {
            content,
            ..TraceOptions::default()
        };
        let trace = rootle_trace::start(&path, options).map_err(io::Error::other)?;
        rootle_trace::record_with(EventKind::SessionStart, || {
            json!({
                "application": "rootle", "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(), "os": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
                "driver": driver(cli), "command": command(cli),
                "content_policy": if cli.log_content { "full" } else { "metadata" },
                "path": path.to_string_lossy(),
                "max_bytes": options.limits.max_bytes,
                "max_events": options.limits.max_events,
                "max_record_bytes": options.limits.max_record_bytes,
            })
        });
        // This happens before raw mode and never touches machine-readable stdout.
        eprintln!(
            "trace: {}{}",
            path.display(),
            if cli.log_content {
                " (sensitive content capture)"
            } else {
                ""
            }
        );
        Ok(Some(Self { trace, path }))
    }

    pub fn finish(self, result: io::Result<()>) -> io::Result<()> {
        rootle_trace::record_with(EventKind::SessionEnd, || {
            json!({
                "outcome": if result.is_ok() { "success" } else { "error" },
                "error_kind": result.as_ref().err().map(|error| format!("{:?}", error.kind())),
            })
        });
        let trace_result = self.trace.finish();
        match (result, trace_result) {
            (result, Ok(())) => result,
            (Ok(()), Err(error)) => Err(io::Error::other(format!(
                "trace {}: {error}",
                self.path.display()
            ))),
            (Err(primary), Err(trace)) => Err(io::Error::other(format!(
                "{primary}; trace {}: {trace}",
                self.path.display()
            ))),
        }
    }
}

fn automatic_path() -> io::Result<PathBuf> {
    let state = rootle_provider::paths::state_dir().ok_or_else(|| {
        io::Error::other("cannot resolve a state directory for --log; use --log-file PATH")
    })?;
    let directory = state.join("rootle").join("logs");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(directory.join(format!("session-{timestamp}-{}.jsonl", std::process::id())))
}

fn driver(cli: &Cli) -> &'static str {
    if cli.provider.is_some() {
        "provider"
    } else if cli.update {
        "update"
    } else if cli.headless.is_some() {
        "headless"
    } else {
        "terminal"
    }
}

fn command(cli: &Cli) -> &'static str {
    match cli.provider.as_ref() {
        Some(ProviderCommand::Install { .. }) => "install",
        Some(ProviderCommand::List { .. }) => "list",
        Some(ProviderCommand::Update { .. }) => "update",
        Some(ProviderCommand::Upgrade { .. }) => "upgrade",
        Some(ProviderCommand::Pin { .. }) => "pin",
        Some(ProviderCommand::Unpin { .. }) => "unpin",
        Some(ProviderCommand::Remove { .. }) => "remove",
        Some(ProviderCommand::Use { .. }) => "use",
        None if cli.update && cli.check => "update_check",
        None if cli.update => "self_update",
        None => "browse",
    }
}
