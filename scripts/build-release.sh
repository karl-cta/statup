#!/usr/bin/env bash
# Usage: scripts/build-release.sh [target...]
#        scripts/build-release.sh --binaries <dir>
# Writes dist/statup-<version>-<platform>.tar.gz (.zip for Windows), each
# holding the binary, the static files, the README and the licenses, and
# dist/SHA256SUMS. Builds each given Rust target, the host by default, or
# packages the binaries already in <dir>/<platform>/.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="$ROOT/dist"
TAILWIND="$ROOT/tailwindcss"
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"

# Files owned by root in the archives, whoever built them: tar run as root
# keeps the owner it reads. No macOS metadata files.
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    ROOT_OWNER=(--owner=0 --group=0 --numeric-owner)
else
    ROOT_OWNER=(--uid 0 --gid 0)
fi
export COPYFILE_DISABLE=1

# The platform of a Rust target in the archive name, as `uname -m` says it.
platform_of() {
    case "$1" in
        x86_64-unknown-linux-*) echo linux-x86_64 ;;
        aarch64-unknown-linux-*) echo linux-aarch64 ;;
        x86_64-pc-windows-*) echo windows-x86_64 ;;
        *) echo "$1" ;;
    esac
}

# The server reads static/ from its working directory, so it ships beside
# the binary. The stylesheet sources stay out.
package() {
    local platform="$1"
    local binary="$2"
    local name="statup-${VERSION}-${platform}"
    local stage="$OUTPUT_DIR/$name"
    local file
    file="$(basename "$binary")"

    mkdir -p "$stage"
    cp "$binary" "$stage/$file"
    # A binary downloaded from a workflow artifact has lost its mode.
    chmod 755 "$stage/$file"
    cp -R "$ROOT/static" "$stage/static"
    # The first launch loads its transitions on their own, apart from style.css.
    find "$stage/static/css" -name '*.css' ! -name style.css ! -name setup-transitions.css -delete
    find "$stage/static/css" -mindepth 1 -type d -exec rm -rf {} +
    cp "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$ROOT/README.md" "$stage/"
    find "$stage" -name .DS_Store -delete

    case "$platform" in
        windows-*) (cd "$OUTPUT_DIR" && zip -qr "$name.zip" "$name") ;;
        *) tar "${ROOT_OWNER[@]}" -C "$OUTPUT_DIR" -czf "$OUTPUT_DIR/$name.tar.gz" "$name" ;;
    esac
    rm -rf "$stage"
    echo "Packaged $name"
}

write_checksums() {
    cd "$OUTPUT_DIR"
    if command -v sha256sum >/dev/null; then
        sha256sum statup-* > SHA256SUMS
    else
        shasum -a 256 statup-* > SHA256SUMS
    fi
}

package_binaries() {
    local dir binary
    for dir in "$1"/*/; do
        dir="${dir%/}"
        binary="$dir/statup"
        [ -f "$dir/statup.exe" ] && binary="$dir/statup.exe"
        if [ ! -f "$binary" ]; then
            echo "Error: no statup binary in $dir" >&2
            exit 1
        fi
        package "$(basename "$dir")" "$binary"
    done
}

build_and_package() {
    local target_dir target binary
    target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
    for target in "$@"; do
        echo "--- statup ${VERSION} for ${target} ---"
        cargo build --release --locked --target "$target"
        binary="$target_dir/$target/release/statup"
        [ -f "$binary.exe" ] && binary="$binary.exe"
        package "$(platform_of "$target")" "$binary"
    done
}

if [ "${1:-}" = "--binaries" ] && [ "$#" -ne 2 ]; then
    echo "Usage: scripts/build-release.sh --binaries <dir>" >&2
    exit 2
fi

if [ ! -x "$TAILWIND" ]; then
    echo "Error: tailwindcss binary not found at $TAILWIND" >&2
    echo "Download it from https://github.com/tailwindlabs/tailwindcss/releases" >&2
    exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"
"$TAILWIND" --input "$ROOT/static/css/input.css" --output "$ROOT/static/css/style.css" --minify

if [ "${1:-}" = "--binaries" ]; then
    package_binaries "$2"
elif [ "$#" -gt 0 ]; then
    build_and_package "$@"
else
    build_and_package "$(rustc -vV | sed -n 's/^host: //p')"
fi
write_checksums
