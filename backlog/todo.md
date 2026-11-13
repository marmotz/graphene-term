<!-- backlog-sync 2026-11-12T19:51Z — GENERATED, do not hand-edit. Regenerate: skill backlog-sync -->

# Backlog

## CI tests  ·  [overview](features/ci-tests/overview.md)

_terminé — voir [technical.md](features/ci-tests/technical.md)_ — 2/2 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [x]  | [#50](https://github.com/marmotz/graphene-term/issues/50) | CI: format the code base with rustfmt | — |
| [x]  | [#51](https://github.com/marmotz/graphene-term/issues/51) | CI: GitHub Actions workflow for pull requests | #50 |

## Command blocks  ·  [overview](features/command-blocks/overview.md)

_terminé — voir [technical.md](features/command-blocks/technical.md)_ — 11/11 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [x]  | [#6](https://github.com/marmotz/graphene-term/issues/6) | Command blocks: own PTY loop replacing alacritty's EventLoop | — |
| [x]  | [#7](https://github.com/marmotz/graphene-term/issues/7) | Command blocks: OSC 133 stream scanner | — |
| [x]  | [#8](https://github.com/marmotz/graphene-term/issues/8) | Command blocks: shell integration scripts and automatic injection | — |
| [x]  | [#9](https://github.com/marmotz/graphene-term/issues/9) | Command blocks: shell state, cell tags and row index | #6, #7 |
| [x]  | [#10](https://github.com/marmotz/graphene-term/issues/10) | Command blocks: gutter, view rows and block decorations | #9 |
| [x]  | [#11](https://github.com/marmotz/graphene-term/issues/11) | Command blocks: block selection, navigation and copy actions | #10 |
| [x]  | [#12](https://github.com/marmotz/graphene-term/issues/12) | Command blocks: folding, chevron, summary and auto-fold | #11 |
| [x]  | [#70](https://github.com/marmotz/graphene-term/issues/70) | Command blocks: rounded corners and vector shapes in the rect pipeline | — |
| [x]  | [#71](https://github.com/marmotz/graphene-term/issues/71) | Command blocks: card layout and view geometry with gaps | — |
| [x]  | [#72](https://github.com/marmotz/graphene-term/issues/72) | Command blocks: card frame and scene rendering | #70, #71 |
| [x]  | [#73](https://github.com/marmotz/graphene-term/issues/73) | Command blocks: selected block shown as a highlighted card | #72 |

## Configuration file and themes  ·  [overview](features/config-themes/overview.md)

_tâches créées, voir [technical.md](features/config-themes/technical.md)_ — 3/11 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [ ]  | [#13](https://github.com/marmotz/graphene-term/issues/13) | Config: centre the grid with the leftover pixels | — |
| [ ]  | [#16](https://github.com/marmotz/graphene-term/issues/16) | Config: configurable keybindings | #14 |
| [ ]  | [#17](https://github.com/marmotz/graphene-term/issues/17) | Config: cursor blinking | #14 |
| [ ]  | [#19](https://github.com/marmotz/graphene-term/issues/19) | Config: hot reload of the configuration and themes | #14, #15, #16 |
| [ ]  | [#20](https://github.com/marmotz/graphene-term/issues/20) | Config: follow the system light/dark appearance | #15, #19 |
| [ ]  | [#21](https://github.com/marmotz/graphene-term/issues/21) | Config: language, tab bar position and UI colors | #13, #15, #19, #35, #40, #42 |
| [ ]  | [#62](https://github.com/marmotz/graphene-term/issues/62) | Config: window startup mode (default, maximized, remember) | #14 |
| [ ]  | [#63](https://github.com/marmotz/graphene-term/issues/63) | Config: transparent background | #14 |
| [x]  | [#14](https://github.com/marmotz/graphene-term/issues/14) | Config: configuration file, model and startup settings | — |
| [x]  | [#15](https://github.com/marmotz/graphene-term/issues/15) | Config: themes, built-in themes and selection color | #14 |
| [x]  | [#18](https://github.com/marmotz/graphene-term/issues/18) | Config: error banner | #14, #15 |

## Images and clickable links  ·  [overview](features/images-links/overview.md)

_tâches créées, voir [technical.md](features/images-links/technical.md)_ — 0/8 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [ ]  | [#22](https://github.com/marmotz/graphene-term/issues/22) | Images: image sequence splitter in the PTY read path | #6 |
| [ ]  | [#23](https://github.com/marmotz/graphene-term/issues/23) | Images: image store, placeholder anchoring and rendering | #22 |
| [ ]  | [#24](https://github.com/marmotz/graphene-term/issues/24) | Links: OSC 8 and URL detection, hover and click to open | — |
| [ ]  | [#25](https://github.com/marmotz/graphene-term/issues/25) | Images: kitty graphics protocol, still images | #22, #23 |
| [ ]  | [#26](https://github.com/marmotz/graphene-term/issues/26) | Images: sixel | #22, #23 |
| [ ]  | [#28](https://github.com/marmotz/graphene-term/issues/28) | Links: tooltip with the OSC 8 target | #24 |
| [ ]  | [#30](https://github.com/marmotz/graphene-term/issues/30) | Images: kitty animations | #25 |
| [ ]  | [#31](https://github.com/marmotz/graphene-term/issues/31) | Images: copy text and image to the clipboard | #23, #25 |

## Mouse reporting  ·  [overview](features/mouse-reporting/overview.md)

_terminé, voir [technical.md](features/mouse-reporting/technical.md)_ — 4/4 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [x]  | [#45](https://github.com/marmotz/graphene-term/issues/45) | Mouse: pure encoding module for mouse, focus and alternate scroll | — |
| [x]  | [#46](https://github.com/marmotz/graphene-term/issues/46) | Mouse: report clicks, drag and motion to applications | #45 |
| [x]  | [#47](https://github.com/marmotz/graphene-term/issues/47) | Mouse: report the wheel, Shift + wheel scrolls the history | #45 |
| [x]  | [#48](https://github.com/marmotz/graphene-term/issues/48) | Mouse: arrow pointer while an application captures the mouse | — |

## Performance  ·  [overview](features/performance/overview.md)

_tâches créées, voir [technical.md](features/performance/technical.md)_ — 0/5 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [ ]  | [#64](https://github.com/marmotz/graphene-term/issues/64) | Performance: local benchmarks and startup timing logs | — |
| [ ]  | [#65](https://github.com/marmotz/graphene-term/issues/65) | Performance: glyph cache and atlas allocator | — |
| [ ]  | [#66](https://github.com/marmotz/graphene-term/issues/66) | Performance: glyph pipeline replaces glyphon | #65 |
| [ ]  | [#67](https://github.com/marmotz/graphene-term/issues/67) | Performance: spawn the shell before the GPU init | #65 |
| [ ]  | [#68](https://github.com/marmotz/graphene-term/issues/68) | Performance: try the Vulkan backend first | #64 |

## Rich input editor  ·  [overview](features/rich-input-editor/overview.md)

_tâches créées — voir [technical.md](features/rich-input-editor/technical.md)_ — 0/5 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [ ]  | [#27](https://github.com/marmotz/graphene-term/issues/27) | Rich input editor: shortcut table, private sequences and buffer distance | — |
| [ ]  | [#29](https://github.com/marmotz/graphene-term/issues/29) | Rich input editor: shell bindings for the private sequences | #8 |
| [ ]  | [#32](https://github.com/marmotz/graphene-term/issues/32) | Rich input editor: rich mode, editor moves and click to move the cursor | #27, #29, #9 |
| [ ]  | [#33](https://github.com/marmotz/graphene-term/issues/33) | Rich input editor: keyboard selection in the input | #32 |
| [ ]  | [#34](https://github.com/marmotz/graphene-term/issues/34) | Rich input editor: typing replaces the input selection | #32 |

## Selection and copy  ·  [overview](features/selection-copy/overview.md)

_terminé — voir [technical.md](features/selection-copy/technical.md)_ — 5/5 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [x]  | [#1](https://github.com/marmotz/graphene-term/issues/1) | Selection: pure helpers, semantic separators and pixel-to-cell geometry | — |
| [x]  | [#2](https://github.com/marmotz/graphene-term/issues/2) | Selection: highlight selected cells in the frame | — |
| [x]  | [#3](https://github.com/marmotz/graphene-term/issues/3) | Selection: mouse selection, copy, primary selection and middle-click paste | #1, #2 |
| [x]  | [#4](https://github.com/marmotz/graphene-term/issues/4) | Selection: auto-scroll while dragging past the grid edges | #3 |
| [x]  | [#5](https://github.com/marmotz/graphene-term/issues/5) | Selection: clear the selection only when its text really changes | #3 |

## Tabs, splits and command palette  ·  [overview](features/tabs-splits-palette/overview.md)

_tâches créées, voir [technical.md](features/tabs-splits-palette/technical.md)_ — 1/10 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [ ]  | [#36](https://github.com/marmotz/graphene-term/issues/36) | Tabs: pane identity in terminal events, shell pid, cwd and foreground program | — |
| [ ]  | [#37](https://github.com/marmotz/graphene-term/issues/37) | Tabs: grid layout inside a pane rectangle | — |
| [ ]  | [#38](https://github.com/marmotz/graphene-term/issues/38) | Tabs: binary split tree | #37 |
| [ ]  | [#39](https://github.com/marmotz/graphene-term/issues/39) | Tabs: Pane extracted from Session, workspace model and event routing | #36, #38 |
| [ ]  | [#40](https://github.com/marmotz/graphene-term/issues/40) | Tabs: multi-pane rendering, dimming, dividers and active border | #39 |
| [ ]  | [#41](https://github.com/marmotz/graphene-term/issues/41) | Tabs: split, focus, resize, zoom shortcuts and pane mouse handling | #40 |
| [ ]  | [#42](https://github.com/marmotz/graphene-term/issues/42) | Tabs: tab bar, UI text and tab shortcuts | #41 |
| [ ]  | [#43](https://github.com/marmotz/graphene-term/issues/43) | Tabs: close confirmation dialog | #35, #42 |
| [ ]  | [#44](https://github.com/marmotz/graphene-term/issues/44) | Tabs: command palette and docs | #35, #43 |
| [x]  | [#35](https://github.com/marmotz/graphene-term/issues/35) | Tabs: translation table and language detection | — |

## Update notification  ·  [overview](features/update-notification/overview.md)

_terminé, voir [technical.md](features/update-notification/technical.md)_ — 4/4 tasks done

| Done | Issue | Title | Blocked by |
|------|-------|-------|------------|
| [x]  | [#56](https://github.com/marmotz/graphene-term/issues/56) | Updates: build timestamp version and tag derived from the binary | — |
| [x]  | [#57](https://github.com/marmotz/graphene-term/issues/57) | Updates: settings, state file and GitHub release check | #56, #14 |
| [x]  | [#58](https://github.com/marmotz/graphene-term/issues/58) | Updates: background check thread and update event | #57 |
| [x]  | [#59](https://github.com/marmotz/graphene-term/issues/59) | Updates: banner with open, ignore and close actions | #58, #18, #35 |
