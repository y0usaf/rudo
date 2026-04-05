# Terminal wave 3 audit

Scope: direct PTY/TUI use outside Zellij, with emphasis on `nvim`, `cmus`, and ncurses/xterm-style expectations.

## Top remaining risks

1. **Keyboard protocol coverage is still narrow** — blocker
   - `src/window/keyboard_manager.rs` only emits classic xterm-style sequences for arrows, Home/End, Insert/Delete, PgUp/PgDn, F1-F12, Tab, Enter, Esc, Backspace, and some keypad cases.
   - Parser replies to kitty keyboard query (`CSI ? u`) with `CSI ? 0 u` in `src/terminal/parser.rs`, so modern extended keyboard reporting is explicitly absent.
   - No support found for `modifyOtherKeys`/CSI-u style input, so many Ctrl/Alt/Shift combinations on punctuation/non-ASCII keys will not match practical xterm/foot behavior. This is a real risk for Neovim mappings and some shell/TUI shortcuts outside Zellij.

2. **Mouse encoding omits modifier bits** — blocker for serious TUI testing with mouse
   - `src/terminal/input.rs` encodes press/drag/release/move/scroll but never adds Shift/Alt/Ctrl bits to the mouse button code.
   - `src/window/mouse_manager.rs` reads modifiers for hyperlink handling only; it does not pass modifiers into terminal mouse encoding.
   - xterm/foot-style TUIs may expect modified mouse events (Shift-click drag, Ctrl-click, Alt-wheel, etc.). Plain click/drag works better, but modifier-aware mouse behavior is currently incompatible.

3. **TERM/terminfo story is still mismatched by default** — blocker for broad external testing
   - PTY env defaults to `TERM=xterm-256color` in `src/pty/mod.rs`.
   - But implemented feature set is only a subset of practical xterm/foot behavior, and the bundled `termvide` terminfo is opt-in only (`docs/termvide-terminfo.md`).
   - This default overclaims compatibility for direct-TUI use: apps may assume more xterm features than Termvide actually supports. Safer for bring-up, but still a high-risk gap for user testing because failures will look app-specific and inconsistent.

4. **Device-attribute / terminal identity responses are synthetic and may mislead apps** — medium risk
   - `report_primary_device_attributes()` returns `CSI ? 62 ; c` and secondary DA returns `CSI > 1 ; 10 ; 0 c` in `src/terminal/state.rs`.
   - These identify as VT220-ish / generic terminal, while runtime defaults to `xterm-256color` and terminfo derives from xterm.
   - Many apps tolerate this, but some capability heuristics and probe scripts compare TERM, DA, and behavior together. The current identity mix is not xterm-like and can produce confusing fallback paths.

5. **Resize path does not send pixel dimensions to the PTY** — nice-to-have, but relevant for polish
   - `PtySize` supports `pixel_width`/`pixel_height`, but window resize sends `PtySize::new(...)`, leaving both as 0 in `src/window/window_wrapper.rs`.
   - Most ncurses/cmus flows only need rows/cols, so this is not a first-pass blocker, but it diverges from modern terminal expectations and can affect apps/features that use pixel-aware sizing.

## What looks good enough now

- Alternate screen `?47/?1047/?1049`, cursor save/restore `?1048`, origin mode, wrap, scroll regions, DSR/DA, DECRQM subset, DECRQSS subset, OSC 4/10/11/12, OSC 8, OSC 52, DEC special graphics, focus reporting, bracketed paste, and SGR mouse toggles are present in parser/state.
- Application cursor and application keypad mode are implemented in parser/state and input encoding.
- This is a decent baseline for basic launch/render/quit coverage in `nvim`, `cmus`, and many ncurses apps.

## Suggested test gate

### Blockers before broad user testing
- Keyboard extended-modifier gap
- Mouse modifier gap
- Default TERM/identity mismatch causing overclaiming vs actual support

### Acceptable for limited smoke testing now
- Basic `nvim -u NONE -i NONE -n`
- Basic `cmus`
- Simple ncurses apps using arrows, function keys, alternate screen, bracketed paste, focus, plain mouse

### Nice-to-have follow-up
- Pass pixel sizes on resize
- Reconcile terminal identity (`TERM`, DA, terminfo) so it is internally consistent
- Consider explicit docs for unsupported keyboard protocols and modified mouse behavior
