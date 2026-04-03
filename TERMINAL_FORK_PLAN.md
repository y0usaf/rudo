# Termvide Build Plan

**Project:** Fork Neovide into a terminal emulator  
**Date:** 2026-04-03  
**Status:** Reviewer-confirmed complete after iterative multi-agent review

---

## 0. Agent Workflow Used

### Shared planner prompt
> Produce a concrete implementation plan to fork Neovide into a terminal emulator. The plan must include architecture, milestones, module/file changes, dependencies, risks, validation, deliverables, and clear stop/go criteria for each phase.

### Shared reviewer prompt
> Review the implementation plan for completeness and execution readiness. Confirm whether it is complete enough for engineering work to begin. Check for missing architecture decisions, unclear scope, missing validation steps, unrealistic sequencing, unaddressed risks, and absent deliverables. Either (a) reject with precise gaps, or (b) approve with explicit sign-off.

### Agent roles used
- **Planner Agent** — drafts and refines the plan
- **Reviewer A** — architecture and module boundaries
- **Reviewer B** — terminal emulation correctness and platform support
- **Reviewer C** — delivery, sequencing, risk, and validation

The reviewers all used the **same reviewer prompt** above. Their role labels only indicate emphasis.

---

## 1. Iteration 1 — Initial Plan Draft

### Objective
Transform Neovide into a standalone cross-platform GPU-accelerated terminal emulator by replacing the Neovim RPC/bridge layer with a PTY + VT parser + terminal state core, while preserving the renderer, font system, windowing, clipboard, and most settings infrastructure.

### Initial architecture

```text
Current:
  Neovim <-> bridge <-> editor <-> renderer <-> window

Target:
  shell/child process <-> PTY <-> VT parser <-> terminal state <-> renderer <-> window
                                          ^                              |
                                          |                              v
                                      input encoder <---------------- keyboard/mouse
```

### Major workstreams
1. **Repository fork and rename**
2. **Core architecture replacement**
   - Remove Neovim-specific bridge/session logic
   - Add PTY abstraction
   - Add VT parser integration
   - Add terminal state model
3. **Renderer adaptation**
   - Reuse grid renderer and cursor renderer
   - Replace Neovim redraw-event-driven editor path with terminal-state-driven draw path
4. **Input and interaction**
   - Keyboard → terminal escape sequences
   - Mouse reporting
   - Clipboard paste/copy
5. **Terminal UX features**
   - Scrollback
   - Selection
   - Alternate screen
   - Resize handling
6. **Packaging and validation**
   - Linux/macOS/Windows support
   - Test matrix
   - Performance and correctness validation

### Initial phase plan

#### Phase 0 — Fork and framing
- Fork upstream repo
- Rename app strings, binary name, bundle metadata
- Freeze upstream hash used as base
- Create architecture docs and tracking issue board
- Decide MVP scope

#### Phase 1 — Minimal terminal bootstrap
- Introduce `pty/` module
- Introduce `terminal/` module
- Spawn shell attached to PTY
- Parse basic ANSI output
- Render a single primary screen buffer
- Send keyboard input to PTY
- Support resize propagation

#### Phase 2 — Correctness core
- Implement SGR attributes
- Implement cursor movement and line wrapping
- Implement scrolling and clear operations
- Add alternate screen buffer
- Add mouse reporting modes
- Handle UTF-8, wide chars, combining marks

#### Phase 3 — Usability
- Scrollback buffer
- Selection model
- Clipboard copy/paste
- Search and link detection
- Configurable font/theme/key options

#### Phase 4 — Platform and polish
- Windows ConPTY
- macOS packaging polish
- Linux Wayland/X11 validation
- Performance tuning
- Crash recovery and logging

### Initial file/module plan

#### Remove or retire
- `src/bridge/` — retire entirely

#### Create
- `src/pty/mod.rs`
- `src/pty/unix.rs`
- `src/pty/windows.rs`
- `src/terminal/mod.rs`
- `src/terminal/state.rs`
- `src/terminal/parser.rs`
- `src/terminal/input.rs`
- `src/terminal/scrollback.rs`
- `src/terminal/selection.rs`
- `src/app/` or equivalent glue layer

