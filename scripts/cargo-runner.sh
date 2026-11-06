#!/usr/bin/env bash
# `cargo run` runner on macOS: signs the app binary with the stable local identity first, so
# a rebuilt dev build keeps keychain access. Test binaries (under deps/) run unsigned.
set -euo pipefail
bin="$1"
if [[ "$bin" != */deps/* ]]; then
  "$(dirname "$0")/sign.sh" "$bin" || true
fi
exec "$@"
