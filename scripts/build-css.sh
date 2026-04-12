#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAILWIND="$ROOT/tailwindcss"

if [ ! -x "$TAILWIND" ]; then
    echo "Error: tailwindcss binary not found at $TAILWIND" >&2
    echo "Download it from https://github.com/tailwindlabs/tailwindcss/releases" >&2
    exit 1
fi

INPUT="$ROOT/static/css/input.css"
OUTPUT="$ROOT/static/css/style.css"

if [ "${1:-}" = "--watch" ]; then
    exec "$TAILWIND" --input "$INPUT" --output "$OUTPUT" --watch
elif [ "${1:-}" = "--minify" ]; then
    exec "$TAILWIND" --input "$INPUT" --output "$OUTPUT" --minify
else
    exec "$TAILWIND" --input "$INPUT" --output "$OUTPUT"
fi
