# Graphene Term

A modern, GPU-accelerated terminal emulator for Linux, written in Rust.
The binary is called `graphene`.

## Run

```bash
cargo run --release
```

Environment variables:

- `GRAPHENE_FONT`: font family (default: system monospace)
- `GRAPHENE_FONT_SIZE`: font size in points (default: 14)
- `RUST_LOG`: log level (default: `warn`)

## Shortcuts

| Keys                          | Action                                          |
|-------------------------------|-------------------------------------------------|
| Left drag                     | Select text (copied to the primary selection)   |
| Double / triple click         | Select a word / a whole line                    |
| Ctrl+left drag                | Block (rectangular) selection                   |
| Shift+left click              | Extend the selection                            |
| Shift+mouse                   | Select even when the app captures the mouse     |
| Ctrl+Shift+C                  | Copy the selection to the clipboard             |
| Middle click                  | Paste the primary selection                     |
| Ctrl+Shift+V, Shift+Insert    | Paste (bracketed when the app requests it)      |
| Shift+PageUp / Shift+PageDown | Scroll the history                              |
| Shift+Home / Shift+End        | Top / bottom of the history                     |
| Ctrl+= / Ctrl+- / Ctrl+0      | Font size up / down / reset                     |
| Mouse wheel                   | Scroll history (arrow keys in full-screen apps) |

## Documentation

- [Vision and roadmap](docs/vision.md)
- [Architecture](docs/architecture.md)
- [Technical decisions](docs/decisions.md)
- [Development guide](docs/development.md)