#### Adapt heavily
- `src/main.rs`
- `src/editor/mod.rs`
- `src/window/keyboard_manager.rs`
- `src/window/mouse_manager.rs`
- `src/renderer/*` (integration only, not rewrite)

### Initial dependency plan
- Remove: `nvim-rs`, `rmpv`
- Add: `vte`
- Consider: `portable-pty` or native per-platform PTY wrappers

### Initial success criteria
- Launch shell in window
- Type commands
- See colored output
- Resize works
- `vim` or `nvim` inside the terminal is usable

---

## 2. Iteration 1 — Reviewer Feedback

### Reviewer A — Architecture
**Rejected**. Missing:
1. Clear runtime ownership: who owns PTY, parser, terminal state, renderer integration
2. Whether `editor/` is adapted or replaced
3. Event model replacing `RedrawEvent` / `DrawCommand` flow
4. Explicit decision on how scrollback interacts with rendering
5. Boundary between platform PTY layer and terminal logic

### Reviewer B — Terminal correctness/platforms
**Rejected**. Missing:
1. Explicit terminal capability target (VT100/xterm subset, OSC support level)
2. Unicode policy for width, grapheme clustering, combining marks
3. Alternate screen, bracketed paste, focus reporting, mouse mode plan
4. Windows strategy details: ConPTY abstraction and parity expectations
5. Shell discovery and child process lifecycle rules

### Reviewer C — Delivery/risk/validation
**Rejected**. Missing:
1. Deliverables per phase
2. Test strategy per phase
3. Risks with mitigations per phase
4. Go/no-go criteria and fallback decisions
5. Definition of MVP vs post-MVP

**Outcome:** plan not complete. Refine.

---

## 3. Iteration 2 — Refined Plan

## 3.1 Product scope

### MVP
A single-window, single-session, local-shell terminal emulator with:
- PTY-backed shell session
- ANSI colors, bold/italic/underline
- Unicode text rendering with wide chars and combining marks handled correctly enough for modern CLI apps
- Resize handling
- Primary and alternate screen buffers
- Scrollback in primary screen
- Text selection and clipboard copy/paste
- Keyboard and mouse support sufficient for `bash`, `zsh`, `fish`, `vim`, `nvim`, `less`, `htop`, `tmux`
- Linux + macOS support first, Windows shortly after if ConPTY integration remains within plan

### Post-MVP
- Multiple tabs/splits
- Search UI
- URL hyperlink UX
- Image protocols/sixel/kitty graphics
- GPU effects tuning and themes
- session restore
- remote shell/session management

## 3.2 Target architecture

### Runtime ownership
- **Application layer** owns app lifecycle, settings, window creation, and the active terminal session
- **Terminal session** owns PTY process, async IO tasks, parser, terminal state, and outbound event channel to UI
- **Renderer path** reads a renderable snapshot of terminal state and converts it into draw commands or directly-updated render model
- **Window/input layer** owns keyboard, mouse, clipboard, IME, focus, resize events

### Recommended module design

```text
src/
  app/
    mod.rs              # application state and routing
    session.rs          # active terminal session orchestration
  pty/
    mod.rs              # trait + factory
    unix.rs             # forkpty/openpty backend
    windows.rs          # ConPTY backend
  terminal/
    mod.rs
    capabilities.rs     # feature support policy
    parser.rs           # vte integration and dispatch
    state.rs            # terminal model
    screen.rs           # primary/alternate screens
    cell.rs             # cell + style + width metadata
    cursor.rs           # terminal cursor state
    scrollback.rs       # scrollback ring / storage
    selection.rs        # selection model
    input.rs            # key/mouse/focus/paste encoding
    lifecycle.rs        # shell launch, environment, exit handling
  render/
    bridge.rs           # terminal state -> renderer model/draw commands
```

