# Symbols & Stock Codes

## A-share market segments

| Exchange | Market code | Code ranges | Examples |
|---|---|---|---|
| Shanghai (SH) | 1 | 600xxx, 601xxx, 603xxx, 605xxx | 600519 贵州茅台 |
| Shanghai STAR (科创板) | 1 | 688xxx | 688001 华兴源创 |
| Shanghai (B-share) | 1 | 900xxx | 900901 云赛B |
| Shenzhen (SZ) 主板 | 0 | 000xxx–004xxx | 000001 平安银行 |
| Shenzhen 中小板 | 0 | 002xxx, 003xxx | 002415 海康威视 |
| Shenzhen 创业板 | 0 | 300xxx–301xxx | 300750 宁德时代 |
| Beijing (北交所) | 0 | 8xxxxx | 830799 艾融软件 |

## symbol convention

The primary key across all data tables is the bare 6-digit `symbol` code.
The older `ts_code` convention (`"{code}.{exchange}"`) has been retired
from the schema; `to_ts_code()` still exists for backward compatibility.

| Bare code | Exchange | Stock |
|---|---|---|
| `000001` | SZ | 平安银行 |
| `600519` | SH | 贵州茅台 |
| `688001` | SH | 华兴源创 (科创板) |
| `300750` | SZ | 宁德时代 (创业板) |
| `830799` | BJ | 艾融软件 (北交所) |

### Conversion functions (`crates/compass-core/src/data/symbol.rs`)

```rust
/// Infer exchange from stock code (heuristic + explicit prefix support).
/// Returns "SH", "SZ", or "BJ".
pub fn to_exchange(code: &str) -> &str

/// Convert bare code to full ts_code: "{code}.{exchange}".
/// "000001" → "000001.SZ", "sh.600519" → "600519.SH"
pub fn to_ts_code(symbol: &str) -> String
```

These functions are used by both `DuckDbProvider` (to convert bare symbols to
ts_code for SQL queries) and `EastMoneyProvider` (for `to_secid` mapping).

### Explicit prefixes (case-insensitive)

For ambiguous codes, use a prefix to force the exchange:

| Input | to_exchange | to_ts_code |
|---|---|---|
| `sh.000001` | `"SH"` | `"000001.SH"` |
| `sz.000001` | `"SZ"` | `"000001.SZ"` |
| `bj.830799` | `"BJ"` | `"830799.BJ"` |
| `SH.600519` | `"SH"` | `"600519.SH"` |

### Heuristic (no prefix)

When no prefix is provided:

| Code starts with | Exchange | ts_code suffix |
|---|---|---|
| `6` | Shanghai (SH) | `.SH` |
| `8` | Beijing (BJ) | `.BJ` |
| Anything else | Shenzhen (SZ) | `.SZ` |

## to_secid() conversion

User-facing symbols map to EastMoney API `secid` format `"{market}.{code}"`.
The implementation is in `crates/compass-core/src/data/eastmoney.rs::to_secid()`.

| Input | secid | Explanation |
|---|---|---|
| `sh.000001` | `1.000001` | 上证指数 — explicit SH |
| `sz.000001` | `0.000001` | 平安银行 — explicit SZ |
| `bj.830799` | `0.830799` | 艾融软件 — explicit BJ |
| `SH.600519` | `1.600519` | case-insensitive |
| `000001` | `0.000001` | Heuristic: SZ (most stocks in 000xxx range) |
| `600519` | `1.600519` | Heuristic: SH (all 6xxxxx codes) |

### Ambiguity: the 000xxx–004xxx range

`000001`–`004999` is the only overlapping range:
- **Shenzhen**: stocks like 平安银行(000001), 万科A(000002)
- **Shanghai**: indices like 上证指数(000001), 深证成指(399001)

**Default behavior**: without prefix → Shenzhen (stocks are the common case).
Use `sh.000001` to fetch Shanghai Composite Index instead.

## Timeframe mapping

EastMoney uses numeric `klt` values:

| User input | klt | Description |
|---|---|---|
| `1m` | `1` | 1 minute |
| `5m` | `5` | 5 minutes |
| `15m` | `15` | 15 minutes |
| `30m` | `30` | 30 minutes |
| `60m`, `1h` | `60` | 60 minutes |
| `1d`, `daily`, `day` | `101` | Daily |
| `1w`, `weekly`, `week` | `102` | Weekly |
| `1M`, `monthly`, `month` | `103` | Monthly |

Unrecognized strings are passed through unchanged (supports numeric klt directly).
