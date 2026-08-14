# cli-downloader - Work Plan

## TL;DR (For humans)

**What you'll get:** A standalone `compass-downloader` CLI that downloads comprehensive A-share data into DuckDB — 7 normalized tables covering OHLCV, adjustments, stock info, trading status, price limits, indicators, and share capital. The entire project (GUI + CLI) migrates from a single `bars` SQLite table to DuckDB with proper financial database design. One command downloads all ~5,000 stocks completely.

**Why this approach:** EastMoney provides 90% of the data (OHLCV, indicators, limits, shares, basic info). Baostock fills the remaining 10% (adjustment factors). Store raw/unadjusted prices + adjustment factors, compute forward/backward adjusted on the fly — correct, space-efficient, and mathematically sound. DuckDB's columnar storage and native Parquet support make it ideal for this OLAP workload.

**What it will NOT do:** No data migration from old compass.db (schema too different). No export beyond Parquet. No daemon mode. No intraday data (daily only for v1).

**Effort:** Large (7 tables, Baostock integration, GUI schema migration)
**Risk:** Medium — Baostock Python subprocess call is a new integration surface; EastMoney realtime API endpoints need empirical verification for PE/PB/limits fields
**Decisions to sanity-check:** Separate DuckDB connection per table or single Mutex-protected connection; Baostock via `std::process::Command` calling Python script vs embedding Python; stock_basic populated once vs refreshed periodically

Your next move: `$start-work` or run a high-accuracy review (dual Momus). Full execution detail follows below.

---

> TL;DR (machine): Large effort, Medium risk. Deliverables: 7-table DuckDB schema, EastMoney multi-endpoint data collection, Baostock adj_factor integration, stock_daily gap detection, compass-downloader CLI, GUI schema migration, TDD tests, KB docs.

## Scope
### Must have
- [ ] 7-table DuckDB schema: stock_daily, stock_adj_factor, stock_basic, stock_status, stock_limit, daily_indicator, stock_share
- [ ] EastMoney: K-line (OHLCV+indicators), realtime quote (PE/PB/limits/shares), stock info (basic), symbol enumeration
- [ ] Baostock: Python subprocess integration for adj_factor retrieval
- [ ] DuckDbProvider: read/write methods for all 7 tables
- [ ] CachedProvider generic cache + NegativeCache trait (as previously designed)
- [ ] Cargo.toml: replace rusqlite with duckdb (+parquet); add clap, indicatif, futures
- [ ] src/lib.rs extraction; src/bin/downloader.rs CLI binary
- [ ] CLI: enumerate → download OHLCV → download indicators/limits/shares → download adj_factor → report
- [ ] Retry (3x exponential backoff) + indicatif progress
- [ ] Parquet export
- [ ] GUI: update to use stock_daily table + new schema
- [ ] TDD test suite: unit + integration (duckdb :memory:, httpmock)
- [ ] KB docs sync

### Must NOT have
- Do NOT keep rusqlite or SqliteProvider code
- Do NOT modify DataProvider/DataWriter trait signatures (add NegativeCache as NEW trait)
- Do NOT use CachedProvider in CLI downloader
- Do NOT use rt-multi-thread for CLI
- Do NOT spawn unbounded futures
- Do NOT use unwrap() — use .expect() with messages
- Do NOT create migration tool for old compass.db
- Do NOT break `cargo run --bin compass`

## Verification strategy
> Zero human intervention — all verification is agent-executed.
- **Test decision**: TDD. Write failing test FIRST, implement, refactor.
- **Framework**: rstest + `#[tokio::test]` + httpmock + duckdb `:memory:` + DuckDbProvider::new_in_memory()
- **Evidence**: `.omo/evidence/task-<N>-cli-downloader.txt`

## Execution strategy
### Waves

**Wave 1 — Foundation (CRITICAL, sequential):**
- DuckDB migration: replace SqliteProvider, create 7-table schema, NegativeCache trait, CachedProvider generic
- Cargo.toml: deps + [lib] + [[bin]]
- lib.rs extraction + main.rs GUI migration to new schema

