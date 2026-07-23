# Testing

## Framework

```toml
[dev-dependencies]
rstest = "0.25"       # parameterized tests + fixtures
httpmock = "0.8"     # HTTP mock server
tempfile = "3"        # temporary file/dir creation
```

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

### HTTP mocking (httpmock)

```rust
use httpmock::MockServer;

let server = MockServer::start_async().await;
let mock = server.mock(|when, then| {
    when.method(httpmock::Method::GET).path("/api/qt/stock/kline/get");
    then.status(200)
        .header("content-type", "application/json")
        .json_body(serde_json::json!({"data": {"klines": ["2025-07-21,12.04,12.01,12.11,11.95,1079027,..."]}}));
});

let provider = EastMoneyProvider::new(
    reqwest::Client::new(),
    server.base_url(),
    server.base_url(),
);
// Now HTTP calls hit the mock server instead of real EastMoney.
```

### DuckDB deadlock avoidance

When writing tests that mix direct `db.conn.lock()` calls with async `DuckDbProvider`
methods (which internally lock via `spawn_blocking`), group all direct lock access
into ONE scope before any async `db` method calls:

```rust
// SAFE: all direct conn access before any async db calls
let (count_a, count_b) = {
    let conn = db.conn.lock().expect("lock");
    let c1 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE ts_code='000001.SZ'", ...)?;
    let c2 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE ts_code='600519.SH'", ...)?;
    (c1, c2)
}; // lock released

// Now safe to call async db methods
let info = db.get_stock_basic("000001.SZ").await?;
```

The `DuckDbProvider` async methods use `spawn_blocking` which tries to lock `conn`
on a thread pool. If you hold `conn.lock()` in the outer scope and then call an
async `db` method, the spawned task blocks waiting for the lock you already hold — deadlock.

## Test patterns

1. **Provider isolation**: Create a fresh provider per test case. Don't share.
2. **Assert isolation**: After saving, fetch with different symbol/timeframe to
   verify no cross-contamination.
3. **Parameterize**: Use `#[case]` for same logic, different inputs.
4. **Error paths**: Test empty data, bad JSON, network failure (via httpmock).
5. **Integration tests**: Mock EastMoney endpoints with httpmock, use DuckDB
   `:memory:`, and run the full pipeline (enumerate → stock_basic → fetch bars →
   save stock_daily → verify counts).
