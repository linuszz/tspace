<h1 align="center">
  <br>
  tspace
  <br>
  <br>
</h1>

<p align="center">
  A <a href="https://github.com/zellij-org/zellij">zellij</a> fork focused on a simpler, menu-driven TUI experience — keyboard shortcuts preserved, mouse + command palette added.
</p>

---

# What is this?

**tspace** is a fork of [Zellij](#origin-of-the-name), a terminal workspace / multiplexer. This fork adds a **command palette** and **mouse-driven interaction layer** on top of zellij's existing keyboard-driven UX, making it accessible to users who prefer GUI-like menu interactions without sacrificing the power-user keyboard workflow.

## What's new in tspace

### Command Palette (`tspace-menu` plugin)

Press **`Ctrl+Shift+P`** anywhere (except locked mode) to open a fuzzy-search command palette — similar to VS Code's `Ctrl+Shift+P`.

| Feature | Details |
|---------|---------|
| **Fuzzy search** | SkimMatcherV2 with exact-match boost and keyword fallback |
| **33 commands** | Tab (11), Pane (11), Session (3), Mode (6), System (2) |
| **Keyboard navigation** | Arrows, vim keys (Ctrl+N/P/J/K), Tab, PageUp/Down, Home/End, Enter, Esc, Backspace, Ctrl+W/U |
| **Mouse interaction** | Hover to highlight, click to execute, scroll to navigate |
| **Visual feedback** | Matched-char highlighting, category badges, shortcut hints, reverse-video selection |
| **Architecture** | Pure plugin — zero core-logic modifications to zellij |
| **Rendering** | [ratatui](https://crates.io/crates/ratatui) 0.30 via custom ANSI `Backend` adapter |

The palette is implemented as a standard zellij WASM plugin at `default-plugins/tspace-menu/`. It renders via ratatui into a floating pane, intercepts keystrokes while open, and dispatches actions through zellij's plugin shim API.

## How do I build and run?

### Prerequisites

* Rust 1.92.0+ (with `wasm32-wasip1` target)
* `protoc` (Protocol Buffers compiler)
* `gcc` / `cc`
* `pkg-config`
* OpenSSL development libraries
* `make`

On NixOS, install everything via:
```bash
nix profile install nixpkgs#rustup nixpkgs#gcc nixpkgs#pkg-config nixpkgs#protobuf nixpkgs#openssl.dev nixpkgs#openssl.out nixpkgs#gnumake
rustup toolchain install 1.92.0 --target wasm32-wasip1
```

### Build & run

```bash
git clone https://github.com/linuszz/tspace.git
cd tspace
cargo xtask run
```

This compiles all plugins (including `tspace-menu`) to `wasm32-wasip1`, embeds them, and launches zellij.

Inside zellij, press **`Ctrl+Shift+P`** to open the command palette.

### Low-memory builds

If compilation is killed by OOM (common on systems with <16 GB RAM):

```bash
CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_OPT_DEBUG=line-tables-only cargo xtask run
```

### Running tests

```bash
cargo xtask test          # all tests
cargo test -p tspace-menu --target wasm32-wasip1  # plugin unit tests
```

## Project structure

```
tspace/
├── default-plugins/
│   └── tspace-menu/          # ← NEW: command palette plugin
│       ├── src/
│       │   ├── main.rs              # ZellijPlugin impl, event dispatch
│       │   ├── backend.rs           # ratatui Backend → ANSI escape adapter
│       │   ├── commands.rs          # 33-command registry + shim dispatch
│       │   ├── click.rs             # ClickRegion hit-testing for mouse
│       │   └── screens/
│       │       ├── mod.rs           # ActiveScreen enum
│       │       └── command_palette.rs  # Fuzzy filter, keyboard nav, rendering
│       └── docs/
│           ├── command-palette-design.md   # UI/UX interaction spec (1340 lines)
│           └── implementation-plan.md      # Technical implementation plan
├── zellij-client/           # Zellij client (unmodified)
├── zellij-server/           # Zellij server (1-line exclusion list addition)
├── zellij-utils/            # Shared utilities (registration + snapshots updated)
├── zellij-tile/             # Plugin SDK (unmodified)
└── xtask/                   # Build system (1 WorkspaceMember addition)
```

## Roadmap

### Done
- [x] Command palette MVP (fuzzy search + keyboard + mouse)
- [x] Ctrl+Shift+P global trigger
- [x] 33 zellij commands across 5 categories

### Planned
- [ ] **Frecency ranking** — recently/frequently used commands bubble up
- [ ] **Theme adaptation** — colors auto-adapt to user's zellij theme
- [ ] **Right-click context menu** — pane/tab/session-specific actions
- [ ] **Session/Tab/Pane manager panel** — visual tree browser
- [ ] **Top menu bar** — GUI-style File/Edit/View/Session dropdowns
- [ ] **Plugin configuration UI** — customize keybinds and palette options

## Relationship to upstream zellij

This fork tracks [zellij-org/zellij](https://github.com/zellij-org/zellij) `main`. All tspace changes are confined to:
- `default-plugins/tspace-menu/` (new crate, ~2300 LOC)
- Registration edits in 5 existing files (workspace, xtask, plugins.rs, consts.rs, default.kdl)
- 1-line addition in `session_layout_metadata.rs` (floating pane exclusion)
- Updated insta snapshots

No zellij core logic was modified. Upstream merges should be conflict-free outside of snapshot files.

## Origin of the Name
[From Wikipedia, the free encyclopedia](https://en.wikipedia.org/wiki/Zellij)

Zellij (Arabic: الزليج, romanized: zillīj; also spelled zillij or zellige) is a style of mosaic tilework made from individually hand-chiseled tile pieces. The pieces were typically of different colours and fitted together to form various patterns on the basis of tessellations, most notably elaborate Islamic geometric motifs such as radiating star patterns composed of various polygons. This form of Islamic art is one of the main characteristics of architecture in the western Islamic world. It is found in the architecture of Morocco, the architecture of Algeria, early Islamic sites in Tunisia, and in the historic monuments of al-Andalus (in the Iberian Peninsula).

## License

MIT
