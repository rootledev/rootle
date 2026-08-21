# Providers

rootle doesn't talk to GitHub directly — it talks to one seam (`trait
Provider`). The `github` backend ships in-tree; **anything else wraps in
as a child process speaking NDJSON-RPC over stdio** — the same model as
LSP. Your adapter can be any language; four methods make a minimal
useful provider.

![how rootle talks to backends: one seam, github in-tree, anything else as an NDJSON-RPC stdio child](architecture.svg)

## Point rootle at your backend

```toml
# ~/.config/rootle/config.toml
[provider]
kind = "stdio"
command = ["python3", "/path/to/fs_provider.py", "/path/to/code"]
```

Misconfiguration never blocks startup — a failed spawn or bad handshake
falls back to GitHub with a warning on the status line.

Try it against a directory of local repos with the reference adapter:

```
python3 examples/providers/fs_provider.py ~/code   # serves ~/code/* under "local"
```

## The shape of a provider

One JSON message per line on stdin/stdout. An `initialize` handshake,
then calls like `repo/tree` and `repo/blob`:

```
→ {"jsonrpc":"2.0","id":1,"method":"repo/tree","params":{"repo":"local/alpha"}}
← {"jsonrpc":"2.0","id":1,"result":{"entries":[…],"truncated":false,"branch":"main"}}
```

## Building one

- **Full wire spec** — every method, error kinds, cancellation:
  [doc/provider-protocol.md](https://github.com/tknawara/rootle/blob/main/doc/provider-protocol.md)
- **Reference adapter** (documentation-by-example):
  [examples/providers/fs_provider.py](https://github.com/tknawara/rootle/blob/main/examples/providers/fs_provider.py)
- **Scaffolding skill** — capability questionnaire, adapter skeleton,
  and a conformance test suite that gates integration:
  [skills/rootle-provider](https://github.com/tknawara/rootle/tree/main/skills/rootle-provider)

The e2e suite drives the full TUI through this protocol against the
reference adapter — offline proof of the whole path.
