# terminal wave 5 audit

## Verdict

**Yes, likely healthy enough for user smoke testing outside Zellij**, with one important qualifier: it looks healthy for **classic xterm-style TUI testing**, not for claiming broad modern terminal feature parity yet.

The project has meaningfully reduced the biggest direct-TUI risks from the earlier audit:

- **Mouse modifiers are now encoded** for press/release/drag/move/scroll paths in `src/terminal/input.rs`, and `src/window/mouse_manager.rs` passes current modifiers through.
- **TERM selection is safer**: `src/pty/mod.rs` now prefers `TERM=termvide` only when a compiled local `termvide` terminfo entry appears present, otherwise it falls back to `xterm-256color`.
- **Clipboard / hyperlink / sync-update plumbing is real end-to-end**:
  - OSC 8 hyperlinks are parsed into cells and can be opened from the UI.
  - OSC 52 set/query is parsed, routed through the window layer, and query replies are emitted back to the PTY.
  - synchronized updates (`?2026` and `DCS = 1/2 s`) are tracked and renderer flushes are deferred until updates end.

## Focus areas checked

### Keyboard protocol support

Much healthier than before, but still not full modern-terminal parity.

Observed behavior:

- `TerminalInputSettings` now tracks `kitty_keyboard_flags`.
- The terminal core supports a pragmatic first layer of kitty keyboard state:
  - `CSI ? u` query
  - `CSI > flags u`
  - `CSI < count u`
  - `CSI = flags ; mode u`
- Supported kitty flag support is intentionally conservative and currently limited to the first useful disambiguation bit.
- `src/window/keyboard_manager.rs` now uses kitty-style `CSI u` encoding for practical modified printable / ambiguous Ctrl cases when that disambiguation flag is active.
- Keyboard encoding still remains primarily **classic xterm-style** for navigation/function/special keys:
  - arrows/home/end/insert/delete/page/function keys with xterm modifier params
  - SS3 / application cursor / application keypad handling
  - Meta/Alt as ESC prefix for legacy text paths when kitty encoding is not active

Assessment:

- This is now good enough for broad smoke testing of shells, ncurses apps, vim/nvim basics, less, htop, etc.
- It is still **not** a full kitty keyboard implementation, and not a full `modifyOtherKeys` / rich modern-keyboard story.
- Remaining practical risk: some advanced Ctrl/Alt/Shift combinations on punctuation, symbols, layout-sensitive keys, and non-ASCII input may still mismatch mature terminals.

### Mouse modifiers

This area looks substantially improved.

Observed behavior:

- `mouse_modifier_bits()` now maps Shift/Alt/Ctrl into xterm mouse modifier bits.
- Encoding covers:
  - press
  - release
  - drag
  - motion
  - wheel
- `mouse_manager` now threads current modifiers into all relevant mouse encoders.
- SGR mouse mode (`?1006`) and mouse tracking modes (`1000/1002/1003`) are tracked in terminal state.

Assessment:

- This closes one of the major direct-TUI gaps from the earlier audit.
- For typical TUI mouse usage, this now looks good enough for smoke testing.
- Residual risk is lower than before and mostly around edge-case expectations in legacy non-SGR mouse mode rather than obvious missing modifier support.

### TERM selection / terminfo

This is in a much healthier state than before.

Observed behavior:

- `src/pty/mod.rs` TERM selection order:
  1. `TERMVIDE_TERM` override
  2. `TERM=termvide` if compiled terminfo entry is detected
  3. fallback `TERM=xterm-256color`
- Detection checks normal terminfo locations and both hashed / letter layouts.
- `docs/termvide-terminfo.md` is intentionally conservative and clearly documents that `termvide` does **not** claim kitty keyboard, modifyOtherKeys, sync-update, or richer clipboard semantics via terminfo.
- Bundled `extra/terminfo/termvide.info` remains xterm-derived but with a restrained set of explicit extras: OSC 52 set, cursor style, focus, bracketed paste, SGR mouse.

Assessment:

- This is a solid improvement because it avoids advertising `TERM=termvide` when the entry is absent.
- The remaining risk is that the fallback is still `xterm-256color`, which overclaims relative to current keyboard behavior. That is acceptable for bring-up, but still the main compatibility risk for external testers who do **not** install the `termvide` entry.

### Clipboard / hyperlink / synchronized update state

This area looks surprisingly complete for smoke testing.

Observed behavior:

- **OSC 52 clipboard**:
  - parser handles set and query
  - session drains clipboard requests
  - window layer writes clipboard contents or replies to query using OSC 52 back to the PTY
- **OSC 8 hyperlinks**:
  - parser tracks active hyperlinks into cells
  - render bridge preserves hyperlink metadata
  - mouse/UI can resolve and open hyperlinks
- **Synchronized updates**:
  - parser accepts `?2026 h/l` and `DCS = 1/2 s`
  - session suppresses draw flushes while active, then performs a full flush when sync mode ends

Assessment:

- Good enough for user smoke testing.
- The only caveat is semantic conservatism: terminfo correctly does **not** promise a broader clipboard/query contract than current implementation should safely advertise.

## Top remaining risks

1. **Keyboard compatibility is still the top risk**
   - Direct TUI smoke tests should work for normal keys and common modified special keys.
   - But applications or user mappings relying on kitty keyboard protocol, `modifyOtherKeys`, or rich modified punctuation/non-ASCII combos may still fail or behave inconsistently.

2. **Fallback `TERM=xterm-256color` still overclaims when `termvide` terminfo is not installed**
   - Better than before, because `TERM=termvide` is no longer blindly used.
   - But for external users who do not install the bundled terminfo, some failures may still present as "xterm-compatible app behaves oddly" rather than clearly signaling a narrower emulator contract.

3. **Test confidence is somewhat limited by environment here**
   - I could inspect code and unit coverage, but full `cargo test` did not complete in this environment because system link deps (`freetype`, `fontconfig`) were unavailable.
   - So the verdict is based on source inspection plus embedded tests, not a clean local test run.

## Bottom line

**Close enough for direct user smoke testing outside Zellij** as long as expectations are framed correctly:

- good target: shell workflows, vim/nvim basics, less, htop, git tools, ncurses apps, plain/modified mouse, bracketed paste, focus, hyperlinks, OSC 52 clipboard checks
- not yet a strong claim: cutting-edge keyboard protocol compatibility or full mature-xterm parity

If you want the lowest-risk external testing posture, encourage testers to install the bundled `termvide` terminfo first and treat keyboard edge cases as the main thing still under validation.
