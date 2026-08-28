# rootle demo tape — renders demos/out/demo.gif (gitignored staging;
# the demo workflow publishes renders to the site repo). Requires
# docker: run it via the VHS image with the repo mounted at /vhs.
# The app under test is ./dist/rootle-linux-x86_64-musl (CI builds it;
# locally, demo_setup.sh falls back to a debug build).
# The fs stdio provider serves a temp fixture built by demo_setup.sh —
# alpha is a real git worktree (two branches, a tag, history), so the
# v1.5 revision lenses work on camera. The render step installs git
# into the VHS container (demo_setup.sh degrades to plain files
# without it).

Output demos/out/demo.gif

Set Shell "bash"
Set FontSize 18
# Font: the vendored JetBrainsMono Nerd Font Mono — family
# "JetBrainsMono NFM" (fc-match the exact name; plain "JetBrains Mono"
# resolves to the vhs base image's stock font, which ships the
# powerline arrows but no icon glyphs — the fs chip went tofu that
# way once). Installed into the container by the caller; fc-cache
# picks it up.
Set FontFamily "JetBrainsMono NFM"
Set Width 1600
Set Height 950
Set TypingSpeed 10ms
# The workflow's render matrix rewrites this per palette — VHS paints
# the window remainder with the tape theme's background, so it must
# match the app's --theme. Verify a name with `vhs themes`.
Set Theme "Catppuccin Mocha"

Type "bash demos/demo_setup.sh && clear" Enter
Sleep 600ms
Type "./dist/rootle-linux-x86_64-musl --config /tmp/rootle-demo/provider.toml" Enter
Sleep 1600ms
Set TypingSpeed 60ms

# --- find the repo ---------------------------------------------------
Type "alpha"
Sleep 600ms
Enter
Sleep 600ms
Enter
Sleep 1200ms

# --- browse: miller columns + highlighted preview --------------------
Type "l"
Sleep 900ms
Type "J"
Sleep 500ms

# --- ␣ b: switch the branch (k: default main sits under feature) ----
Space
Sleep 200ms
Type "b"
Sleep 1500ms
Type "k"
Sleep 700ms
Enter
Sleep 1600ms

# --- ␣ p: the preview owns the keyboard, zoomed ----------------------
# (the tree reload reset the selection — drill back onto main.rs)
Type "l"
Sleep 900ms
Space
Sleep 200ms
Type "p"
Sleep 900ms
Type "jjj"
Sleep 700ms
Type "G"
Sleep 600ms
Type "gg"
Sleep 700ms
Type "%"
Sleep 800ms

# --- ␣ p b: blame margin ---------------------------------------------
Type "b"
Sleep 1500ms

# --- Enter: history at the blamed commit ------------------------------
Enter
Sleep 1500ms
Type "j"
Sleep 600ms

# --- Enter: the file AT that commit; Esc restores --------------------
Enter
Sleep 1500ms
Escape
Sleep 900ms
Escape
Sleep 700ms

# --- ␣ g: grep with the grammar, boxed results -----------------------
Space
Sleep 200ms
Type "g"
Sleep 600ms
Type "render language:rust"
Sleep 900ms
Enter
Sleep 1800ms
Type "j"
Sleep 600ms

# --- Enter: the whole file at the match line --------------------------
Enter
Sleep 1800ms
Type "/"
Sleep 400ms
Type "draw"
Sleep 900ms
Escape
Sleep 600ms
Escape
Sleep 700ms

# --- yank the hit's permalink ----------------------------------------
Space
Sleep 200ms
Type "y"
Sleep 1000ms
Escape
Sleep 600ms

# --- ? keybinds -------------------------------------------------------
Type "?"
Sleep 900ms
Type "jjjj"
Sleep 700ms
Escape
Sleep 400ms

# --- and when you need it locally: the clone wizard -------------------
# (back at the repos level so the mark lands on the repo, not a file)
Escape
Sleep 600ms
Type "h"
Sleep 400ms
Type "h"
Sleep 400ms
Type "v"
Sleep 500ms
Space
Sleep 500ms
Type "v"
Sleep 400ms
Type ":"
Sleep 300ms
Type "clone"
Sleep 500ms
Enter
Sleep 1000ms
Tab
Sleep 300ms
Enter
Sleep 800ms
Tab
Sleep 300ms
Enter
Sleep 1200ms
Escape
Sleep 500ms

# --- quit -------------------------------------------------------------
Type "q"
Sleep 800ms
