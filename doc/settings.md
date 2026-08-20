# Settings reference

Every ghx setting: key, acceptable values, meaning, default. Config
lives at `~/.config/ghx/config.toml` (`$GHX_CONFIG` does not apply —
use `ghx --config PATH` for an alternate file). Missing keys fall
back to defaults; a malformed file never blocks startup (defaults are
used silently). The `:settings` popup edits these in place and writes the same file —
hot-reloads the theme on save. Text fields edit with `Enter`; booleans
and lists (themes, provider kind) toggle/cycle with `Space`. Provider
changes save too but apply after restart.

```toml
[editor]
program = "hx"          # string, optional
args = []               # list of strings
read_only = true        # boolean

[theme]
name = "catppuccin-mocha"   # string
# path = "/abs/or/~/theme.toml"   # string, optional — overrides name

[cache]
max_mb = 512            # integer

[provider]
kind = "github"         # "github" | "stdio"
command = []            # list of strings (kind = "stdio")
```

## `[editor]` — how files open (Enter on a file)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `program` | string, optional | unset | Editor binary. Unset → `$VISUAL` → `$EDITOR` → first of `hx`, `nvim`, `vim`, `vi` on PATH. |
| `args` | list of strings | `[]` | Extra arguments inserted before the file path. |
| `read_only` | boolean | `true` | With `true`, the vim family (`vim`, `nvim`, `vi`, `view`) opens with `-R`. Editors without a read-only flag (e.g. helix) edit the cache copy — ghx never writes back either way. |

Files open from `~/.cache/ghx/edit/<owner>__<repo>/<path>`; ghx
suspends the terminal while the editor runs and fully redraws on
return.

## `[theme]` — colors

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | `"catppuccin-mocha"` | Palette to load. Resolution: `~/.config/ghx/themes/<name>.toml` → `<config dir>/themes/<name>.toml` → embedded Catppuccin Mocha. Unknown name → embedded fallback. |
| `path` | string, optional | unset | Explicit palette file; wins over `name`. |

`--theme NAME` (CLI) overrides `name` for one session.

### Palette files (`themes/<name>.toml`)

Only `[semantic]` role overrides, each a hex color (`"#89b4fa"` or
`"89b4fa"`). Unknown roles and bad hex are silently ignored — bad
palettes never crash the app. Roles (Catppuccin Mocha defaults):

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
| `border_focused` | `#89b4fa` | focused borders — the dominant accent (blue) |
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

Syntax highlighting maps syntect scopes onto the active palette — a
palette change recolors previews automatically.

## `[cache]` — the GitHub provider's content store

| Key | Type | Default | Meaning |
|---|---|---|---|
| `max_mb` | integer | `512` | Blob cache cap in MiB. Least-recently-used blobs are evicted past it at startup; orphaned trees/blobs are swept. |

Blobs/trees are content-addressed and immutable (never invalidated,
only evicted); repo refs revalidate via ETag (a `304` is free).
Deleting `~/.cache/ghx` is always safe. This section is
GitHub-provider-internal — stdio providers manage their own caching.

## `[provider]` — backend selection

| Key | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `"github"` \| `"stdio"` | `"github"` | `github` = the built-in provider. `stdio` = external child process speaking NDJSON-RPC ([provider-protocol.md](provider-protocol.md)). |
| `command` | list of strings | `[]` | argv for `kind = "stdio"`; element 0 is the executable, the rest its arguments. Ignored for `github`. |

Invalid/misfiring stdio configuration falls back to `github` with a
warning in the status line — a provider misconfiguration never blocks
startup. Scaffolding a provider:
[skills/ghx-provider](../skills/ghx-provider/SKILL.md).

## Environment variables

| Variable | Meaning |
|---|---|
| `GHX_TOKEN`, `GITHUB_TOKEN` | GitHub token (GitHub provider only; `gh auth token` is tried after these). Code search requires a token. |
| `VISUAL`, `EDITOR` | Editor fallbacks when `[editor].program` is unset. |
| `GHX_CLIPBOARD` | Path to a file — yanks (`␣ y`) write there instead of the clipboard (scripts/CI). |
| `GHX_TRACE` | Path to a log file — worker request tracing (debugging). |
| `NO_COLOR` | **Ignored** — a full-screen TUI's colors are semantic, like vim/helix. |

## Command line

```
ghx                    # launch (search popup only on fresh state)
ghx owner/repo         # skip the popup, open a repo
ghx --config PATH      # alternate config file
ghx --theme NAME       # override [theme].name for this session
ghx --version | -V
```
