# Writing a rootle palette

Pick one of seventeen embedded palettes in `:settings` or with
`rootle --theme NAME`. **Hone** is the fresh-profile default; explicit
existing preferences are preserved. The six Strop originals are `hone`,
`slate`, `carbide`, `vellum`, `paper`, and `paper-dark`. The eleven
previous palettes remain available. See the
[settings page](https://rootle.dev/docs/settings.html) for regular use.

Palettes live at `~/.config/rootle/themes/<name>.toml` and **merge over
an embedded palette**: a file named `dracula.toml` forks the builtin
`dracula`; an unknown name starts from Hone. `[theme].path` in
config.toml points at an explicit file instead, and wins over `name`.

## File format

`[semantic]` and `[syntax]` role overrides, each a hex color
(`"#89b4fa"` or `"89b4fa"`). Unknown roles and bad hex are silently
ignored — a bad palette never crashes the app.

```toml
# ~/.config/rootle/themes/my-theme.toml
[semantic]
base = "#202020"
text = "#d0d0d0"
border_focused = "#ff8700"   # the dominant accent
selection_bg = "#303030"
selection_fg = "#ff8700"

[syntax]
keyword = "#ff79c6"
string = "#f1fa8c"
comment = "#6272a4"
```

## Roles

Every overridable `[semantic]` role, with its Hone default:

| Role | Default | Used for |
|---|---|---|
| `crust` | `#15171c` | text on accent chips, match-chip foreground |
| `mantle` | `#1c1f25` | popup/modeline surface |
| `base` | `#15171c` | pane background |
| `surface0` | `#23272f` | raised controls |
| `surface2` | `#4a5160` | scrollbar track |
| `overlay0` | `#858c9a` | muted placeholders |
| `subtext0` | `#a8adb8` | secondary text |
| `text` | `#e6e1d6` | body text |
| `border_focused` | `#e8a848` | focus accent |
| `border_unfocused` | `#4a5160` | inactive borders |
| `directory` | `#a8adb8` | directory context |
| `file` | `#e6e1d6` | file identity |
| `selection_bg` | `#30405a` | selected rows |
| `selection_fg` | `#e6e1d6` | selected-row text |
| `hint` | `#858c9a` | hint text |
| `error` | `#e8707c` | errors |
| `warning` | `#d9a85a` | status warnings |
| `mode_browse` | `#e8a848` | Browse chip |
| `mode_search` | `#6fb8c4` | Search/Find chip |
| `mode_insert` | `#8fc48a` | Insert chip |
| `mode_normal` | `#e8a848` | Normal chip |
| `mode_leader` | `#e8a848` | Leader chip |
| `mode_visual` | `#b79ce6` | Visual chip and marks |
| `forge` | `#23272f` | provider chip background |
| `forge_fg` | `#a8adb8` | provider chip text |
| `badge_repo` | `#e8a848` | repository badge |
| `badge_org` | `#a8adb8` | organization badge |
| `search_match` | `#e8a848` | match background; foreground is `crust` |
| `diff_add_fg` | `#8fc48a` | addition signs |
| `diff_del_fg` | `#e8707c` | deletion signs |
| `diff_add_bg` | `#1e2d22` | added-row wash |
| `diff_del_bg` | `#331f24` | deleted-row wash |
| `diff_add_strong` | `#2b4a33` | added-span emphasis |
| `diff_del_strong` | `#5a2c35` | deleted-span emphasis |
| `diff_band` | `#1c1f25` | structural diff rows |

The source tables live in `crates/rootle/src/theme/{palettes,strop}.rs`.
The Strop palettes preserve their base/surface/raised planes, selection
colors, dedicated mode inks and two-level diff washes. Legacy provider
chip text continues to follow `crust` unless `forge_fg` is specified.

## Syntax roles

`[syntax]` drives code highlighting in the preview pane. The Tree-sitter
capture mapping lives in `crates/rootle/src/highlight/styles.rs`; the roles:

| Role | Default | Used for |
|---|---|---|
| `keyword` | `#c99ce0` | keywords |
| `string` | `#aec37f` | strings |
| `comment` | `#838a97` | italic comments |
| `function` | `#86b7d8` | functions |
| `type` | `#8ecdb8` | types |
| `constant` | `#e8a848` | constants |
| `tag` | `#c99ce0` | markup tags |
| `namespace` | `#8ecdb8` | namespaces |
| `invalid` | `#e8707c` | invalid captures |
| `number` | `#e39a7b` | numbers |
| `attribute` | `#d1aa62` | annotations |
| `operator` | `#b4b8c2` | operators |
| `punctuation` | `#9da3af` | punctuation |
| `variable` | `#e6e1d6` | variables |
| `property` | `#d7cfc0` | properties |
| `heading` | `#e8a848` | markup headings |
| `link` | `#7fb2d4` | underlined links |
| `code` | `#aec37f` | uninjected markup code |

The last nine roles are optional in older palettes. When absent they
retain the original role mapping: number → constant, attribute →
function, heading → tag, link/code → string, and the remaining roles
→ semantic text. Explicit overrides always win.

Theme switches restyle already-fetched files instantly — blobs are
cached as raw text and re-highlighted under the new palette, no
refetch. The `:settings` popup previews the palette live (chrome and
code) and hot-reloads on save — iterate with the popup open.
