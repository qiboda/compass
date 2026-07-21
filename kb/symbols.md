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

## to_secid() conversion

User-facing symbols map to EastMoney API `secid` format `"{market}.{code}"`.
The implementation is in `src/data/eastmoney.rs::to_secid()`.

### Explicit prefixes (case-insensitive)

For ambiguous codes, use a prefix to force the exchange:

| Input | secid | Explanation |
|---|---|---|
| `sh.000001` | `1.000001` | 上证指数 — explicit SH |
| `sz.000001` | `0.000001` | 平安银行 — explicit SZ |
| `bj.830799` | `0.830799` | 艾融软件 — explicit BJ |
| `SH.600519` | `1.600519` | case-insensitive |

### Heuristic (no prefix)

When no prefix is provided:

| Code starts with | Market | Rationale |
|---|---|---|
| `6` | Shanghai (1) | All SH stocks: 600/601/603/605/688/900 |
| Anything else | Shenzhen (0) | SZ stocks: 000–004, 002–003, 300–301; BJ: 8xxxxx |

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
