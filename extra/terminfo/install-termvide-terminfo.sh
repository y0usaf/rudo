#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SRC="$SCRIPT_DIR/termvide.info"
DEST_DIR=${TERMINFO_DIRS_OVERRIDE:-${TERMINFO:-$HOME/.terminfo}}

if ! command -v tic >/dev/null 2>&1; then
  echo "error: tic not found; install ncurses/terminfo tools first" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
echo "Installing termvide terminfo to: $DEST_DIR"
tic -x -o "$DEST_DIR" "$SRC"
echo "Installed. Verify with: infocmp -A '$DEST_DIR' termvide"
echo "Opt in with: TERMVIDE_TERM=termvide"
