#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COV_DIR="$ROOT/coverage"

echo "=== Statup, Code Coverage ==="
echo ""

# Check prerequisites
if ! command -v cargo-llvm-cov &>/dev/null; then
    echo "Error: cargo-llvm-cov is not installed."
    echo "Install it with: cargo install cargo-llvm-cov"
    echo "Then run: rustup component add llvm-tools-preview"
    exit 1
fi

# Clean previous coverage data
cargo llvm-cov clean --workspace

# Run tests and generate HTML report + console summary
echo "--- Running tests with coverage instrumentation ---"
echo ""

mkdir -p "$COV_DIR"

cargo llvm-cov --all-features --workspace \
    --html --output-dir "$COV_DIR" \
    2>&1

echo ""
echo "--- Coverage Summary ---"
echo ""

# Generate text summary and display it
SUMMARY=$(cargo llvm-cov report --summary-only 2>&1)
echo "$SUMMARY"

echo ""
echo "--- Per-module coverage (services & models) ---"
echo ""

# Extract coverage for key modules
cargo llvm-cov report 2>&1 | grep -E "(services/|models/)" || true

echo ""
echo "HTML report: $COV_DIR/html/index.html"
echo ""

# Check threshold on services and models
THRESHOLD=80

check_module_coverage() {
    local module="$1"
    local lines

    lines=$(cargo llvm-cov report 2>&1 \
        | grep "$module" \
        | awk '{
            # Find the last percentage in the line (region/function/line coverage, line is last)
            for (i=NF; i>=1; i--) {
                if ($i ~ /%$/) {
                    gsub(/%/, "", $i)
                    print $i
                    exit
                }
            }
        }')

    if [ -z "$lines" ]; then
        echo "  ⚠ $module: no coverage data found"
        return 0
    fi

    # Average across files in the module
    local total=0
    local count=0
    while IFS= read -r pct; do
        total=$(echo "$total + $pct" | bc)
        count=$((count + 1))
    done <<< "$lines"

    if [ "$count" -eq 0 ]; then
        echo "  ⚠ $module: no files found"
        return 0
    fi

    local avg
    avg=$(echo "scale=1; $total / $count" | bc)
    local int_avg
    int_avg=$(echo "$avg" | cut -d. -f1)

    if [ "$int_avg" -ge "$THRESHOLD" ]; then
        echo "  ✓ $module: ${avg}% (threshold: ${THRESHOLD}%)"
        return 0
    else
        echo "  ✗ $module: ${avg}% < ${THRESHOLD}% threshold"
        return 1
    fi
}

PASS=true
check_module_coverage "services/" || PASS=false
check_module_coverage "models/" || PASS=false

echo ""
if [ "$PASS" = true ]; then
    echo "=== Coverage check PASSED ==="
else
    echo "=== Coverage check FAILED, some modules below ${THRESHOLD}% ==="
    exit 1
fi
