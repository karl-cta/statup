#!/usr/bin/env bash
# Usage: scripts/build-release.sh [target...]
# Builds dist/statup-<version>-<target>.tar.gz (binary, static files, README and licenses) for the host or each given Rust target.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="$ROOT/dist"
TAILWIND="$ROOT/tailwindcss"
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"

if [ "$#" -gt 0 ]; then
    TARGETS=("$@")
else
    TARGETS=("$(rustc -vV | sed -n 's/^host: //p')")
fi

if [ ! -x "$TAILWIND" ]; then
    echo "Error: tailwindcss binary not found at $TAILWIND" >&2
    echo "Download it from https://github.com/tailwindlabs/tailwindcss/releases" >&2
    exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

"$TAILWIND" --input "$ROOT/static/css/input.css" --output "$ROOT/static/css/style.css" --minify

# The server reads static/ from its working directory, so it ships beside
# the binary. The stylesheet sources stay out.
package() {
    local target="$1"
    local name="statup-${VERSION}-${target}"
    local stage="$OUTPUT_DIR/$name"

    mkdir -p "$stage"
    cp "$ROOT/target/$target/release/statup" "$stage/statup"
    cp -R "$ROOT/static" "$stage/static"
    find "$stage/static/css" -name '*.css' ! -name style.css -delete
    find "$stage/static/css" -mindepth 1 -type d -exec rm -rf {} +
    cp "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/README.md" "$stage/"

    tar -C "$OUTPUT_DIR" -czf "$OUTPUT_DIR/$name.tar.gz" "$name"
    rm -rf "$stage"
    echo "Built $OUTPUT_DIR/$name.tar.gz"
}

for target in "${TARGETS[@]}"; do
    echo "--- statup ${VERSION} for ${target} ---"
    cargo build --release --locked --target "$target"
    package "$target"
done
