#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
RUSTFLAGS="-Zsanitizer=realtime" cargo test -p audio --target aarch64-apple-darwin --target-dir "${CARGO_TARGET_DIR:-target}-rtsan" "$@"
