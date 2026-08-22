# Writing a rootle palette

For regular use, pick one of the eleven embedded palettes (seven dark, four light) in `:settings`
or via `rootle --theme NAME` — see [settings.md](settings.md). This
page is for authoring your own.

Palettes live at `~/.config/rootle/themes/<name>.toml` and **merge over
an embedded palette**: a file named `dracula.toml` forks the builtin
`dracula`; any other name forks Catppuccin Mocha. `[theme].path` in
config.toml points at an explicit file instead, and wins over `name`.

## File format

Only `[semantic]` role overrides, each a hex color (`"#89b4fa"` or
`"89b4fa"`). Unknown roles and bad hex are silently ignored — a bad
palette never crashes the app.

```toml
# ~/.config/rootle/themes/my-theme.toml
[semantic]
base = "#202020"
text = "#d0d0d0"
border_focused = "#ff8700"   # the dominant accent
selection_bg = "#303030"
selection_fg = "#ff8700"
```

## Roles

Every overridable role, with its Catppuccin Mocha default:

| Role | Default | Used for |
|---|---|---|
| `crust` | `#11111b` | text on accent chips, match-chip foreground |
| `mantle` | `#181825` | popup/modeline background |
| `base` | `#1e1e2e` | pane background, unthemed cells |
| `surface0` | `#313244` | selection background, idle buttons |
| `surface2` | `#585b70` | unfocused borders |
| `overlay0` | `#6c7086` | empty-preview placeholder text |
| `subtext0` | `#a6adc8` | secondary text: hints, line numbers, disabled radio items |
| `text` | `#cdd6f4` | body text, file names |
| `border_focused` | `#89b4fa` | focused borders — the dominant accent |
| `border_unfocused` | `#585b70` | unfocused field/pane borders |
| `directory` | `#89b4fa` | directories (bold) and dir previews |
| `file` | `#cdd6f4` | file entries |
| `selection_bg` | `#313244` | selected-row background |
| `selection_fg` | `#89b4fa` | selected-row text |
| `hint` | `#a6adc8` | hint rows in borders/modeline |
| `error` | `#f38ba8` | (reserved) error accents |
| `warning` | `#f9e2af` | status-line messages |
| `mode_browse` | `#a6e3a1` | `[BROWSE]` chip |
| `mode_search` | `#f9e2af` | `[SEARCH]` chip |
| `mode_insert` | `#94e2d5` | `[INSERT]` chip |
| `mode_normal` | `#89b4fa` | `[NORMAL]` chip |
| `mode_leader` | `#fab387` | `[LEADER]` chip, `:` prompt |
| `mode_visual` | `#f5c2e7` | `[VISUAL]` chip, marked-entry dot `●` |
| `badge_repo` | `#89b4fa` | `[repo]` badge in search results |
| `badge_org` | `#fab387` | `[org]` badge in search results |
| `search_match` | `#f9e2af` | grep match chips (crust text on top) |

The embedded palettes are the reference material: each is a complete
role assignment in `src/theme.rs` (`DRACULA`, `GRUVBOX_DARK`, …), built
from its palette's published spec.

Syntax highlighting maps syntect scopes onto the active palette, so
previews recolor automatically with any role change. The `:settings`
popup hot-reloads the theme on save — iterate with the popup open.
