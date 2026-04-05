#!/usr/bin/env python3
"""Small PTY probe for terminal startup/control-sequence inspection.

Launches a command under a pseudo-terminal, optionally sends extra query bytes,
captures startup output for a short duration, prints an escaped transcript, and
can save raw/escaped logs for later comparison.

Examples:
  python3 extra/compat/pty_probe.py -- nvim -u NONE -i NONE -n
  python3 extra/compat/pty_probe.py --duration 1.5 -- cmus
  python3 extra/compat/pty_probe.py --queries common --save-dir extra/compat/logs -- nvim -u NONE -i NONE -n
"""

from __future__ import annotations

import argparse
import os
import pty
import select
import shlex
import signal
import sys
import termios
import time
from pathlib import Path
from typing import Iterable, List, Tuple

QUERY_SETS = {
    "none": [],
    "common": [
        ("decrqm_cursor_keys", b"\x1b[?1$p"),
        ("decrqm_keypad", b"\x1b[?66$p"),
        ("decrqss_sgr", b"\x1bP$qm\x1b\\"),
        ("kitty_keyboard", b"\x1b[?u"),
        ("xterm_color_10", b"\x1b]10;?\x07"),
        ("xterm_color_11", b"\x1b]11;?\x07"),
    ],
}


def shell_escape(data: bytes) -> str:
    out: List[str] = []
    for b in data:
        if b == 0x1B:
            out.append(r"\x1b")
        elif b == 0x07:
            out.append(r"\a")
        elif b == 0x08:
            out.append(r"\b")
        elif b == 0x09:
            out.append(r"\t")
        elif b == 0x0A:
            out.append(r"\n\n")
        elif b == 0x0D:
            out.append(r"\r")
        elif 0x20 <= b <= 0x7E:
            out.append(chr(b))
        else:
            out.append(f"\\x{b:02x}")
    return "".join(out)


def set_winsize(fd: int, rows: int, cols: int) -> None:
    import fcntl
    import struct

    winsz = struct.pack("HHHH", rows, cols, 0, 0)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, winsz)


def capture(argv: List[str], duration: float, settle: float, rows: int, cols: int,
            queries: Iterable[Tuple[str, bytes]], send: Iterable[bytes]) -> Tuple[bytes, int]:
    pid, master_fd = pty.fork()
    if pid == 0:
        env = os.environ.copy()
        env.setdefault("TERM", env.get("TERM", "xterm-256color"))
        os.execvpe(argv[0], argv, env)

    set_winsize(master_fd, rows, cols)
    chunks: List[bytes] = []
    start = time.monotonic()
    sent_queries = False

    try:
        while True:
            now = time.monotonic()
            if not sent_queries and now - start >= settle:
                for _, payload in queries:
                    os.write(master_fd, payload)
                for payload in send:
                    os.write(master_fd, payload)
                sent_queries = True

            if now - start >= duration:
                break

            rlist, _, _ = select.select([master_fd], [], [], 0.05)
            if master_fd in rlist:
                try:
                    data = os.read(master_fd, 65536)
                except OSError:
                    break
                if not data:
                    break
                chunks.append(data)
    finally:
        for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGKILL):
            try:
                os.kill(pid, sig)
                time.sleep(0.05)
            except ProcessLookupError:
                break
        try:
            _, status = os.waitpid(pid, 0)
        except ChildProcessError:
            status = 0
        os.close(master_fd)

    return b"".join(chunks), status


def main() -> int:
    parser = argparse.ArgumentParser(description="Capture early PTY output/control sequences from TUIs.")
    parser.add_argument("--duration", type=float, default=1.0, help="Total capture time in seconds")
    parser.add_argument("--settle", type=float, default=0.15, help="Delay before sending queries/input")
    parser.add_argument("--rows", type=int, default=24)
    parser.add_argument("--cols", type=int, default=80)
    parser.add_argument("--queries", choices=sorted(QUERY_SETS), default="none", help="Built-in query set to send after settle delay")
    parser.add_argument("--send", action="append", default=[], help=r"Extra bytes to send, escaped like '\x1b[?1$p'")
    parser.add_argument("--save-dir", help="Directory for raw/escaped logs and metadata")
    parser.add_argument("cmd", nargs=argparse.REMAINDER, help="Command after '--'")
    args = parser.parse_args()

    argv = args.cmd
    if argv and argv[0] == "--":
        argv = argv[1:]
    if not argv:
        parser.error("expected command after '--'")

    extra_send = [bytes(s, "utf-8").decode("unicode_escape").encode("latin1") for s in args.send]
    query_items = QUERY_SETS[args.queries]
    raw, status = capture(argv, args.duration, args.settle, args.rows, args.cols, query_items, extra_send)
    escaped = shell_escape(raw)

    print(f"# cmd: {' '.join(shlex.quote(x) for x in argv)}")
    print(f"# duration: {args.duration}s settle: {args.settle}s size: {args.cols}x{args.rows}")
    print(f"# queries: {[name for name, _ in query_items]}")
    print(f"# bytes: {len(raw)} status: {status}")
    print(escaped, end="" if escaped.endswith("\n") else "\n")

    if args.save_dir:
        out_dir = Path(args.save_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        stem = f"probe_{int(time.time())}"
        (out_dir / f"{stem}.raw").write_bytes(raw)
        (out_dir / f"{stem}.escaped.txt").write_text(escaped)
        (out_dir / f"{stem}.meta.txt").write_text(
            "\n".join([
                f"cmd={' '.join(shlex.quote(x) for x in argv)}",
                f"duration={args.duration}",
                f"settle={args.settle}",
                f"rows={args.rows}",
                f"cols={args.cols}",
                f"queries={','.join(name for name, _ in query_items)}",
                f"bytes={len(raw)}",
                f"status={status}",
            ]) + "\n"
        )
        print(f"# saved: {out_dir / (stem + '.raw')}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
