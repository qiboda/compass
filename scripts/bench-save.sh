#!/bin/bash
# Save benchmark results to bench_results/<version>/
#
# Usage:
#   scripts/bench-save.sh              # auto-generate version from date
#   scripts/bench-save.sh v1.0         # explicit version name
#   scripts/bench-save.sh v1.0 quick   # quick mode (fewer samples)
#
# After saving, compare against a previous baseline:
#   cargo bench -- --baseline v0.9

set -euo pipefail

VERSION="${1:-$(date +%Y%m%d-%H%M%S)}"
MODE="${2:-full}"
BENCH_DIR="bench_results/$VERSION"

echo "=== Compass Benchmark Save ==="
echo "Version: $VERSION"
echo "Output:  $BENCH_DIR"
echo ""

# Benchmarks to run (skip binary targets that don't use criterion)
BENCHES="parquet_bench duckdb_bench cached_bench eastmoney_bench dolt_bench"
BENCH_FLAGS=""
for b in $BENCHES; do
    BENCH_FLAGS="$BENCH_FLAGS --bench $b"
done

# Run benchmarks with --save-baseline to generate criterion data
if [ "$MODE" = "quick" ]; then
    echo "Mode: quick (fewer samples)"
    cargo bench $BENCH_FLAGS -- --save-baseline "$VERSION" --quick
else
    echo "Mode: full"
    cargo bench $BENCH_FLAGS -- --save-baseline "$VERSION"
fi

# Copy results from target/criterion/ to bench_results/<version>/
# Criterion stores baselines under target/criterion/<bench_group>/<baseline>/
COPIED=0
for bench_dir in target/criterion/*/; do
    bench_name=$(basename "$bench_dir")
    # Skip the report directory
    [ "$bench_name" = "report" ] && continue
    src="$bench_dir/$VERSION"
    if [ -d "$src" ]; then
        dst="$BENCH_DIR/$bench_name"
        mkdir -p "$dst"
        cp -r "$src"/* "$dst/" 2>/dev/null || true
        COPIED=$((COPIED + 1))
    fi
done

echo ""
echo "=== Done ==="
echo "Copied $COPIED benchmark groups to $BENCH_DIR"
echo ""
echo "Compare against a previous baseline:"
echo "  cargo bench -- --baseline <prev-version>"
echo ""
echo "Available baselines:"
ls -d bench_results/*/ 2>/dev/null | sed 's|bench_results/||;s|/||' | head -10
