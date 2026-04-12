#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-$(cargo metadata --no-deps --format-version 1 2>/dev/null | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4)}"
OUTPUT_DIR="$ROOT/dist"

echo "=== Statup release build v${VERSION} ==="

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

# Targets to build
TARGETS=(
    "x86_64-unknown-linux-musl:linux-amd64"
    "aarch64-unknown-linux-musl:linux-arm64"
    "x86_64-apple-darwin:darwin-amd64"
    "aarch64-apple-darwin:darwin-arm64"
)

# Build Tailwind CSS (minified)
echo "--- Building CSS ---"
TAILWIND="$ROOT/tailwindcss"
if [ -x "$TAILWIND" ]; then
    "$TAILWIND" --input "$ROOT/static/css/input.css" --output "$ROOT/static/css/style.css" --minify
    echo "CSS built (minified)"
else
    echo "Warning: tailwindcss binary not found, skipping CSS build"
    echo "Download it from https://github.com/tailwindlabs/tailwindcss/releases"
fi

for entry in "${TARGETS[@]}"; do
    TARGET="${entry%%:*}"
    LABEL="${entry##*:}"
    BINARY_NAME="statup-${LABEL}"

    echo ""
    echo "--- Building ${BINARY_NAME} (${TARGET}) ---"

    if ! rustup target list --installed | grep -q "$TARGET"; then
        echo "Installing target ${TARGET}..."
        rustup target add "$TARGET" || {
            echo "Skipping ${TARGET} (cannot install target on this host)"
            continue
        }
    fi

    if cargo build --release --target "$TARGET" 2>/dev/null; then
        BINARY="$ROOT/target/${TARGET}/release/statup"

        # Strip symbols to reduce size
        if command -v strip &>/dev/null; then
            strip "$BINARY" 2>/dev/null || true
        fi

        cp "$BINARY" "$OUTPUT_DIR/${BINARY_NAME}"

        SIZE=$(du -h "$OUTPUT_DIR/${BINARY_NAME}" | cut -f1)
        echo "Built: ${BINARY_NAME} (${SIZE})"
    else
        echo "Skipping ${TARGET} (build failed, cross-compilation may require additional tooling)"
    fi
done

echo ""
echo "=== Release artifacts in ${OUTPUT_DIR}/ ==="
ls -lh "$OUTPUT_DIR/"
