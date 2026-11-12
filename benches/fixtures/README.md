# Benchmark fixtures

Raw PTY output fed to the benchmarks of `benches/render.rs`, recorded at 120 columns by
32 lines with `TERM=xterm-256color` by [record.py](record.py)
(`record.py OUTPUT SECONDS COMMAND...`), from the repository root:

```bash
python3 benches/fixtures/record.py benches/fixtures/btop.raw 3 btop
python3 benches/fixtures/record.py benches/fixtures/cat.raw 1 cat src/config.rs src/app.rs src/selection.rs src/theme.rs
python3 benches/fixtures/record.py benches/fixtures/vim.raw 2 vim -u NONE -N -i NONE -c 'syntax on' -c 'set number' src/app.rs
```

| File       | Content                                                         |
|------------|-----------------------------------------------------------------|
| `btop.raw` | btop 1.4 for 3 s: frame, then the first data refresh            |
| `cat.raw`  | ~2600 lines of source code; also used scrolled 500 lines back   |
| `vim.raw`  | vim with syntax highlighting and line numbers on `src/app.rs`   |

Re-recording changes the benchmark numbers: save a criterion baseline before and after
with the same fixtures when comparing code changes.
