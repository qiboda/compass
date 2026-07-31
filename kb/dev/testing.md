# Testing

## Framework

| Crate | Purpose |
|---|---|
| `rstest` | parameterized tests + fixtures |
| `httpmock` | HTTP mock server |
| `tempfile` | temporary file/dir creation |

## Test runner

```sh
cargo test                              # standard runner
cargo nextest run                       # recommended: faster, better output
cargo test --test integration_test      # integration tests only
```

## Test organization

- **Unit tests**: `#[cfg(test)] mod tests` at the bottom of each source file.
  Tests can access private functions and structs.
- **Integration tests**: `tests/` directory. Tests only the public API of
  `compass-core` (library crate).

## Writing tests

### Async tests with rstest

```rust
#[rstest]
#[case("000001", "1d")]
#[case("600519", "1w")]
#[tokio::test]
async fn test_name(#[case] symbol: &str, #[case] timeframe: &str) {
    // test body
}
```

Order matters: `#[rstest]` outermost, `#[tokio::test]` innermost.

### In-memory DuckDB

Use `DuckDbProvider::new_in_memory()` for fully isolated test databases:

```rust
let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
// Each call creates a separate in-memory DB — tests never interfere.
```

No cleanup needed — the database is dropped when `provider` goes out of scope.

### Dolt (test database)

Tests that need a Dolt database use `dolt init` + `dolt sql` to create a temporary,
self-contained database at runtime. No external data dependency.

```rust
let tmp = tempfile::tempdir().expect("create temp dir");

// Set identity for dolt init (uses git underneath)
std::process::Command::new("dolt")
    .arg("config").arg("--global").arg("--add")
    .arg("user.email").arg("test@compass.local")
    .output().expect("dolt config");
std::process::Command::new("dolt")
    .arg("config").arg("--global").arg("--add")
    .arg("user.name").arg("Test")
    .output().expect("dolt config");

// Init and create schema
std::process::Command::new("dolt")
    .arg("--data-dir").arg(tmp.path())
    .arg("init").output().expect("dolt init");

std::process::Command::new("dolt")
    .arg("--data-dir").arg(tmp.path())
    .arg("sql").arg("-q")
    .arg("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)")
    .output().expect("dolt sql");

// Query via run_dolt_sql_parquet / run_dolt_sql_csv
let data = run_dolt_sql_parquet(tmp.path(), "SELECT * FROM t").unwrap();
```

CI installs `dolt` from GitHub releases. Tests clean up automatically via
`TempDir` drop. The `investment_data` repo (18M+ rows) is never cloned.

### DuckDB deadlock avoidance

When writing tests that mix direct `db.conn.lock()` calls with async `DuckDbProvider`
methods (which internally lock via `spawn_blocking`), group all direct lock access
into ONE scope before any async `db` method calls:

```rust
// SAFE: all direct conn access before any async db calls
let (count_a, count_b) = {
    let conn = db.conn.lock().expect("lock");
    let c1 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE symbol='SZ000001'", ...)?;
    let c2 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE symbol='SH600519'", ...)?;
    (c1, c2)
}; // lock released

// Now safe to call async db methods
let info = db.get_stock_basic("SZ000001").await?;
```

The `DuckDbProvider` async methods use `spawn_blocking` which tries to lock `conn`
on a thread pool. If you hold `conn.lock()` in the outer scope and then call an
async `db` method, the spawned task blocks waiting for the lock you already hold — deadlock.

## Test patterns

1. **Provider isolation**: Create a fresh provider per test case. Don't share.
2. **Assert isolation**: After saving, fetch with different symbol/timeframe to
   verify no cross-contamination.
3. **Parameterize**: Use `#[case]` for same logic, different inputs.
4. **Error paths**: Test empty data, bad JSON, missing files.
5. **Integration tests**: Use in-memory DuckDB and run the full pipeline
   (import → save stock_daily → fetch bars → verify counts).

## Benchmarks

Performance benchmarks use [criterion.rs](https://github.com/bheisler/criterion.rs)
and live under `benches/` in each crate.

### Running

```sh
cargo bench                       # all benchmarks (slow — ~hours for full suite)
cargo bench --bench parquet_bench # specific benchmark
cargo bench -- --quick            # quick run (fewer samples, for development)
cargo bench --no-run              # CI: compile only, don't execute
```

Results are written to `target/criterion/` as HTML reports.

### Available benchmarks

| Crate | Bench file | What it measures |
|---|---|---|
| `compass-core` | `parquet_bench` | ParquetReader cold/warm read at 100/1000/5000 rows, real SZ000001 |
| `compass-core` | `duckdb_bench` | DuckDbProvider cache hit/miss, save throughput (10–5000 rows) |

### Data requirements

- **Parquet benchmarks**: need `parquet_data/` with real data OR generate synthetic data via in-memory DuckDB
- **All others**: use in-memory DuckDB or temp directories — no external dependencies

### CI policy

CI runs `cargo bench --no-run` to verify compilation. Benchmarks are NOT executed
in CI — CI environments are too variable for meaningful performance data.
Run benchmarks locally before and after performance-sensitive changes.

### Saving and comparing baselines

Benchmark results are saved to `bench_results/<version>/` for versioned tracking:

```sh
# Save a full baseline (auto-generates timestamp-based version)
scripts/bench-save.sh

# Save with explicit version
scripts/bench-save.sh v1.0

# Quick run (fewer samples, faster)
scripts/bench-save.sh v2.0 quick

# Compare current code against a previous baseline
cargo bench -- --baseline v1.0
```

The script runs `cargo bench -- --save-baseline <version>` then copies
results out of `target/criterion/` into `bench_results/<version>/`,
keeping them outside the build cache.

## Profiling (Tracy)

Compass supports the [Tracy profiler](https://github.com/wolfpld/tracy) via the
`tracing-tracy` crate. Tracy provides real-time, nanosecond-resolution CPU
profiling with flamegraph visualization.

### Setup

1. Install the Tracy profiler server from [GitHub Releases](https://github.com/wolfpld/tracy/releases)
   or build from source. You need the `tracy-capture` (or `tracy-profiler`) binary.

2. Run the Tracy capture server:
   ```sh
   tracy-capture -o compass.tracy
   ```
   This opens the Tracy GUI. It listens on `localhost:8086` by default.

3. Run Compass with the `tracy` feature:
   ```sh
   cargo run --features tracy
   # or: cargo run --bin compass-data --features tracy -- import --symbols 000001
   ```

### How it works

- All `tracing` spans (from `#[instrument]` and `#[tracing::instrument]` macros)
  are automatically converted to Tracy zones — no additional instrumentation needed.
- When Tracy is not running, the layer silently no-ops.
- When the `tracy` feature is not enabled at compile time, the entire dependency
  tree is pruned — zero runtime or compile-time overhead.
- Build without `--features tracy` for normal use. Only enable it when profiling.

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `cargo build --features tracy` fails | Missing C++ toolchain or cmake | `sudo apt install cmake build-essential` |
| No data appears in Tracy GUI | Firewall blocking port 8086 | Check `tracy-capture` is running on same machine |
| Link error: symbol not found | `tracy-client-sys` version mismatch with installed Tracy | Use `tracy-capture` version matching `tracy-client-sys 0.24` |
