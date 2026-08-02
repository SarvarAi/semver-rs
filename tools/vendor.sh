#!/usr/bin/env bash
# Reproduce the exact upstream tree this port was built against.
#
# The upstream clone is deliberately NOT committed to this repo. Instead it is
# reproducible from this script and verifiable against baseline/KICKOFF-HASHES.txt,
# which was generated before any port code existed.
#
#   ./tools/vendor.sh          # clone + checkout pinned commit
#   ./tools/vendor.sh --verify # also re-verify every file hash
set -euo pipefail

PIN=6e05b7637396ac66522cff8731f07cfe0ef49a29
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/vendor/node-semver"

if [ ! -d "$DEST/.git" ]; then
  mkdir -p "$ROOT/vendor"
  git clone --quiet https://github.com/npm/node-semver.git "$DEST"
fi

git -C "$DEST" fetch --quiet --all
git -C "$DEST" checkout --quiet "$PIN"

echo "vendor/node-semver @ $(git -C "$DEST" rev-parse HEAD) ($(git -C "$DEST" describe --tags))"

if [ "${1:-}" = "--verify" ]; then
  echo "verifying against baseline/KICKOFF-HASHES.txt ..."
  ( cd "$DEST" && shasum -a 256 -c "$ROOT/baseline/KICKOFF-HASHES.txt" --quiet ) \
    && echo "OK: all 154 upstream files match the kickoff manifest byte-for-byte."
fi
