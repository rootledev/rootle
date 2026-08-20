//! Command registry (plans/0003 §3): the `:` command line's option
//! list is derived from this table — one source of truth, like the
//! keymap tables. Adding a command = one row + one `RunCommand` arm.

/// A `:` command.
pub struct Command {
    pub name: &'static str,
    pub summary: &'static str,
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "settings",
        summary: "open the settings popup",
    },
    Command {
        name: "clone",
        summary: "clone the selected repos (VISUAL marks)",
    },
];

/// Commands whose name contains `needle` (case-insensitive prefix
/// first, then substring — the usual command-line feel).
pub fn filter(needle: &str) -> Vec<&'static Command> {
    let needle = needle.to_lowercase();
    let mut prefix = Vec::new();
    let mut substr = Vec::new();
    for cmd in COMMANDS {
        let name = cmd.name.to_lowercase();
        if needle.is_empty() || name.starts_with(&needle) {
            prefix.push(cmd);
        } else if name.contains(&needle) {
            substr.push(cmd);
        }
    }
    prefix.extend(substr);
    prefix
}

#[cfg(test)]
mod tests {
    #[test]
    fn filters_prefix_first() {
        assert_eq!(super::filter("set")[0].name, "settings");
        assert_eq!(super::filter("clone")[0].name, "clone");
        assert_eq!(super::filter("").len(), super::COMMANDS.len());
        assert!(super::filter("zzz").is_empty());
    }
}
