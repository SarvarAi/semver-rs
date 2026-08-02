#!/usr/bin/env bash
# One-time setup so the demo can be recorded without any investigation on camera.
#
#   ./demo/setup.sh
#
# Only the FIRST step is needed to build and test the port itself. The rest
# exists so the live Node-comparison and fuzz sections of the demo can run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> building the shipped port (this is the whole build)"
cargo build --release

echo "==> running the offline parity replay (no Node required)"
cargo test --release

echo "==> fetching the pinned upstream clone (harness only)"
./tools/vendor.sh

echo "==> installing upstream dev deps, without touching any tracked file"
npm install --no-save --no-package-lock --silent --prefix vendor/node-semver

echo "==> building the differential harness binaries"
cargo build --release --features harness

echo "==> verifying upstream is byte-identical to the kickoff manifest"
./tools/vendor.sh --verify

echo
echo "ready. Walk demo/script.md from the top."
