//! Editor integration (PLAN.md §12): materialize the cached blob to a
//! real file, suspend the terminal, spawn the editor read-only, resume
//! with a full redraw — the one legitimate `terminal.clear()`.

use crate::config::Config;
use crate::provider::Provider;
use std::io;
use std::path::{Path, PathBuf};

/// A prepared editor invocation, ready for the main loop to run while
/// the terminal is suspended.
pub struct EditorJob {
    pub program: String,
    pub args: Vec<String>,
}

/// Resolution order: `[editor].program` → `$VISUAL` → `$EDITOR` →
/// probe `hx`, `nvim`, `vim`, `vi` on PATH.
pub fn resolve_program(config: &Config) -> Option<String> {
    if let Some(program) = &config.editor.program {
        return Some(program.clone());
    }
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    ["hx", "nvim", "vim", "vi"]
        .iter()
        .find(|p| find_in_path(p))
        .map(|p| p.to_string())
}

fn find_in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(program);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

/// Read-only args for the vim family; helix & co. have no such flag —
/// they edit the materialized cache copy, which is harmless (rootle never
/// writes back; PLAN.md §12 read-only decision).
pub fn build_args(program: &str, config: &Config) -> Vec<String> {
    let mut args = config.editor.args.clone();
    if config.editor.read_only {
        let base = program.rsplit('/').next().unwrap_or(program);
        if matches!(base, "vim" | "nvim" | "vi" | "view") {
            args.insert(0, "-R".into());
        }
    }
    args
}

/// Write the blob bytes to `~/.cache/rootle/edit/<owner>/<repo>/<sha>.<ext>`
/// — content-addressed like the rest of the cache, so the same path in
/// two branches/repos never collides and the editor still gets a real
/// extension for syntax detection. Traversal-safe: the sha must be hex
/// (network bytes never reach a path component).
pub fn materialize(
    owner: &str,
    repo: &str,
    sha: &str,
    rel_path: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bad content id",
        ));
    }
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("");
    let Some(root) = dirs::cache_dir().map(|d| d.join("rootle")) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no cache dir"));
    };
    let name = if ext.is_empty() {
        sha.to_string()
    } else {
        format!("{sha}.{ext}")
    };
    let file = root.join("edit").join(owner).join(repo).join(name);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, bytes)?;
    Ok(file)
}

/// Prepare an editor job: resolve the editor, get the bytes
/// (cache-first; blocking is fine — the UI is about to suspend), and
/// materialize the file.
pub fn prepare(
    config: &Config,
    provider: &dyn Provider,
    repo: &str,
    rel_path: &str,
    sha: &str,
) -> Result<EditorJob, String> {
    let program = resolve_program(config).ok_or_else(|| {
        "no editor found — set [editor].program in config.toml or $EDITOR".to_string()
    })?;
    let bytes = provider.fetch_blob(repo, sha).map_err(|e| e.to_string())?;
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| format!("bad repo id: {repo:?}"))?;
    let file = materialize(owner, name, sha, rel_path, &bytes).map_err(|e| e.to_string())?;
    let mut args = build_args(&program, config);
    args.push(file.to_string_lossy().into_owned());
    Ok(EditorJob { program, args })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(program: Option<&str>, read_only: bool) -> Config {
        Config {
            editor: crate::config::EditorConfig {
                program: program.map(str::to_string),
                args: vec![],
                read_only,
            },
            theme: crate::config::ThemeConfig {
                name: "catppuccin-mocha".into(),
                path: None,
            },
            cache: crate::config::CacheConfig { max_mb: 512 },
            provider: crate::config::ProviderConfig::default(),
            ui: crate::config::UiConfig::default(),
        }
    }

    #[test]
    fn vim_family_gets_read_only_flag() {
        let args = build_args("vim", &config(None, true));
        assert_eq!(args, vec!["-R"]);
        let args = build_args("/usr/bin/nvim", &config(None, true));
        assert_eq!(args, vec!["-R"]);
    }

    #[test]
    fn helix_has_no_read_only_flag() {
        let args = build_args("hx", &config(None, true));
        assert!(args.is_empty(), "helix has no -R; got {args:?}");
    }

    #[test]
    fn read_only_disabled_means_no_flag() {
        let args = build_args("vim", &config(None, false));
        assert!(args.is_empty());
    }

    #[test]
    fn config_args_come_before_read_only() {
        let mut cfg = config(Some("vim"), true);
        cfg.editor.args = vec!["--clean".into()];
        let args = build_args("vim", &cfg);
        assert_eq!(args, vec!["-R", "--clean"]);
    }

    #[test]
    fn materialize_rejects_bad_ids() {
        // Non-hex ids never reach a path component.
        for sha in ["", "abc-", "../escape", "/abs/x"] {
            let err = materialize("o", "r", sha, "lib.rs", b"x").unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "sha {sha:?}");
        }
    }

    #[test]
    fn materialize_writes_real_file() {
        if dirs::cache_dir().is_none() {
            return;
        }
        let path = materialize(
            "testowner",
            "testrepo",
            "cafe1234",
            "src/lib.rs",
            b"fn main() {}",
        )
        .unwrap();
        assert!(path.is_file());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn main() {}");
    }
}
