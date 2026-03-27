#!/usr/bin/env bash
set -euo pipefail

OUTDIR="./build"
mkdir -p "$OUTDIR"

echo "Building yeokja binaries..."
cargo build --release -p yeokja-cli -p yeokja-server

cp target/release/yeokja "$OUTDIR/"
cp target/release/yeokja-server "$OUTDIR/"

echo ""
echo "Build complete:"
ls -lh "$OUTDIR"/yeokja "$OUTDIR"/yeokja-server