### Event model
Replace Neovide's `RedrawEvent -> Editor -> DrawCommand` pipeline with:

```text
PTY bytes -> parser events -> terminal state mutations -> render invalidation -> renderer update
```

Suggested event categories:
- `TerminalOutput(Vec<u8>)`
- `TerminalStateInvalidated(DirtyRegion | FullFrame)`
- `ChildExited(status)`
- `Bell`
- `TitleChanged(String)`
- `ClipboardRequest/Response` if needed later

### Keep/replace decision for `editor/`
- **Do not keep the current editor event processing model as the primary core.**
- Reuse concepts and some structs where helpful:
  - grid/cell storage patterns
  - style model
  - cursor rendering assets
- Build a **terminal-native state core** rather than forcing terminal semantics through Neovim redraw semantics.

### Renderer integration decision
Use a translation layer that maps terminal state into a renderable grid compatible with the current renderer. This minimizes renderer rewrites and isolates terminal logic from Skia/winit specifics.

### Scrollback design decision
- Primary screen owns visible viewport + scrollback storage
- Alternate screen has **no scrollback persistence** beyond its own active content
- Rendering always targets a viewport slice: either visible primary buffer or alternate buffer
- Selection spans visible primary viewport first; deferred support for selection into historical scrollback can be phase-gated if needed

## 3.3 Terminal capability target

### Initial compatibility target
Target a practical **xterm-compatible subset** sufficient for common CLI/TUI applications:
- C0 controls
- CSI cursor movement and erase ops
- SGR styles and colors
- DEC private modes required by common TUIs
- OSC 0/2 title updates
- OSC 7 optional after MVP
- Bracketed paste
- Focus in/out reporting
- Mouse reporting modes used by modern TUIs
- Alternate screen

### Explicitly deferred until post-MVP
- sixel
- kitty graphics protocol
- inline images
- advanced OSC integrations beyond title and practical shell support

## 3.4 Unicode/text policy

Adopt these rules:
- Parse input/output as UTF-8 byte streams, falling back safely on invalid sequences
- Store cells with:
  - displayed text/grapheme fragment
  - style attributes
  - width metadata: 0 / 1 / 2
  - continuation markers for wide cells
- Use Unicode width logic for East Asian wide/full-width codepoints
- Preserve combining marks with base character composition at the cell/grapheme layer
- Renderer continues using swash shaping for visual output
- Add fixture tests for combining marks, emoji, box drawing, powerline glyphs, ligatures off/on behavior

## 3.5 PTY and process lifecycle plan

### PTY abstraction
Define a cross-platform trait:
- spawn(command, env, cwd, size) -> session handle
- resize(cols, rows)
- write(bytes)
- read stream
- kill/terminate
- wait for exit

### Unix backend
- Prefer native implementation using existing `rustix` where practical
- If complexity rises, allow fallback to `portable-pty`
- Support login shell / configured shell resolution

### Windows backend
- Use ConPTY
- Hide backend details behind the same PTY trait
- Accept that parity may temporarily trail Unix during bring-up, but keep API identical

### Shell discovery rules
- Unix: configured shell > `$SHELL` > passwd shell > `/bin/sh`
- Windows: configured shell > `pwsh.exe` > `powershell.exe` > `cmd.exe`

### Child lifecycle rules
- On normal exit: show exit status and close or hold based on config
- On startup failure: surface error window and logs
- On crash/PTY break: terminate session cleanly and preserve logs

## 3.6 Input/output feature plan

### Keyboard encoding
Support:
- printable text
- enter, backspace, delete, tab, shift-tab
- arrows, home/end, page up/down
- function keys
- ctrl/alt/meta combinations as appropriate per xterm conventions
- IME commit path routed as text insertion

### Paste behavior
- normal paste
- bracketed paste if terminal mode enabled

### Mouse behavior
Support:
- click, drag, release
- wheel scrolling
- motion reporting when enabled
- default local selection behavior when reporting is off

### Focus behavior
- send focus in/out when enabled by terminal mode

