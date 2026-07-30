# Symbols & Stock Codes

Stock codes in China's A-share market aren't globally unique — the same numeric
code can mean different things on different exchanges. Compass has to handle
this ambiguity while keeping the data model simple enough for a single developer
to maintain.

## The A-share market landscape

China has three stock exchanges with distinct code ranges:

| Exchange | Chinese Name | Market Code | Code Ranges | Notable Segment |
|---|---|---|---|---|
| Shanghai (SH) | 上海证券交易所 | `1` (EastMoney) | 600xxx–605xxx | Main board |
| Shanghai (SH) | 上海证券交易所 | `1` | 688xxx | STAR Market (科创板) |
| Shanghai (SH) | 上海证券交易所 | `1` | 900xxx | B-shares (外币) |
| Shenzhen (SZ) | 深圳证券交易所 | `0` (EastMoney) | 000xxx–004xxx | Main board |
| Shenzhen (SZ) | 深圳证券交易所 | `0` | 002xxx–003xxx | SME board (中小板) |
| Shenzhen (SZ) | 深圳证券交易所 | `0` | 300xxx–301xxx | ChiNext (创业板) |
| Beijing (BJ) | 北京证券交易所 | `0` (EastMoney) | 8xxxxx | 北交所 |

The key insight: **for stocks, code ranges don't overlap across exchanges**.
`600xxx` is always Shanghai, `300xxx` always Shenzhen, `8xxxxx` always Beijing.
This means we can **infer the exchange from the code** — no need to store it
alongside every data row.

## Why Dolt-native symbols?

Compass uses the Dolt-native prefixed format (`"SZ000001"`, `"SH600519"`,
`"BJ830799"`) as the canonical identifier everywhere: in the `symbol` column
of `stock_daily.parquet`, in the DuckDB `symbol` column, and as the primary
key. Bare 6-digit codes are accepted as user input for convenience and
resolved to the canonical format via exchange inference.

The older format `"000001.SZ"` (ts_code convention) has been retired. Here's why:

**The problem with ts_code**: mixing identity with metadata creates ambiguity.
`"000001.SZ"` encodes two facts — the stock is `000001`, and it trades on
Shenzhen. But the exchange is **already determinable from the code** (see
inference rules below). Storing it in the identifier is redundant, and
redundancy breeds inconsistency — what if someone writes `"000001.SH"` by
mistake?

**What we gain from Dolt-native symbols**:
- No cross-exchange collisions: `SZ000852` (stock) and `SH000852` (index) are distinct rows in `stock_daily.parquet`
- Clear exchange at a glance: the 2-letter prefix is immediately visible
- Simpler format: all symbols live in one `stock_daily.parquet` file, identified by the `symbol` column

The `to_ts_code()` helper still exists in the codebase for backward
compatibility, but it's no longer used as a primary key.

## Exchange inference: the heuristic

Given a bare 6-digit code, what exchange does it belong to?

```rust
pub fn to_exchange(code: &str) -> &str
```

The rules, in order:

| Code starts with | Exchange | Rationale |
|---|---|---|
| `6` | `"SH"` | All 6xxxxx codes are Shanghai stocks |
| `8` | `"BJ"` | All 8xxxxx codes are Beijing stocks |
| Anything else | `"SZ"` | 000xxx–004xxx, 002xxx, 300xxx are all Shenzhen |

This heuristic is correct for **stocks**. Indices are a different story.

## The ambiguity: 000001

The `000xxx` range is the only one where codes overlap between exchanges:

| Code | Exchange | What it is |
|---|---|---|
| `000001` | Shenzhen | 平安银行 (Ping An Bank) — a stock |
| `000001` | Shanghai | 上证指数 (Shanghai Composite Index) — an index |
| `000002` | Shenzhen | 万科A (Vanke A) — a stock |
| `399001` | Shenzhen | 深证成指 (SZSE Component Index) — an index |

For stocks (the 99% use case), the heuristic defaults to **Shenzhen** for
`000xxx` codes, since that's where almost all stocks in this range trade.

For indices, use an **explicit prefix**:

| Input | Meaning |
|---|---|
| `000001` | 平安银行 (SZ stock — default) |
| `sh.000001` | 上证指数 (SH index — explicit) |
| `sz.000001` | 平安银行 (SZ stock — explicit, same as default) |

## Explicit prefixes

When the heuristic isn't what you want, force the exchange with a prefix:

| Prefix | Exchange | Example | Result |
|---|---|---|---|
| `sh.` | Shanghai | `sh.000001` | `"SH"`, `000001` |
| `sz.` | Shenzhen | `sz.600519` | `"SZ"`, `600519` (unusual but valid) |
| `bj.` | Beijing | `bj.830799` | `"BJ"`, `830799` |

Prefixes are **case-insensitive**: `SH.600519` and `sh.600519` are equivalent.

## EastMoney secid mapping

EastMoney's API uses a different identifier format: `"{market}.{code}"`, where
market is `0` for Shenzhen/Beijing and `1` for Shanghai.

The `to_secid()` function converts Compass symbols to EastMoney secids:

```rust
pub fn to_secid(symbol: &str) -> String
```

### Conversion table

