# Testing

## Framework

```toml
[dev-dependencies]
rstest = "0.25"       # parameterized tests + fixtures
httpmock = "0.8"     # HTTP mock server (not yet wired)
```

## Test runner

```sh
cargo test                    # standard runner
cargo nextest run             # recommended: faster, better output
```

## Test organization

- **Unit tests**: `#[cfg(test)] mod tests` at the bottom of each source file.
  Tests can access private functions and structs.
- **Integration tests**: `tests/` directory (not yet created). Tests only the
  public API.

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

### In-memory SQLite

Pass `":memory:"` to `SqliteProvider::new()` for fully isolated test databases:

```rust
let provider = SqliteProvider::new(":memory:").unwrap();
// Each call creates a separate in-memory DB — tests never interfere.
```

No cleanup needed — the database is dropped when `provider` goes out of scope.

### HTTP mocking (httpmock)

```rust
use httpmock::MockServer;

let server = MockServer::start();
let mock = server.mock(|when, then| {
    when.method(GET).path("/api/qt/stock/kline/get");
    then.status(200).body(r#"{"data":{"klines":[...]}}"#);
});

let provider = EastMoneyProvider::new(
    reqwest::Client::new(),
    server.base_url(),
);
// Now HTTP calls hit the mock server instead of real EastMoney.
```

## Test patterns

1. **Provider isolation**: Create a fresh provider per test case. Don't share.
2. **Assert isolation**: After saving, fetch with different symbol/timeframe to
   verify no cross-contamination.
3. **Parameterize**: Use `#[case]` for same logic, different inputs.
4. **Error paths**: Test empty data, bad JSON, network failure (via httpmock).