## 3.7 Delivery plan by phase

### Phase 0 — Project setup and architecture freeze
**Goal:** establish a clean fork with agreed architecture and success criteria.

**Tasks**
- Fork repository into Termvide
- Rename product strings, binary, metadata, icons if desired
- Freeze upstream base commit
- Create architecture decision record (ADR) set:
  - PTY abstraction
  - renderer integration strategy
  - MVP scope
  - Windows support plan
- Remove direct Neovim branding from user-facing paths where needed
- Set up issue labels and milestones

**Deliverables**
- runnable renamed app shell
- ADR docs
- milestone board
- MVP checklist

**Validation**
- app builds on Linux/macOS
- no broken rename/package metadata

**Go/no-go**
- proceed only after architecture ADRs are approved

### Phase 1 — Session bootstrap and minimal rendering
**Goal:** get shell output on screen.

**Tasks**
- Add `pty/` abstraction and Unix backend
- Add `terminal/state` minimal model (screen grid, cursor, resize)
- Integrate `vte`
- Create render bridge from terminal state to renderer
- Route keyboard text and basic control keys to PTY
- Propagate window resize to PTY and state

**Deliverables**
- local shell launches
- command input works
- plain text and basic ANSI color output renders
- app title optionally updates from OSC title

**Validation**
- run `echo`, `ls --color`, `top`/`htop` basic output
- resize during active command
- smoke test shell exit/restart

**Go/no-go**
- do not proceed until shell IO, rendering, and resize are reliable

### Phase 2 — Correct terminal semantics
**Goal:** support real TUI applications.

**Tasks**
- implement erase, insert, delete, scroll regions
- implement alternate screen
- implement full SGR set needed for common TUIs
- implement bracketed paste
- implement focus reporting
- implement mouse reporting modes
- improve Unicode cell semantics
- add bell/title hooks

**Deliverables**
- `vim`/`nvim` usable inside terminal
- `less`, `htop`, `tmux` function correctly enough for daily use tests
- alternate screen transitions correct

**Validation**
- fixture replay tests for escape sequences
- interactive manual tests for TUIs
- Unicode rendering test sheet

**Go/no-go**
- do not proceed to UX polish until `vim` and `tmux` pass the manual checklist

### Phase 3 — Scrollback, selection, clipboard
**Goal:** make the terminal comfortable for everyday use.

**Tasks**
- add primary screen scrollback ring
- viewport navigation
- local text selection model
- clipboard copy/paste integration
- selection rendering overlays
- ensure selection + mouse reporting interact correctly

**Deliverables**
- selectable output
- copy/paste works
- scrollback works without corrupting live terminal state

**Validation**
- select/copy multiline text
- paste into shell and TUIs
- wheel scroll in primary buffer
- mouse-mode applications do not break local selection when reporting disabled

**Go/no-go**
- proceed only after selection and clipboard behavior are predictable

### Phase 4 — Cross-platform completion and hardening
**Goal:** production-worthy terminal core across supported OSes.

**Tasks**
- add Windows ConPTY backend
- test on Wayland/X11/macOS/Windows
- improve logging and startup diagnostics
- harden shutdown/restart behavior
- benchmark large-output workloads
- optimize dirty-region rendering and scroll performance

**Deliverables**
- tested builds across platforms
- perf notes and optimization results
- known limitations list
- release candidate checklist

**Validation**
- platform smoke matrix
- long-output benchmarks
- open/close/relaunch stability

**Go/no-go**
- release only after platform matrix passes agreed checklist

### Phase 5 — Post-MVP enhancements
**Goal:** differentiate and expand.

Possible tasks:
- tabs or splits
- search UI
- hyperlink UX
- shell integration enhancements
- configurable cursor/animation presets
- terminal themes package support

## 3.8 File-level implementation map

### Retire
- `src/bridge/` in full

### Keep mostly intact
- `src/renderer/`
- `src/renderer/fonts/`
- `src/renderer/cursor_renderer/`
- `src/window/mod.rs`
- `src/settings/`
- `src/clipboard.rs`
- `src/utils/`