| Our input | secid | How it works |
|---|---|---|
| `000001` | `0.000001` | Heuristic: code starts with `0` → SZ → market code `0` |
| `600519` | `1.600519` | Heuristic: code starts with `6` → SH → market code `1` |
| `688001` | `1.688001` | Same heuristic: `6` → SH → `1` |
| `300750` | `0.300750` | Starts with `3` (not 6 or 8) → SZ → `0` |
| `830799` | `0.830799` | Heuristic: code starts with `8` → BJ → but EastMoney uses `0` for BJ |
| `sh.000001` | `1.000001` | Explicit SH prefix → market `1` |
| `sz.000001` | `0.000001` | Explicit SZ prefix → market `0` |
| `bj.830799` | `0.830799` | Explicit BJ prefix → but EastMoney uses `0` for BJ |

Important note: Beijing exchange uses market code `0` in EastMoney's system —
same as Shenzhen. The distinction is handled by the code range, not the market
code.

### The conversion pipeline

```
User types: "600519"
    │
    ▼
to_exchange("600519") → "SH"         (infer exchange)
    │
    ▼
to_ts_code("600519") → "600519.SH"   (legacy, not primary key)
    │
    ▼
to_secid("600519") → "1.600519"      (EastMoney API format)
    │
    ▼
HTTP GET ...?secid=1.600519&klt=101...
```

## Dolt symbol mapping

Dolt's `investment_data` database stores symbols with exchange prefixes.
These prefixes are the canonical identifier — in the `symbol` column of
`stock_daily.parquet`, in the database, and in user-facing interfaces:

| Symbol | Stock |
|---|---|
| `SZ000001` | 平安银行 |
| `SH600519` | 贵州茅台 |
| `BJ830799` | 艾融软件 |

The `strip_prefix()` function exists for backward compatibility when matching
bare 6-digit input, but the canonical format always includes the prefix.

## Timeframe mapping

Compass accepts human-friendly timeframe strings in the GUI and CLI. These map
to EastMoney's numeric `klt` (K-line type) parameter:

| User Input | klt | Semantics | Typical Use |
|---|---|---|---|
| `1m` | `1` | 1 minute | Intraday |
| `5m` | `5` | 5 minutes | Intraday |
| `15m` | `15` | 15 minutes | Intraday |
| `30m` | `30` | 30 minutes | Intraday |
| `60m`, `1h` | `60` | 60 minutes | Intraday |
| `1d`, `daily`, `day` | `101` | Daily | Primary view |
| `1w`, `weekly`, `week` | `102` | Weekly | Long-term trends |
| `1M`, `monthly`, `month` | `103` | Monthly | Very long-term |
| (numeric string) | passthrough | Raw klt value | For testing |

If the input doesn't match any known string, it's passed through as-is. This
supports direct numeric klt values (e.g., `"101"` is interpreted as klt=101,
daily).

### Why these particular values?

EastMoney's klt numbering is their internal convention. `101`/`102`/`103` for
daily/weekly/monthly is the EastMoney standard. The minute-level values
(1/5/15/30/60) are more intuitive.

### Timeframe in the data model

The timeframe string is used as part of the composite key `(symbol, timeframe)`
in CompassState's BarsMap. This means you can have `("600519", "1d")` and
`("600519", "1w")` loaded simultaneously, and switching between them is instant
(no refetch).

## Putting it all together

A complete symbol conversion for a typical user flow:

```
User opens Compass.
Config says: default_symbol = "000001", default_timeframe = "1d"

    "000001" → to_exchange → "SZ"
    "000001" → to_secid → "0.000001"
    "1d"     → timeframe_to_klt → 101

Worker sends: GET ...?secid=0.000001&klt=101&beg=...
Response → parsed into Vec<Bar>
Bars stored under key ("000001", "1d") in CompassState.bars

User types "600519", clicks Fetch.

    "600519" → to_secid → "1.600519"
    Fetches, caches, displays.

User types "sh.000001", wants the index.

    "sh.000001" → explicit prefix → to_secid → "1.000001"
     Fetches Shanghai Composite Index instead of 平安银行.

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 规范标识符：股票代码格式 | ts_code（`000001.SZ`）/ 裸六位码 / Dolt-native 前缀格式（`SZ000001`） | Dolt-native 前缀格式 | 前缀显式区分交换所，消除跨所碰撞（`SZ000852` vs `SH000852` 是不同行）；格式一致，均为一列 symbol | ts_code 混合身份与元数据，后缀格式不一致且解析需处理 `.` 分隔符；裸六位码无法区分 `000001`（平安银行 SZ）和 `000001`（上证指数 SH） |
| 交换所推断：如何从代码确定交易所 | 按代码前缀启发式规则 / 存储独立 exchange 列 / 要求用户手动选择 | 启发式规则：6→SH、8→BJ、其余→SZ | A 股代码区间不跨所：600xxx 必为沪市，300xxx 必为深市，8xxxxx 必为北交所；无需每行存储 exchange，减少冗余 | 存储独立列增加数据冗余且无实际收益；启发式对 000xxx 区间有歧义（股 vs 指），通过显式前缀 `sh.` 解决 |
| 歧义处理：000001 既可是股票也可是指数 | 不支持指数 / 默认选常见类型 / 显式前缀覆盖 | 默认 SZ（股票），显式前缀 `sh.` / `sz.` / `bj.` 覆盖 | 99% 使用场景是查股票，默认即可满足；极少数需要指数的场景用 `sh.000001` 显式指定 | 不支持指数会丢失功能；默认选指数会导致股票查询错误 |
```
