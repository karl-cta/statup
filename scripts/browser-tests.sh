#!/usr/bin/env bash
# Usage: scripts/browser-tests.sh [playwright options]
# Builds the stylesheet and the binary, then runs the browser tests of tests/browser against a fresh instance.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

"$ROOT/scripts/build-css.sh"
cargo build --locked --manifest-path "$ROOT/Cargo.toml"

cd "$ROOT/tests/browser"
[ -d node_modules ] || npm ci
npx playwright test "$@"
