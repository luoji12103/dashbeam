#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/dashbeam-flash-drop.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT

xcrun clang \
  -fobjc-arc \
  -fmodules \
  -Wall \
  -Wextra \
  -Werror \
  -framework AppKit \
  "$SCRIPT_DIR/flash-drop-harness.m" \
  -o "$BUILD_DIR/flash-drop-harness"

"$BUILD_DIR/flash-drop-harness"
