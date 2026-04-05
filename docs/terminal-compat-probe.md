# Terminal compatibility probe notes

This repo now includes a lightweight PTY probe script for capturing a TUI's first burst of terminal I/O.

- Script: `extra/compat/pty_probe.py`
- Purpose: quickly inspect startup/control sequences from apps like `nvim -u NONE -i NONE -n` and `cmus`
- Output: escaped transcript on stdout, optional raw/escaped/meta log files

## Sequences to care about right now

These are the areas currently worth checking while terminal compatibility work is in flight:

- **ACS / charset switching**
  - Look for `ESC ( 0`, `ESC ( B`, `SO` (`0x0e`), and `SI` (`0x0f`)
  - TUIs may use these for line drawing / alternate character set behavior
  - Problems show up as wrong box-drawing glyphs or stray `q`, `x`, `m`, etc.

- **Application keypad mode**
  - Expect enable/disable sequences like `ESC =` and `ESC >`
  - Related mode queries can include DECRQM for `?66`
  - Problems show up as keypad keys sending the wrong sequences or apps not recognizing keypad input

- **DECRQM** (Request Mode)
  - Typical examples to query: cursor keys mode `CSI ? 1 $ p`, keypad mode `CSI ? 66 $ p`
  - Response is typically `CSI ? <mode> ; <value> $ y`
  - Useful to confirm whether the terminal replies at all and whether state matches expectations

- **DECRQSS** (Request Status String)
  - Example in the probe: `DCS $ q m ST` to ask for SGR state
  - Reply is usually a DCS status string response
  - Useful for checking parser support for DCS-based status queries

- **Kitty keyboard query**
  - Probe sends `CSI ? u`
  - Used to detect kitty keyboard protocol support / behavior
  - Important when debugging modern key reporting and modifier handling

- **Color query**
  - Probe sends OSC color queries for foreground/background:
    - `OSC 10 ; ? BEL`
    - `OSC 11 ; ? BEL`
  - Useful to verify OSC parsing and reply formatting

## Basic usage

Run a command under a PTY and print escaped startup output:

```bash
python3 extra/compat/pty_probe.py -- nvim -u NONE -i NONE -n
python3 extra/compat/pty_probe.py --duration 1.5 -- cmus
```

Capture startup output and send the built-in query set after a short delay:

```bash
python3 extra/compat/pty_probe.py \
  --duration 1.5 \
  --queries common \
  --save-dir extra/compat/logs \
  -- nvim -u NONE -i NONE -n
```

Send an additional custom sequence:

```bash
python3 extra/compat/pty_probe.py \
  --queries common \
  --send '\\x1b[>0c' \
  -- nvim -u NONE -i NONE -n
```

Notes:

- Use `--` before the command being probed.
- `--duration` controls total capture time.
- `--settle` controls when built-in queries or `--send` bytes are injected.
- `--save-dir` writes three files:
  - `*.raw` raw bytes
  - `*.escaped.txt` printable escaped transcript
  - `*.meta.txt` command and capture metadata

## What to look for in output

For quick manual review, check whether you see:

- enter/exit alternate screen: `CSI ? 1049 h` / `CSI ? 1049 l`
- bracketed paste toggles: `CSI ? 2004 h` / `CSI ? 2004 l`
- cursor visibility toggles: `CSI ? 25 l` / `CSI ? 25 h`
- keypad mode toggles: `ESC =` / `ESC >`
- charset selection: `ESC ( 0`, `ESC ( B`, `SO`, `SI`
- expected query replies instead of silence or malformed responses

## Short manual smoke checklist outside Zellij

Run these in a plain terminal window first, not inside Zellij/tmux/screen:

1. `nvim -u NONE -i NONE -n`
   - startup screen draws correctly
   - box/line glyphs are not corrupted
   - arrow keys and keypad behave normally
   - quitting restores screen/cursor state cleanly

2. `cmus`
   - interface borders render correctly
   - function keys / arrows / keypad are recognized
   - terminal state is restored after exit

3. Probe captures
   - compare `--queries none` vs `--queries common`
   - verify that DECRQM / DECRQSS / OSC queries produce plausible replies
   - note any missing replies, malformed DCS/OSC endings, or bad charset toggles

If results differ inside Zellij, save probe logs from both environments for comparison.
