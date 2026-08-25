//! The settings row model: what a row is (text / bool / radio) and
//! the section layout built from the working config. Rows carry
//! their own descriptions; rendering lives in `render.rs`.

use crate::config::Config;

#[derive(Debug, Clone)]
pub(super) enum Row {
    /// Free text; enter edits in place. `placeholder` renders dim while
    /// the value is empty — it says what the empty value resolves to.
    Text {
        key: &'static str,
        label: &'static str,
        value: String,
        placeholder: &'static str,
        desc: &'static str,
    },
    /// true/false; activating toggles the dot.
    Bool {
        key: &'static str,
        label: &'static str,
        value: bool,
        desc: &'static str,
    },
    /// One option of a radio group (`name` = themes, `kind` =
    /// providers): activating commits `option` for the group's key.
    /// The dot marks the group's current value (from the working
    /// config, so it tracks edits live).
    Radio {
        group: &'static str,
        option: String,
        desc: &'static str,
    },
}

impl Row {
    /// Footer description of the row under the cursor.
    pub(super) fn desc(&self) -> &'static str {
        match self {
            Row::Text { desc, .. } | Row::Bool { desc, .. } | Row::Radio { desc, .. } => desc,
        }
    }
}

/// One settings section: sidebar entry plus its rows.
pub(super) struct Section {
    pub(super) name: &'static str,
    pub(super) blurb: &'static str,
    pub(super) rows: Vec<Row>,
}

/// Build the section list from the working config: editor, theme
/// (embedded mocha first, then any discovered palettes), cache, and
/// the provider backend. `themes` are names found on disk.
pub(super) fn build(config: &Config, themes: Vec<String>) -> Vec<Section> {
    let mut names = vec!["catppuccin-mocha".to_string()];
    for t in themes {
        if !names.contains(&t) {
            names.push(t);
        }
    }
    let theme_rows = names
        .into_iter()
        .map(|option| Row::Radio {
            group: "name",
            option,
            desc: "palette: ~/.config/rootle/themes/<name>.toml — missing file falls back to embedded mocha",
        })
        .chain([Row::Text {
            key: "path",
            label: "path",
            value: config
                .theme
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            placeholder: "by name",
            desc: "explicit palette file; wins over name when set",
        }])
        .collect();

    vec![
        Section {
            name: "editor",
            blurb: "files",
            rows: vec![
                Row::Text {
                    key: "program",
                    label: "program",
                    value: config.editor.program.clone().unwrap_or_default(),
                    placeholder: "auto — $VISUAL · $EDITOR · hx…",
                    desc: "editor binary; empty → $VISUAL → $EDITOR → first of hx, nvim, vim, vi",
                },
                Row::Text {
                    key: "args",
                    label: "args",
                    value: config.editor.args.join(" "),
                    placeholder: "none",
                    desc: "extra arguments inserted before the file path",
                },
                Row::Bool {
                    key: "read_only",
                    label: "read_only",
                    value: config.editor.read_only,
                    desc: "vim family opens with -R; others edit the cache copy — rootle never writes back",
                },
            ],
        },
        Section {
            name: "theme",
            blurb: "colors",
            rows: theme_rows,
        },
        Section {
            name: "cache",
            blurb: "storage",
            rows: vec![Row::Text {
                key: "max_mb",
                label: "max_mb",
                value: config.cache.max_mb.to_string(),
                placeholder: "512",
                desc: "blob cache cap in MiB — least-recently-used blobs are evicted past it",
            }],
        },
        Section {
            name: "provider",
            blurb: "backend",
            rows: vec![
                Row::Radio {
                    group: "kind",
                    option: "github".into(),
                    desc: "built-in GitHub REST backend — applies after restart",
                },
                Row::Radio {
                    group: "kind",
                    option: "stdio".into(),
                    desc: "external child process speaking NDJSON-RPC — applies after restart",
                },
                Row::Text {
                    key: "command",
                    label: "command",
                    value: config.provider.command.join(" "),
                    placeholder: "required for stdio",
                    desc: "argv for stdio providers; element 0 is the executable — ignored by github",
                },
            ],
        },
    ]
}