### Adapt
- `src/main.rs` — boot app/session rather than Neovim bridge
- `src/window/keyboard_manager.rs` — encode xterm-style sequences instead of Neovim UI commands
- `src/window/mouse_manager.rs` — feed terminal mouse/focus/selection behavior
- `src/window/application.rs` — own terminal session lifecycle and redraw scheduling
- selected `src/editor/` pieces may be migrated into `src/terminal/` or deleted over time

### New modules to add
- `src/pty/*`
- `src/terminal/*`
- `src/render/bridge.rs`
- optionally `src/app/*`

## 3.9 Testing strategy

### Unit tests
- parser event handling
- cell width and combining behavior
- scroll region semantics
- alternate screen transitions
- input encoding for key combinations

### Fixture/replay tests
- record representative byte streams from:
  - shell prompt
  - `vim`
  - `tmux`
  - `htop`
  - color test scripts
- replay into parser/state and compare snapshots

### Integration tests
- spawn shell in PTY, send commands, assert screen content
- resize during output burst
- child exit and relaunch behavior

### Manual test checklist
- startup shell
- colored output
- resize
- paste
- selection/copy
- `vim`
- `tmux`
- `less`
- `git log --decorate --graph`
- Unicode sample file

## 3.10 Risk register

| Risk | Phase | Impact | Mitigation |
|------|-------|--------|------------|
| PTY backend complexity on Windows | 4 | High | keep strict PTY trait, bring up Unix first, isolate ConPTY backend |
| Forcing terminal semantics into Neovide editor path causes complexity | 1-2 | High | build terminal-native state core; use adapter for renderer only |
| Unicode correctness issues | 2 | Medium/High | explicit cell-width policy, fixture tests, incremental hardening |
| Scrollback performance degradation | 3-4 | Medium | ring buffer design, dirty-region rendering, benchmark early |
| Mouse selection conflicts with mouse reporting | 3 | Medium | formal interaction rules and manual matrix |
| renderer assumptions tied to Neovim multigrid | 1-2 | Medium | target single-grid terminal viewport first; use render bridge |
| upstream fork drift | all | Medium | freeze base, document divergence, rebase policy later |

## 3.11 Decision checkpoints

At each phase end, decide:
1. Does current architecture still reduce complexity?
2. Are we preserving renderer reuse as intended?
3. Is Windows still in-MVP or should it shift to immediately-post-MVP?
4. Do we need `portable-pty` to accelerate delivery?
5. Do we keep or delete more of `editor/`?

## 3.12 MVP definition of done

MVP is complete when all are true:
- app launches a local shell
- shell input/output stable
- ANSI colors/styles correct for common usage
- primary + alternate screens work
- `vim`/`nvim`, `less`, `htop`, `tmux` pass manual smoke checklist
- resize is reliable
- scrollback works in primary screen
- selection and clipboard copy/paste work
- Linux and macOS builds pass smoke tests
- known limitations documented

---

## 4. Iteration 2 — Reviewer Feedback

### Reviewer A — Architecture
**Approved with conditions.**
Plan is now architecturally coherent. Remaining ask:
- be explicit about whether the renderer consumes terminal state snapshots or queued draw commands
- define migration strategy for retiring `editor/`

### Reviewer B — Terminal correctness/platforms
**Approved with conditions.**
Coverage is sufficient to start. Remaining ask:
- clarify terminal width/grapheme policy as “good-enough MVP, harden later” to avoid overcommitting
- clarify Windows may slip behind Unix without blocking Unix MVP

### Reviewer C — Delivery/risk/validation
**Rejected, minor gaps.** Missing:
- work breakdown for the first 2 weeks
- ownership of benchmark and fixture creation
- rollback plan if render bridge proves awkward

**Outcome:** refine once more.

---

## 5. Iteration 3 — Final Refinement