**Wave 2 — Data Providers (parallel):**
- EastMoney: search_all_symbols, stock info API, realtime quote API (PE/PB/limits/shares)
- Baostock: Python script + Rust integration for adj_factor
- DuckDbProvider: all table read/write methods

**Wave 3 — CLI (sequential, depends on Wave 2):**
- Chunk splitting + CLI binary + retry + progress + Parquet export

**Wave 4 — Polish:**
- Integration tests + KB sync

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. DuckDB 7-table schema | — | 2,3,4,5,6,7,8,9,10,11 | — |
| 2. Cargo.toml + [lib]+[[bin]] | 1 | 3 | — |
| 3. lib.rs + main.rs GUI migration | 1,2 | 8,9 | 4,5,6,7 |
| 4. EastMoney multi-endpoint | 1 | 8,9 | 3,5,6,7 |
| 5. Baostock integration | 1 | 8,9 | 3,4,6,7 |
| 6. DuckDbProvider table methods | 1 | 8,9 | 3,4,5,7 |
| 7. Chunk splitting | 1 | 8 | 3,4,5,6 |
| 8. CLI binary | 1,3,4,5,6,7 | 10,11 | 9 |
| 9. Retry + progress | 1,3,4,5,6 | 8 | — |
| 10. Parquet export | 8 | — | 11 |
| 11. Integration tests + KB | 8 | — | 10 |

## DuckDB Schema (7 tables)

```sql
-- 1. Core OHLCV
CREATE TABLE IF NOT EXISTS stock_daily (
    ts_code     VARCHAR NOT NULL,      -- e.g. '000001.SZ'
    trade_date  DATE NOT NULL,
    open        DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    close       DOUBLE,
    pre_close   DOUBLE,                -- 昨收 (LAG(close) computed at insert time; NULL for first bar in chunk)
    change      DOUBLE,                -- 涨跌额
    pct_chg     DOUBLE,                -- 涨跌幅 (%)
    vol         DOUBLE,                -- 成交量 (手)
    amount      DOUBLE,                -- 成交额 (元)
    PRIMARY KEY (ts_code, trade_date)
);

-- 2. Adjustment factors (from Baostock)
CREATE TABLE IF NOT EXISTS stock_adj_factor (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    adj_factor  DOUBLE NOT NULL,       -- 复权因子
    PRIMARY KEY (ts_code, trade_date)
);
-- Usage: 前复权价格 = 原始价格 × adj_factor / latest_adj_factor
--        后复权价格 = 原始价格 × adj_factor

-- 3. Stock basic info
CREATE TABLE IF NOT EXISTS stock_basic (
    ts_code     VARCHAR PRIMARY KEY,
    symbol      VARCHAR,               -- 股票简称, e.g. '平安银行'
    name        VARCHAR,               -- 全称
    area        VARCHAR,               -- 地区
    industry    VARCHAR,               -- 行业
    market      VARCHAR,               -- 主板/创业板/科创板/北交所
    exchange    VARCHAR,               -- SH/SZ/BJ
    list_date   DATE,                  -- 上市日期
    delist_date DATE                   -- 退市日期 (NULL if active)
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_daily_date ON stock_daily(trade_date);
CREATE INDEX IF NOT EXISTS idx_adj_date ON stock_adj_factor(trade_date);
CREATE INDEX IF NOT EXISTS idx_status_date ON stock_status(trade_date);
CREATE INDEX IF NOT EXISTS idx_limit_date ON stock_limit(trade_date);
CREATE INDEX IF NOT EXISTS idx_indicator_date ON daily_indicator(trade_date);
CREATE INDEX IF NOT EXISTS idx_share_date ON stock_share(trade_date);

-- 4. Trading status
CREATE TABLE IF NOT EXISTS stock_status (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    is_open     BOOLEAN DEFAULT TRUE,  -- 是否交易
    PRIMARY KEY (ts_code, trade_date)
);

-- 5. Price limits (A-share特有)
CREATE TABLE IF NOT EXISTS stock_limit (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    up_limit    DOUBLE,                -- 涨停价
    down_limit  DOUBLE,                -- 跌停价
    PRIMARY KEY (ts_code, trade_date)
);

-- 6. Daily indicators
CREATE TABLE IF NOT EXISTS daily_indicator (
    ts_code         VARCHAR NOT NULL,
    trade_date      DATE NOT NULL,
    turnover_rate   DOUBLE,            -- 换手率 (%)
    turnover_rate_f DOUBLE,            -- 自由流通换手率
    volume_ratio    DOUBLE,            -- 量比
    pe              DOUBLE,            -- 市盈率
    pe_ttm          DOUBLE,            -- 市盈率TTM
    pb              DOUBLE,            -- 市净率
    ps              DOUBLE,            -- 市销率
    PRIMARY KEY (ts_code, trade_date)
);

-- 7. Share capital
CREATE TABLE IF NOT EXISTS stock_share (
    ts_code         VARCHAR NOT NULL,
    trade_date      DATE NOT NULL,
    total_share     DOUBLE,            -- 总股本 (万股)
    float_share     DOUBLE,            -- 流通股 (万股)
    free_share      DOUBLE,            -- 自由流通股 (万股)
    total_mv        DOUBLE,            -- 总市值 (万元)
    circ_mv         DOUBLE,            -- 流通市值 (万元)
    PRIMARY KEY (ts_code, trade_date)
);
```

