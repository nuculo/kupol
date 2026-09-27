#!/usr/bin/env bash
# Install the kupol wrapper to ~/.local/bin. Optionally copy the engine binary.
set -euo pipefail

PREFIX="${KUPOL_PREFIX:-$HOME/.local/bin}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

mkdir -p "$PREFIX"

if [ ! -f "$SCRIPT_DIR/bin/kupol" ]; then
  echo "install.sh: missing $SCRIPT_DIR/bin/kupol" >&2
  exit 1
fi

cp "$SCRIPT_DIR/bin/kupol" "$PREFIX/kupol"
chmod +x "$PREFIX/kupol"
echo "installed $PREFIX/kupol"

if [ -n "${1:-}" ]; then
  if [ ! -f "$1" ]; then
    echo "install.sh: engine path is not a file: $1" >&2
    exit 1
  fi
  cp "$1" "$PREFIX/duo-agents"
  chmod +x "$PREFIX/duo-agents"
  echo "installed $PREFIX/duo-agents"
fi

case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *)
    echo "add $PREFIX to PATH, for example:"
    echo "  export PATH=\"$PREFIX:\$PATH\""
    ;;
esac