## 5.1 Renderer consumption decision
Use **terminal state snapshots + dirty regions** as the primary integration contract.

Why:
- terminal state is the source of truth
- easier to test than ephemeral draw-command-only logic
- renderer can still internally emit/consume draw updates efficiently
- scrollback and selection are easier to reason about from a state snapshot

Contract:
- terminal session mutates state
- terminal state publishes dirty region(s)
- render bridge converts changed viewport cells/cursor/selection into renderer updates

## 5.2 `editor/` retirement strategy
- **Week 1-2:** leave current editor code untouched while new terminal pipeline is built in parallel
- **Week 3-4:** route application to terminal path by default behind a feature flag or branch-local switch
- **Week 5+:** delete unused Neovim/editor event plumbing in chunks once terminal rendering is stable
- Preserve reusable types only after they are migrated or copied into terminal-native modules

## 5.3 Unicode scope clarification
For MVP:
- support UTF-8 text
- handle common wide characters and combining marks correctly for mainstream CLI apps
- accept that rare edge-case grapheme behavior may be hardened post-MVP

This avoids blocking delivery on perfect Unicode conformance while still making the terminal broadly usable.

## 5.4 Windows scope clarification
- Linux + macOS are the primary bring-up targets
- Windows ConPTY remains planned in Phase 4
- Windows is **not required to block Unix MVP** if ConPTY integration expands unpredictably
- API boundaries must still be designed for Windows from day one

## 5.5 Two-week execution breakdown

### Week 1
- fork and rename repo
- add ADRs and milestone board
- stub `pty/` and `terminal/` modules
- integrate `vte` dependency
- define PTY trait and terminal state skeleton
- create render bridge skeleton

### Week 2
- implement Unix PTY backend
- launch shell and stream bytes
- wire parser into state mutations
- render plain text + basic colors
- propagate resize
- route keyboard printable text + enter/backspace/arrows

### End of week 2 demo target
- launch shell
- run `echo hello`
- run `ls --color`
- resize window during output

## 5.6 Ownership model
Even for a solo developer, assign explicit lanes:
- **Core lane:** PTY + parser + terminal state
- **Render lane:** render bridge + viewport/dirty updates
- **Validation lane:** fixtures, replay tests, benchmark scripts, manual checklist

Validation lane deliverables begin in Week 2, not after feature completion.

## 5.7 Rollback plan if render bridge is awkward
If adapting terminal state into the existing renderer proves too indirect:
1. keep font shaping and cursor rendering
2. bypass old draw-command batching for grid painting
3. implement a thinner terminal-specific render submission path directly against grid renderer

This preserves most of the renderer investment while avoiding architectural deadlock.

---

## 6. Final Reviewer Sign-off

### Reviewer A — Architecture
**Approved.**
The plan now defines source-of-truth ownership, renderer contract, module boundaries, migration strategy, and fallback path.

### Reviewer B — Terminal correctness/platforms
**Approved.**
The compatibility target is adequate, scope is realistic, Unicode policy is pragmatic, and Windows expectations are properly bounded.

### Reviewer C — Delivery/risk/validation
**Approved.**
The plan now has actionable sequencing, deliverables, validation, go/no-go gates, and rollback strategy. Engineering work can begin.

---

## 7. Final Confirmed Plan

### Recommended execution order
1. Phase 0 — architecture freeze and fork setup
2. Phase 1 — Unix shell bootstrap + parser + render bridge
3. Phase 2 — TUI correctness (`vim`, `tmux`, `htop`, `less`)
4. Phase 3 — scrollback + selection + clipboard
5. Phase 4 — Windows + hardening + perf
6. Phase 5 — post-MVP differentiation

### Immediate next actions
1. fork/rename repo
2. remove `nvim-rs` and bridge bootstrap from the startup path
3. add `vte`
4. create `pty/` and `terminal/` module skeletons
5. define PTY trait and terminal state snapshot contract
6. produce end-of-week-2 shell demo

### Final confidence
**High** — the plan is now reviewer-confirmed complete enough to execute.
