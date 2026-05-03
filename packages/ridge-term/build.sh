#!/usr/bin/env sh
# Thin shim → build.mjs (the actual cross-platform build script).
# Kept around so muscle-memory `./build.sh` still works on macOS/Linux.
exec node "$(dirname "$0")/build.mjs" "$@"
