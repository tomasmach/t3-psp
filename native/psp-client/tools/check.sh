#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p target
cargo fmt -- --check
rustc +stable --edition 2024 --test tools/preview.rs -o target/gui-tests
target/gui-tests
rustc +stable --edition 2024 --test src/stream_queue.rs -o target/stream-queue-tests
target/stream-queue-tests
cargo psp --release --locked
test -s target/mipsel-sony-psp/release/EBOOT.PBP
