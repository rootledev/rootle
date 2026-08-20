# ghx demo tape (vhs). Render with: vhs doc/demo.tape → doc/demo.gif
# Requires the fs provider root at /tmp/ghx-demo — demo.py prepares the
# content; for a self-contained tape the Output/Setup below use the
# project's own repo as the served "code" directory.
Output demo.gif
Set FontSize 16
Set Width 1000
Set Height 700
Set Theme "Catppuccin Mocha"

Type "cat > /tmp/ghx-p.toml <<'EOF'"
Enter
Type "[provider]"
Enter
Type "kind = \"stdio\""
Enter
Type "command = [\"python3\", \"examples/providers/fs_provider.py\", \".\"]"
Enter
Type "EOF"
Enter
Type "./target/debug/ghx --config /tmp/ghx-p.toml"
Enter
Sleep 1200ms
Type "alpha"
Sleep 600ms
Enter
Sleep 800ms
Enter
Sleep 1200ms
Type "j"
Type "j"
Sleep 400ms
Type "l"
Sleep 800ms
Type "@-spacing"
Type " g"
Sleep 400ms
Type "render"
Sleep 400ms
Enter
Sleep 1500ms
Type "j"
Sleep 600ms
Escape
Escape
Sleep 400ms
Type "?"
Sleep 1200ms
Escape
Sleep 400ms
Type "q"
Sleep 600ms