### ts_code convention
- `{code}.{exchange}`: `000001.SZ`, `600519.SH`, `836149.BJ`
- Converts from current bare `000001` → `to_secid()` logic remains but adds exchange suffix

## Todos
> Implementation + Test = ONE todo. Never separate.

- [ ] 1. DuckDB migration: 7-table schema, DuckDbProvider, NegativeCache trait, CachedProvider generic
  What to do:
    - Delete `src/data/sqlite.rs` entirely
    - Remove `rusqlite` from Cargo.toml
    - Add `duckdb = { version = "1", features = ["bundled", "parquet"] }` to Cargo.toml
    - In `src/data/provider.rs`:
      - Change `DataError::Database` from `#[from] rusqlite::Error` to `#[from] duckdb::Error`
      - Add `NegativeCache` trait with `mark_no_data()` and `is_no_data()`
    - **Extract symbol→exchange logic** from `EastMoneyProvider::to_secid()` (private, `eastmoney.rs:37-52`) to `src/data/symbol.rs` as:
      ```rust
      pub fn to_exchange(code: &str) -> &str  // "000001"→"SZ", "600519"→"SH", "8xxxxx"→"BJ"
      pub fn to_ts_code(code: &str) -> String  // "000001"→"000001.SZ"
      ```
      This MUST be accessible from `DuckDbProvider` so it can convert bare symbols to ts_code in its `fetch_bars()` implementation — otherwise `WHERE ts_code='000001'` won't match `'000001.SZ'`.
    - Create `src/data/duckdb.rs` with `DuckDbProvider`:
      ```rust
      pub struct DuckDbProvider {
          conn: Arc<Mutex<duckdb::Connection>>,  // Connection: Send, not Sync
      }
      ```
    - `new(path)`: detect `:memory:` → `open_in_memory()`, else `open(path)`. Run all 7 CREATE TABLE statements
    - `new_in_memory()`: convenience for tests
    - Implement `DataProvider` for DuckDbProvider (reads stock_daily for fetch_bars)
    - Implement `DataWriter` for DuckDbProvider (writes to stock_daily for save_bars)
    - Implement `NegativeCache` for DuckDbProvider (no_data_marks table)
    - Add per-table read/write methods:
      - `save_stock_daily()`: INSERT OR REPLACE; **compute `pre_close` as `LAG(close) OVER (PARTITION BY ts_code ORDER BY trade_date)`** via DuckDB window function during insert (NULL acceptable for first bar in chunk — it's not in EastMoney K-line f51-f61 fields)
      - `get_stored_range()`: SELECT MIN/MAX trade_date for gap detection
      - `save_adj_factors()`, `get_adj_factor_range()`
      - `upsert_stock_basic()`, `get_stock_basic()`
      - `save_status()`, `save_limits()`, `save_indicators()`, `save_shares()`
    - In `src/data/mod.rs`: make CachedProvider cache generic `C: DataProvider + NegativeCache`
    - Update `src/main.rs` worker thread: `SqliteProvider::new` → `DuckDbProvider::new`
    - Port all tests: `SqliteProvider::new(":memory:")` → `DuckDbProvider::new_in_memory()`
    - `cargo build --bin compass` + `cargo test` → all green
  Must NOT do: Keep rusqlite; keep SqliteProvider; change DataProvider/DataWriter trait sigs
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2,3,4,5,6,7,8,9,10,11
  References: schema above, src/data/sqlite.rs:15-50, src/data/mod.rs:23-116, src/main.rs:151-165
  Acceptance criteria: `cargo build --bin compass` + `cargo test` green
  QA: `grep -r "rusqlite" src/` empty. `grep "stock_daily" src/data/duckdb.rs` found.
  Commit: Y | refactor(data): replace SqliteProvider with 7-table DuckDbProvider; add NegativeCache trait

- [ ] 2. Cargo.toml: Add [lib] + [[bin]] + dependencies
  What to do:
    - `[lib]`: `name = "compass_rs"`, `path = "src/lib.rs"`
    - `[[bin]]` GUI: `name = "compass"`, `path = "src/main.rs"`
    - `[[bin]]` CLI: `name = "compass-downloader"`, `path = "src/bin/downloader.rs"`
    - Add: `clap = { version = "4", features = ["derive"] }`, `indicatif = "0.17"`, `futures = "0.3"`
    - `cargo check` all three targets
  Must NOT do: Change existing dep versions (besides rusqlite→duckdb done in todo 1)
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 3
  References: Cargo.toml:1-27
  Acceptance criteria: All three `cargo check` targets pass
  QA: `cargo check --lib && cargo check --bin compass && cargo check --bin compass-downloader`
  Commit: Y | chore(Cargo): add lib target, compass-downloader binary, clap+indicatif+futures

- [ ] 3. src/lib.rs extraction + main.rs GUI migration to new schema
  What to do:
    - Create `src/lib.rs`: `pub mod data; pub mod model;`
    - Update `src/main.rs`: replace `mod data; mod model;` with `use compass_rs::...`
    - GUI schema migration:
      - `CompassApp`: update chart data pipeline from `bars` table to `stock_daily` table
      - Use `to_ts_code()` from `src/data/symbol.rs` (extracted in todo 1)
      - Update `fetch_bars` call: use `stock_daily` WHERE ts_code=? AND trade_date BETWEEN ?
      - **Weekly/monthly restriction for v1**: GUI timeframe dropdown limited to "1d" only. Remove "1w" and "1M" options. Weekly/monthly aggregation from daily data deferred to future iteration.
      - Keep existing chart rendering (Bar struct consumes same fields)
    - Update tests in main.rs to use new types
    - `cargo build --bin compass` and `cargo test` pass
  Must NOT do: Move GUI code into lib.rs; change chart rendering logic
  Parallelization: Wave 2 | Blocked by: 1,2 | Blocks: 8,9
  References: src/main.rs:12-13,244-280 (CompassApp), src/model.rs:124-141 (CompassState)
  Acceptance criteria: `cargo check --bin compass` + `cargo test` green
  QA: `grep "mod data" src/main.rs` empty. `grep "compass_rs::" src/main.rs` found.
  Commit: Y | refactor: extract data+model into library; migrate GUI to 7-table DuckDB schema

- [ ] 4. EastMoneyProvider: Multi-endpoint data collection
  What to do:
    - Add `to_ts_code(symbol: &str) -> String`: apply current `to_secid` logic + append `.SH`/`.SZ`/`.BJ`
    - Extend K-line fetch: include `pre_close`, `change`, `pct_chg`, `turnover_rate` from `fields2=f51..f61`
      - `pre_close` can be computed from previous day's close OR fetched from API
    - Add `fetch_realtime_quote(symbol) -> StockQuote` method:
      - URL: `push2.eastmoney.com/api/qt/stock/get`
      - Parse: PE (f9), PB (f167), total_share (f84), float_share (f85), up_limit (f51), down_limit (f52)
    - Add `fetch_stock_basic(symbol) -> StockBasic` method:
      - URL: `push2.eastmoney.com/api/qt/stock/get` or use search_symbols result
      - Parse: name (f58), industry, list_date, market
    - Add `search_all_symbols(page_size, fs_filter) -> Vec<SymbolInfo>` (NOT on DataProvider trait)
    - Write integration tests: httpmock for each new endpoint
  Must NOT do: Change DataProvider::search_symbols signature; break existing fetch_bars
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 8,9 | Parallel: 3,5,6,7
  References: src/data/eastmoney.rs:86-213, eastmoney-data-sdk API_FIELDS.md
  Acceptance criteria: `cargo test eastmoney` all pass; 3+ new test cases
  QA: httpmock each new endpoint → parsed correctly
  Commit: Y | feat(eastmoney): add multi-endpoint data collection (realtime, basic, symbols)

- [ ] 5. Baostock adj_factor integration
  What to do:
    - Create `scripts/fetch_adj_factor.py`: Python script that takes `ts_code`, `start_date`, `end_date` → outputs JSON array of `[{trade_date, adj_factor}]`
      ```python
      import baostock as bs, json, sys
      bs.login()
      rs = bs.query_adjust_factor(code, start_date, end_date)
      # output JSON to stdout
      ```
    - Create `src/bin/downloader/baostock.rs`:
      - `async fn fetch_adj_factors(ts_code, start, end) -> Result<Vec<AdjFactor>>`
      - Execute: `std::process::Command::new("python3").arg("scripts/fetch_adj_factor.py").arg(ts_code)...`
      - Parse stdout JSON → `Vec<AdjFactor { trade_date, adj_factor }>`
      - Cache: write to `stock_adj_factor` DuckDB table via DuckDbProvider
    - Add `AdjFactor` struct to model.rs
    - Write test: mock Python script output → verify parsing
  Must NOT do: Embed CPython; use blocking I/O in async context (use spawn_blocking for subprocess); hardcode Python path (use `which python3`)
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 8,9 | Parallel: 3,4,6,7
  References: Baostock docs (query_adjust_factor), std::process::Command
  Acceptance criteria: `cargo test downloader::baostock` passes
  QA: Mock JSON output → Vec<AdjFactor> parsed. Empty output → Ok(vec![]). Non-zero exit → Err.
  Commit: Y | feat(downloader): add Baostock adj_factor integration via Python subprocess

- [ ] 6. DuckDbProvider: Per-table read/write methods
  What to do:
    - Add methods to DuckDbProvider (NOT on any trait — direct methods):
      - `save_stock_daily(ts_code, records: &[DailyRecord])` — INSERT OR REPLACE into stock_daily
      - `get_stored_range(ts_code) -> Option<(NaiveDate, NaiveDate)>` — MIN/MAX trade_date
      - `save_adj_factors(ts_code, factors: &[AdjFactor])` — INSERT OR REPLACE into stock_adj_factor
      - `get_adj_factor_range(ts_code) -> Option<(NaiveDate, NaiveDate)>`
      - `upsert_stock_basic(info: &StockBasic)` — INSERT OR REPLACE into stock_basic
      - `get_stock_basic(ts_code) -> Option<StockBasic>`
      - `save_status(ts_code, records: &[StatusRecord])`
      - `save_limits(ts_code, records: &[LimitRecord])`
      - `save_indicators(ts_code, records: &[IndicatorRecord])`
      - `save_shares(ts_code, records: &[ShareRecord])`
    - All methods use `conn.lock()` inside `spawn_blocking`
    - Write unit tests: each method with in-memory DuckDB
  Must NOT do: Change DataProvider/DataWriter trait implementations; put these on traits (direct methods only)
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 8,9 | Parallel: 3,4,5,7
  References: src/data/sqlite.rs:110-211 (save/fetch pattern), schema above
  Acceptance criteria: `cargo test duckdb` all table method tests pass
  QA: Save 3 records → get_stored_range returns correct min/max. Save 0 → None.
  Commit: Y | feat(duckdb): add per-table read/write methods for all 7 tables

- [ ] 7. src/bin/downloader.rs: Date chunk splitting
  What to do:
    - `fn split_date_range(start: NaiveDate, end: NaiveDate, max_days: i64 = 2000) -> Vec<(NaiveDate, NaiveDate)>`
    - Simple day-based splitting (daily data only for v1)
    - Unit tests: 1-day, 2000-day, 5000-day ranges
  Must NOT do: Make HTTP calls; depend on providers
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 8 | Parallel: 3,4,5,6
  References: chrono::NaiveDate, src/data/eastmoney.rs:112 (lmt=2000)
  Acceptance criteria: `cargo test downloader::split_date_range` 5+ cases
  Commit: Y | feat(downloader): add date-based chunk splitting

- [ ] 8. src/bin/downloader.rs: CLI binary with full pipeline orchestration
  What to do:
    - Binary entry: `#[tokio::main(flavor = "current_thread")] async fn main()`
    - clap `Cli` struct: `--symbols`, `--db`, `--concurrency`, `--delay-ms`, `--start-date`, `--end-date`
    - Pipeline (per-symbol, via Semaphore + buffer_unordered):
      1. `search_all_symbols()` or parse CLI symbols
      2. For each symbol `ts_code`:
         a. Build `StockBasic` from EastMoney search result (one-time per symbol)
         b. `db.upsert_stock_basic(info)` → populate stock_basic table
         c. Gap detection: `db.get_stored_range(ts_code)` → compute missing date chunks
         d. For each chunk: fetch K-line → parse into `DailyRecord` → `db.save_stock_daily(ts_code, records)`
         e. Fetch realtime quote → parse PE/PB/limits/shares → `db.save_indicators()`, `db.save_limits()`, `db.save_shares()`
         f. `fetch_with_retry` wrapper for all HTTP calls
         g. `sleep(delay_ms)` between requests
      3. After all symbols: Baostock adj_factor batch:
         a. For each symbol with new data: `fetch_adj_factors(ts_code, ...)` via Python subprocess
         b. `db.save_adj_factors(ts_code, factors)`
      4. Summary + exit code
    - Write integration test: httpmock + DuckDB :memory:, 2 symbols complete pipeline
  Must NOT do: Use CachedProvider; use multi-thread runtime; use unwrap()
  Parallelization: Wave 3 | Blocked by: 1,3,4,5,6,7 | Blocks: 10,11
  References: schema above, todo 4 (EastMoney methods), todo 5 (Baostock), todo 6 (DuckDbProvider methods)
  Acceptance criteria: `cargo check --bin compass-downloader`; integration test passes
  QA: End-to-end with 2 symbols → all 7 tables populated
  Commit: Y | feat(downloader): add CLI binary with 7-table pipeline orchestration

- [ ] 9. src/bin/downloader.rs: Retry logic + indicatif progress
  What to do:
    - `fetch_with_retry(provider, ..., max_retries=3)`: exponential backoff 1s/2s/4s, retry on Network + RateLimited only
    - indicatif MultiProgress: spinner (enumeration) + ProgressBar (symbols) + per-symbol message
    - Integrate into todo 8's main flow
    - Unit test: retry succeeds on 2nd attempt; fails after 3
  Parallelization: Wave 3 | Blocked by: 1,3,4,5,6 | Blocks: —
  References: indicatif docs, src/data/provider.rs:14,22-24
  Acceptance criteria: `cargo test downloader::retry` passes
  Commit: Y | feat(downloader): add retry with exponential backoff and indicatif progress

- [ ] 10. src/bin/downloader.rs: Parquet export
  What to do:
    - After download: `--export-parquet <dir>` → `COPY stock_daily TO '<dir>/stock_daily.parquet' (FORMAT PARQUET, COMPRESSION ZSTD)`
    - Export all 7 tables as separate Parquet files
    - Test: in-memory DuckDB with mock data → export → verify files exist
  Parallelization: Wave 4 | Blocked by: 8 | Blocks: —
  Acceptance criteria: `cargo test downloader::parquet_export` passes
  Commit: Y | feat(downloader): add Parquet export for all 7 tables

- [ ] 11. Integration tests + KB docs sync
  What to do:
    - End-to-end test: httpmock (EastMoney K-line + realtime + symbols) + mock Baostock script output + DuckDB :memory:
      - 3 symbols enumerated → basic info saved → K-line fetched → indicators/limits/shares saved → adj_factor saved
      - Verify all 7 tables have expected row counts
    - KB sync:
      - `kb/architecture.md`: new 7-table schema, Source layout (add downloader.rs, delete sqlite.rs, add duckdb.rs), Baostock integration, data flow diagram
      - `kb/data-providers.md`: EastMoney multi-endpoint docs, Baostock integration, DuckDbProvider table methods
      - `kb/symbols.md`: ts_code convention (`000001.SZ` format)
      - `kb/process.md`: CLI commands, duckdb CLI inspection, Baostock setup (pip install baostock)
  Must NOT do: Keep SQLite references in KB
  Parallelization: Wave 4 | Blocked by: 8 | Blocks: —
  References: kb/*.md
  Acceptance criteria: `cargo test` all pass. `git diff kb/` shows comprehensive updates.
  Commit: Y | test(downloader): end-to-end integration test; docs(kb): 7-table schema, Baostock, CLI

## Final verification wave
- [ ] F1. Plan compliance: all 11 todos done, no Must-NOT violations
- [ ] F2. Code quality: `cargo clippy -- -D warnings` + `cargo fmt --check` + `lsp_diagnostics` clean
- [ ] F3. Real QA: `cargo run --bin compass-downloader -- --symbols "000001" --delay-ms 0` → all 7 tables populated. `cargo run --bin compass` GUI launches.
- [ ] F4. Scope fidelity: no rusqlite, no SQLite, 7 tables in DuckDB schema

## Commit strategy
| # | Type | Message |
|---|------|---------|
| 1 | refactor(data) | replace SqliteProvider with 7-table DuckDbProvider; add NegativeCache trait |
| 2 | chore(Cargo) | add lib target, compass-downloader binary, deps |
| 3 | refactor | extract data+model into library; migrate GUI to 7-table schema |
| 4 | feat(eastmoney) | add multi-endpoint data collection |
| 5 | feat(downloader) | add Baostock adj_factor integration |
| 6 | feat(duckdb) | add per-table read/write methods |
| 7 | feat(downloader) | add date-based chunk splitting |
| 8 | feat(downloader) | add CLI binary with 7-table pipeline |
| 9 | feat(downloader) | add retry + indicatif progress |
| 10 | feat(downloader) | add Parquet export |
| 11 | test(downloader) | end-to-end integration test; docs(kb): full schema + Baostock docs |

All commits = `ref #5`. Push after each.

## Success criteria
- [ ] `cargo run --bin compass-downloader -- --symbols all` populates all 7 tables
- [ ] Re-run idempotent per table
- [ ] `cargo run --bin compass` GUI works with new schema
- [ ] `--export-parquet /tmp/` produces valid Parquet files for all tables
- [ ] `cargo nextest run` all green
- [ ] `cargo clippy -- -D warnings` + `cargo fmt --check` clean
- [ ] Zero rusqlite references
- [ ] KB docs accurate and complete
