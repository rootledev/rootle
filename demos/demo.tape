# rootle demo tape — renders doc/demo.gif. Requires docker:
#   docker run --rm -v "$PWD:/vhs" -w /vhs ghcr.io/charmbracelet/vhs doc/demo.tape
# The app under test is ./dist/rootle-linux-x86_64-musl (CI builds it;
# locally, demo_setup.sh falls back to a debug build).
# the fs stdio provider serves a temp fixture built by demo_setup.sh.

Output doc/demo.gif

Set Shell "bash"
Set FontSize 18
# Font: vendored JetBrains Mono, installed into the container by the
# caller (demo workflow mounts /vhs; fc-cache picks it up).
Set FontFamily "JetBrains Mono"
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

# --- launch: repo search over the stdio provider -------------------
Type "alpha"
Sleep 600ms
Enter
Sleep 600ms
Enter
Sleep 1200ms

# --- browse: miller columns + highlighted preview ------------------
Type "j"
Sleep 400ms
Type "l"
Sleep 800ms
Type "J"
Sleep 500ms

# --- ␣ g grep: folded regions, match chips -------------------------
Space
Sleep 200ms
Type "g"
Sleep 600ms
Type "render"
Sleep 700ms
Enter
Sleep 1800ms
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms

# --- / filter over results -----------------------------------------
Type "/"
Sleep 300ms
Type "main"
Sleep 800ms
Escape
Sleep 500ms

# --- scope radio popup ---------------------------------------------
Tab
Sleep 300ms
Enter
Sleep 600ms
Type "j"
Sleep 600ms
Escape
Sleep 400ms
Escape
Sleep 400ms

# --- yank ----------------------------------------------------------
Space
Sleep 200ms
Type "y"
Sleep 900ms

# --- ? keybinds ----------------------------------------------------
Type "?"
Sleep 900ms
Type "jjjj"
Sleep 700ms
Escape
Sleep 400ms

# --- :settings -----------------------------------------------------
Type ":"
Sleep 300ms
Type "settings"
Sleep 500ms
Enter
Sleep 900ms
Tab
Sleep 400ms
Tab
Sleep 600ms
Escape
Sleep 400ms

# --- v visual marks ------------------------------------------------
Type "v"
Sleep 500ms
Space
Sleep 500ms
Type "j"
Sleep 300ms
Space
Sleep 600ms
Type "v"
Sleep 400ms

# --- :clone wizard -------------------------------------------------
Type ":"
Sleep 300ms
Type "clone"
Sleep 500ms
Enter
Sleep 900ms
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

# --- quit ----------------------------------------------------------
Type "q"
Sleep 800ms
