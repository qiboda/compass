# 数据管线（CLI）

## 概述

`compass-data` 通过六个子命令管理 A 股 OHLCV 数据：

```
Dolt investment_data ──import─────────► parquet_data/
Dolt compass_data ────import-compass──► parquet_data/
parquet_data/ ────────export──────────► duckdb / csv / parquet-dir
parquet_data/ ────────backup──────────► 百度云（zip）
parquet_data/ ────────check-stock-daily──► 缺口校验（只读）
parquet_data/ ────────sepa────────────► Dolt compass_data（评分写回）
```

东方财富数据由 Rust 采集器（`crates/compass-collectors`，二进制 `compass-collectors`）获取，存入 Dolt `compass_data`，再通过 `import-compass` 导入。Rust CLI 本身从不与东方财富通信。

## 通用选项

- **`--overwrite`**（用于 `import-compass` 和 `export`）：用新值替换已有数据。默认行为是合并/跳过 — 已有数据保留，仅添加新数据。
- `import`（Dolt investment_data）始终直接写入完整数据集 — 没有 `--overwrite` 标志。

---

## `import` — Dolt investment_data → Parquet（主要）

从本地 Dolt `investment_data` 数据库导入完整历史数据到 Parquet 主数据库。

```sh
cargo run --bin compass-data -- import [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--dolt-dir` | 来自配置 `[dolt].investment_data_dir` | Dolt 数据库目录 |
| `--output` | 来自配置 `[parquet].dir` | Parquet 文件输出目录 |
| `--symbols` | （全部） | 逗号分隔的带前缀代码（如 `SH600519,SZ000001`）。**仅接受带前缀输入**——裸 6 位码（如 `600519`）报错拒绝（D9）；`sh.600519` 等 dot 形式自动规范化为前缀。**⚠️ 过滤 + 覆盖**——parquet 将只剩这些符号的数据 |
| `--limit` | `0`（全部） | 最大导入股票数量。**⚠️ 过滤 + 覆盖**——parquet 将只剩前 N 只股票 |
| `--start-date` | （最早） | 按起始日期过滤（YYYYMMDD）。**⚠️ 过滤 + 覆盖**——parquet 将只剩该日期段 |
| `--end-date` | （最晚） | 按截止日期过滤（YYYYMMDD）。**⚠️ 过滤 + 覆盖**——parquet 将只剩该日期段 |
| `--since` | （无） | 仅导出 tradedate >= since 的数据（YYYYMMDD）。**⚠️ 过滤 + 覆盖全文件，非增量追加**——需增量请用 `import-compass` |

导入过程通过 `dolt sql -r parquet`（直接二进制 Parquet）读取每只股票的行数据，写入单一的 `stock_daily.parquet` 文件。再次运行会重新导入完整数据集。

**单位换算（ref #201）**：import 将 Dolt 源的 `volume`（手）×100 为**股**、`amount`（千元）×1000 为**元**后写入 parquet——`stock_daily.parquet` 中 volume 为股、amount 为元（SEPA 评分等下游按此口径消费）。

**指数剔除（ref #201）**：import 无条件剔除 6 个指数代码（SH000300/SH000852/SH000905/SH000906/SH000985/SZ399300）——即使 `--symbols` 显式指定也剔除；`stock_daily.parquet` 与 `stock_daily.symbols.txt` 均不含指数。

**⚠️ 过滤参数不是增量**：`import` 是「全量直写」命令——任何过滤参数（`--symbols`/`--limit`/`--start-date`/`--end-date`/`--since`）都只是 WHERE 过滤查询后**整体覆盖** `stock_daily.parquet`，旧数据不保留。需要增量/merge 语义请用 `import-compass`。

**数据质量校验（ref #136）**：`import` 写盘 `stock_daily.parquet` 后自动校验数据完整性，不一致即命令报错退出（exit 1）：

- **行数对比**：源 Dolt `final_a_stock_eod_price` 的 COUNT（同一 WHERE 过滤条件）vs parquet 实际行数，不一致 → 报错退出
- **`--limit>0` 时的预期**：`min(源 COUNT, limit)`（LIMIT 语义是"至多 N 行"）
- **日期范围**：源 vs 目标 `tradedate` MIN/MAX 对比（parquet 侧 `tradedate` 为 TIMESTAMP，经 `CAST AS DATE` 规范化为 `YYYY-MM-DD` 后再与 Dolt 侧 DATE 比较）；`--limit>0` 时跳过（LIMIT 截断行会破坏 MIN/MAX 语义）

### 输出结构

```
parquet_data/
├── stock_basic.parquet             # 股票元数据（由 import-compass --table stock_basic 生成）
├── stock_daily.parquet             # OHLCV 数据（单文件，含 symbol 列）
└── stock_daily.symbols.txt         # 股票索引（每行一个）
```

`stock_daily.parquet` 中的 `symbol` 列存储 Dolt 原生的股票代码格式（如 `SZ000001`、`SH600519`）。**指数代码已在 import 时剔除**（见上），parquet 仅含股票；共享同一 6 位代码的股票（SZ）与指数（SH）不再共存于数据中。

> **⚠️ 符号前缀规范化（issue #181）后需重新 import + export**：旧版 `compass.duckdb`
> 中的裸码数据在前缀化查询下会查空（Metis B9 实测）。升级后请重新运行
> `import` + `export` 让 DuckDB 数据以带前缀形式生效。

### 示例

```sh
# 全量导入（全部 6000+ 只股票，约 1 小时）
cargo run --bin compass-data -- import

# 导入指定股票（仅接受带前缀代码；裸码如 000001 会报错）
cargo run --bin compass-data -- import --symbols SZ000001,SH600519

# 带日期过滤的导入
cargo run --bin compass-data -- import --start-date 20200101 --end-date 20250721

# 导入前 100 只股票（测试用）
cargo run --bin compass-data -- import --limit 100

# 日期过滤导入（⚠️ 覆盖全文件为 since 后的子集，非增量追加；需增量请用 import-compass）
cargo run --bin compass-data -- import --since 20260725
```

---

## `check-stock-daily` — Parquet 交易日缺口校验（只读）

对比 `stock_daily.parquet` 中已有的交易日集合与 Dolt `investment_data`
的 `ts_trade_day_calendar`（SSE 开市日历），报告 parquet 在
`[min, max]` 范围内缺失的开市日期。**只读命令**——不写任何文件。

```sh
cargo run --bin compass-data -- check-stock-daily [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--dolt-dir` | 来自配置 `[dolt].investment_data_dir` | Dolt 数据库目录 |
| `--parquet-dir` | 来自配置 `[parquet].dir` | Parquet 文件目录 |

失败条件（exit 1）：
- parquet 无交易日（`stock_daily.parquet has no trade dates`）
- SSE 交易日历为空（calendar 查询失败或 0 行）
- 存在缺失日期（输出缺失数量与首批 10 个日期）

无缺口时静默通过（exit 0），`scripts/update-database.sh` step 1b 用它做
import 后的硬校验（缺口即中止流水线）。

---

## `import-compass` — Dolt compass_data → Parquet

从我们自己的 `compass_data` Dolt 仓库导入表（公司概况、财务指标、资产负债表、利润表、现金流量表）到 Parquet。

```sh
cargo run --bin compass-data -- import-compass --table <table> [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--table` | （必填） | `stock_basic`、`fin_indicators`、`fin_balance_sheet`、`fin_income`、`fin_cash_flow`、`capital_main_flow`、`dragon_list`、`block_trade`、`institution_survey`、`index_daily`、`index_basic` |
| `--dolt-dir` | 来自配置 `[dolt].compass_data_dir` | Dolt 数据库目录 |
| `--output` | 来自配置 `[parquet].dir` | Parquet 文件输出目录 |
| `--overwrite` | `false` | 替换已有数据而非合并 |
| `--since` | （无） | 增量导入：仅导入各表日期列 >= since 的数据（YYYY-MM-DD，如 2026-08-21；`import` 命令仍用 YYYYMMDD）。日期列按表不同：财务表（fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow）为 `report_date`；行情表（capital_main_flow/dragon_list/block_trade/index_daily）为 `trade_date`；institution_survey 为 `survey_date`。index_basic/stock_basic 不支持 `--since`（全量覆盖/镜像）。增量 merge 前会校验 Dolt `< since` 历史与既有 parquet 一致性，不一致自动降级为全量导出（ref #343） |

**指数/板块表（epic #255）**：`index_daily` / `index_basic` 存指数与板块数据
（官方指数 + 行业板块，来源：腾讯主源 + 东财备用 + THS，Rust 采集器 `index_daily`；
`index_type` 仅 `official`/`industry` 两种取值）：

- `index_daily`（指数/板块日线，含 `index_type` 列）：**增量 merge**——按 parquet 侧 PK
  `(symbol, tradedate)` 与既有 parquet 去重合并，`--since` 后新行并入、旧行不丢
  （与 `capital_main_flow` 同款 `import_append_table` 语义）；导出列含
  `adjclose = close` 占位（指数无复权，供 GUI 查询对齐）
- `index_basic`（指数/板块名称表）：**全量覆盖**——每次导入镜像 Dolt 当前状态，
  上游删板块即从 parquet 消失（`import index_daily` 时伴生写入）

```sh
# 导出指数/板块日线（增量 merge）
cargo run --bin compass-data -- import-compass --table index_daily --since 2026-01-01
# 导出指数/板块名称表（全量覆盖）
cargo run --bin compass-data -- import-compass --table index_basic
```

### 示例

```sh
# 导入公司概况
cargo run --bin compass-data -- import-compass --table stock_basic

# 导入财务指标（增量）
cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-01-01

# 强制覆盖
cargo run --bin compass-data -- import-compass --table stock_basic --overwrite
```

**数据质量校验（ref #136）**：`import-compass` 写盘后自动校验数据完整性：

- **全量导入**（无 `--since`/`--overwrite`/首次）：源 Dolt COUNT（含过滤条件）vs parquet 行数精确对比，不一致 → 报错退出（exit 1）
- **增量 merge**：merge 前先比对 Dolt `< since` 历史与旧 parquet `< since` 切片（双向 EXCEPT，可检出缺失历史/过期值/孤儿行三类分叉）；不一致 → **自动降级为不带 `--since` 的真全量导出**写回（先保留 `pre_merge_backup`），并对全量 Dolt COUNT 校验；一致 → 正常 merge，merge 后 parquet 行数 ≥ 旧 parquet 行数，否则报错退出；DuckDB merge 失败 fallback 同全量导出（ref #298、#343）
- **新鲜度（仅 warn，不退出）**：读 `compass_data` Dolt 的 `data_updates.last_report_date`，超过阈值仅告警——财务表（fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow）阈值 120 天；行情表（capital_main_flow/dragon_list/block_trade/institution_survey/index_daily/index_basic）阈值 7 天；stock_basic 不检查（其 last_report_date 为 NULL，采集器写库时不填）
- **⚠️ `--overwrite --since`**：显式覆盖时不再走增量 merge，而是用 `--since` 过滤后的导出**整体替换** parquet；该组合会丢掉过滤条件之外的历史行（与 `import --since` 同义）。无特殊需求应避免同时传这两个 flag（ref #298 外层根因提醒）。

---

## `export` — Parquet → DuckDB / CSV / parquet-dir

将 Parquet 主数据库导出为其他格式。

```sh
cargo run --bin compass-data -- export [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--input` | 来自配置 `[parquet].dir` | Parquet 数据目录 |
| `--format` | `duckdb` | 输出格式：`duckdb`、`csv`、`parquet-dir` |
| `--output` | `/data/compass-data/compass.duckdb` | 输出路径（`csv` 为文件路径，`parquet-dir` 为目录路径） |
| `--overwrite` | `false` | 替换已有数据而非跳过 |

三种格式的语义：

- **`duckdb`**：GUI 的读库路径。经 `fetch_bars` 读取，输出为**前复权价**
  （`factor_i = adjclose_i / close_i` 已烘焙，`adjclose` 列 == close）。
  如需原始未复权价，请直接读 Dolt `investment_data` 或 Parquet 源文件。
- **`csv`**：单文件 CSV。直读原始 parquet 行（不经 `fetch_bars`——其 `Bar`
  无 `amount` 字段），应用与图表路径相同的前复权因子
  （`factor = adjclose/close`，close ≤ 0 或 adjclose 非有限/非正时 factor = 1.0），
  因此**价格与图表一致而 `amount` 保留**。表头固定
  `symbol,trade_date,open,high,low,close,adjclose,volume,amount`；只导出
  规范带前缀符号（`SH/SZ/BJ`+6 位、`BK`+4-6 位），裸码/非规范行跳过；
  源缺失或符号集为空时仍写出 header-only 文件。
- **`parquet-dir`**：生成与主库同布局的新 parquet 目录
  （`stock_daily.parquet` + 伴生 `stock_daily.symbols.txt` + 镜像
  `index_daily.parquet`/`index_basic.parquet`），可再次被
  `ParquetReader::new` 读取。价格同样前复权（与 csv 同规则），
  `amount`/`volume` 原样透传；`symbols.txt` 符号集与导出 parquet
  `DISTINCT symbol` 一致；源 index parquet 缺失/空时跳过不阻断。

`--overwrite=true` 才替换已有输出（csv 文件 / parquet-dir 目录）；
默认（false）时输出已存在则 warn 跳过、不触碰旧文件。

### 示例

```sh
# 导出到 DuckDB
cargo run --bin compass-data -- export

# 导出为单文件 CSV（前复权价格 + 保留 amount）
cargo run --bin compass-data -- export --format csv --output /tmp/stock_daily.csv

# 导出为 parquet 目录（同主库布局）
cargo run --bin compass-data -- export --format parquet-dir --output /data/compass-data/parquet_data_mirror

# 强制覆盖
cargo run --bin compass-data -- export --format csv --output /tmp/stock_daily.csv --overwrite
```

---

## `backup` — Parquet → 百度云

将 `parquet_data/` 打包为 zip 并通过 `baidupcs`（BaiduPCS-Go）上传到百度云。

```sh
cargo run --bin compass-data -- backup [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--input` | 来自配置 `[parquet].dir` | 要备份的 Parquet 数据目录 |
| `--keep-zip` | `false` | 上传后保留本地 zip 文件 |

- 文件名带时间戳：`parquet_data-YYYYMMDD-HHMMSS.zip`
- 目标文件夹：百度云上的 `/compass/`
- 独立脚本：`scripts/upload-parquet.sh [--keep-zip]`

---

## Rust 采集器（数据源 → Dolt）

`crates/compass-collectors`（二进制 `compass-collectors`）是采集层唯一入口；
Python `collectors/` 已在 epic #310 完成迁移并退役。它使用 `wreq`
（Chrome TLS 指纹 HTTP 客户端）从各数据源获取数据并写入原始 CSV
（默认输出到 `/data/compass-data/csv/`，可用 `COMPASS_CSV_DIR` 环境变量覆盖），
再导入 Dolt `compass_data`。财务表与 SEPA 资金/题材表来自东方财富；
`stock_basic` 已切换到三大交易所官网：

`compass-collectors` 顶层子命令全集（`print_usage`，24 个）：

| 子命令 | 用途 |
|---|---|
| `fetch <target>` | 抓取单表数据到 CSV（`--years`/`--incremental` 等按 target） |
| `import <target>` | 将 CSV 导入 Dolt `compass_data` |
| `sync` | 获取 + 导入全部（auto-heal → 各表按序 → data_updates） |
| `sync-investment [--restart]` | 同步 `investment_data` 上游 Dolt 仓库 |
| `progress [target] [--json]` | 查询 SEPA 采集器抓取进度（issue #267） |
| `backfill --table T START END` | 按日期窗口回补（`--table` 可多个，如 `index_daily`） |
| `block-trade` | 大宗交易专用入口（`--start`/`--end`/`--years`/`--page-size`） |
| `dragon` | 龙虎榜专用入口（`--start`/`--end`/`--page-size`） |
| `institution-survey` | 机构调研专用入口（`--start-date`/`--page-size`） |
| `main-flow` | 主力资金流专用入口（新浪 lscjfb 逐股窗口，无需参数；只请求当日处于上市状态的股票，`stock_basic` list/delist 活跃区间过滤） |
| `main-flow-backfill --start D --end D [--symbols S,S]` | 主力资金流回补（`--symbols` 与全量均只请求 [start,end] 内上市股；`--symbols` 被过滤清空时报 `outside the active window` 错误，退市股不再被请求） |
| `fin-indicators` | 财务指标（`--years`/`--periods`/`--page-size`/`--incremental`） |
| `balance-sheet` / `income` / `cash-flow` | 财务三表（同 fin-indicators 参数） |
| `stock-basic` | 官网股票基本信息（`--output`/`--page-size`/`--max-pages`） |
| `index-daily` | 指数/板块日线（官方 + THS 行业） |
| `index-daily-probe --secid ID` | 单官方指数 kline 探测 |
| `index-daily-industries-probe` | THS 行业列表解析探测 |
| `index-daily-backfill --start D --end D` | 指数/板块日线回补 |
| `stock-basic-official` | = `fetch stock_basic` 的等价专用形态（`--output`/`--update-date`） |
| `freeproxy` | 代理源导入（`--source json`/`--json-url`/`--redis-url`/`--table`/`--limit`） |
| `keepalive` | 代理池保温常驻（见下方说明） |
| `check-proxy-pool` | 检查代理池可用性（`--api-url`/`--count`/`--timeout`） |

```sh
cargo run -p compass-collectors -- fetch stock_basic          # 上交所/深交所/北交所官网
cargo run -p compass-collectors -- fetch fin_indicators
cargo run -p compass-collectors -- fetch balance_sheet
cargo run -p compass-collectors -- fetch income
cargo run -p compass-collectors -- fetch cash_flow
cargo run -p compass-collectors -- fetch main_flow            # SEPA: 主力资金流（新浪 lscjfb 逐股 num=20 窗口）
cargo run -p compass-collectors -- fetch dragon               # SEPA: 龙虎榜席位
cargo run -p compass-collectors -- fetch block_trade          # SEPA: 大宗交易
cargo run -p compass-collectors -- fetch institution_survey   # SEPA: 机构调研
cargo run -p compass-collectors -- fetch index_daily          # SEPA: 指数/板块日线
cargo run -p compass-collectors -- sync                       # 获取 + 导入全部
cargo run -p compass-collectors -- sync-investment --restart
```

**抓取进度查询（`progress` 子命令，issue #267）**：一次写 CSV 的采集器在抓取期间
实时写 `csv_dir()/<name>.progress.json`（tmp+os.replace 原子写，可安全跨进程读取）。
写入方 6 个模块、产出 8 个进度文件：SEPA 五采集器（main_flow/block_trade/
dragon/institution_survey/index_daily——用快名）+ 财务三表（balance_sheet/income/
cash_flow 经 financial.rs 共享路径——进度文件名为 API 报告名
`RPT_F10_FINANCE_*.progress.json`）。**fin_indicators 不产生进度文件**（自有
fetch 循环，无 Progress 写入）；`progress <target>` 的 target 用对应文件名
（SEPA 快名 / RPT_* 报告名）。CSV 保持一次性写入语义：

```sh
cargo run -p compass-collectors -- progress                  # 全部采集器进度（人类可读）
cargo run -p compass-collectors -- progress dragon           # 单个采集器
cargo run -p compass-collectors -- progress --json           # 全部（原始 JSON，供脚本消费）
cargo run -p compass-collectors -- progress block_trade --json
```

进度文件在抓取结束后保留（status = `completed` / `failed`，failed 含 error 信息），
可复查上次运行结果；`progress` 自动扫描 `csv_dir()` 下全部 `*.progress.json`，
无 target 清单硬编码（列出的目标不存在时提示"no progress file"）。
`stock_basic`（官网采集器）不产生进度文件。

`fetch`/`import` 的 target 集合（`stock_basic`/`fin_indicators`/`balance_sheet`/
`income`/`cash_flow`/`dragon`/`block_trade`/`institution_survey`/`main_flow`/
`index_daily`）；`sync` 保持 auto-heal → 各表按序 fetch+import → data_updates 的完整顺序。

### 环境变量（collectors）

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `COMPASS_DATA_DIR` | config.toml `[dolt].compass_data_dir` → `/data/compass-data/compass_data` | compass_data Dolt 仓库目录（**env 优先**） |
| `COMPASS_INVESTMENT_DATA_DIR` | config.toml `[dolt].investment_data_dir` → `/data/compass-data/investment_data` | investment_data Dolt 仓库目录（**env 优先**） |
| `COMPASS_CSV_DIR` | `/data/compass-data/csv` | 原始 CSV 输出目录（也会在抓取时创建） |
| `COMPASS_NAME_EN_MAPPING` | `crates/compass-collectors/data/name_en_mapping.csv` | 英文名映射 CSV 路径 |
| `COMPASS_PROXY_API_URL` | `http://127.0.0.1:5010` | proxy_pool API 基址（`proxy.rs`） |
| `COMPASS_PROXY_DISABLE` | （未设） | `=1`/`true` 完全禁用代理层 |
| `COMPASS_AUTO_HEAL` | 启用 | `=0`/`false` 禁用 `sync` 的 auto-heal 缺口回补 |
| `COMPASS_TIMING_FILE` | 临时 JSONL | `sync` timing 事件上报路径（issue #334） |

`$HOME/.config/compass/config.toml` 的 `[dolt]` 节（`investment_data_dir`/
`compass_data_dir`）同样生效（A2，issue #336）：读取路径与 compass-data 的
`load_config` 相同，缺文件/坏文件 warn 并回退内置默认值；**env 始终优先于
config.toml**。

关键概念：
- **wreq** 用于 TLS 伪装（东方财富反爬虫；BSE 官网需要携带会话 cookie）
- **CSV 作为中间格式**，连接 API 与 Dolt
- **增量机制**：财务三表（fin_balance_sheet/fin_income/fin_cash_flow）与 fin_indicators 使用 **UPDATE_DATE 时间锚点**增量（见下方说明）；SEPA 时间序列表（main_flow/dragon/block_trade/institution_survey/index_daily）继续用 Dolt `data_updates.last_report_date` 锚点，只抓 `>= 最新已抓报告期` 的窗口；任一天/板块抓取失败即整体中止（不推进 watermark，重跑补全）。财务三表自 ref #202 起改用 **F10 完整版报表**（RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW，203/319/254 字段），自 issue #299 起采用 **UPDATE_DATE 增量 + merge/ODKU 导入**（历史永不丢失、修订覆盖）；fin_indicators + 5 个时间序列表（main_flow/dragon/block_trade/institution_survey/index_daily）**merge 导入**（CREATE IF NOT EXISTS + INSERT IGNORE 或 ODKU 按 PK 去重）——增量窗口 CSV 追加进已有表，绝不覆盖完整历史。长文本表（institution_survey org_name 可达 ~800 字节）与宽表（财务三表 203-319 列超 Dolt `-c` 推断行尺寸上限）用显式宽 schema 建临时表导入（`dolt table import -u`，采集器 `create_sql` 参数），避免 dolt 类型推断按 varchar(200) 字节截断 UTF-8 或 65504 字节行尺寸超限。

**fin_indicators 增量修订检测（issue #135，替代 #27 的 `--refresh N`）**：`cargo run -p compass-collectors -- fetch fin-indicators --incremental` 改用 **UPDATE_DATE 时间锚点**——filter=`(UPDATE_DATE>='{anchor}')`，锚点 = `min(data_updates.last_updated, state.json last_update_date)`（Dolt 为 source of truth，state.json 兜底；两源皆无则全量 REPORTDATE 枚举）。增量模式忽略 `--years/--periods`（锚点过滤跨报告期，旧报告期修订与新披露一体覆盖）。`fin_indicators::import_to_dolt` 改 **UPSERT**（`INSERT ... SELECT ... ON DUPLICATE KEY UPDATE`，SELECT 侧全列别名 + ODKU 无前缀别名引用——Dolt 2.2.3 不支持限定源列引用与 `VALUES()`），修订后的行覆盖 Dolt 旧 PK 行；CSV 每次写入后整文件 keep-LAST 去重（键 `(SECURITY_CODE, REPORTDATE)`）。**已知限制**：不做历史回补（锚点前的存量修订不重抓，风险自担）；fetch 与 import 应同日运行（跨日/单独 import 会致锚点超前漏抓间隙修订）；API 侧下架/删除的行不传播到 Dolt（UPSERT 只能覆盖不能删除）。

**财务三表增量修订检测（issue #299）**：`cargo run -p compass-collectors -- fetch balance-sheet --incremental`（income/cash_flow 同理）改用 **UPDATE_DATE 时间锚点**——filter=`(UPDATE_DATE>='{anchor}')`，锚点 = `min(data_updates.last_updated, state.json last_update_date)`（查 `fin_balance_sheet`/`fin_income`/`fin_cash_flow` 各自的 data_updates 行）。**无 anchor（首跑/无 state/无 data_updates）时固定 `2020-01-01`** 走一次 UPDATE_DATE 全历史拉取，不回退 REPORT_DATE 枚举。增量模式忽略 `--years/--periods`。导入改 `import_replace_table(merge=True)` + **`INSERT ... ON DUPLICATE KEY UPDATE`**（SELECT 侧全列唯一别名 + ODKU 无前缀别名引用），同 `(symbol, report_date)` 修订行覆盖旧值、历史永不丢失；CSV 写入后按 `(SECURITY_CODE, REPORT_DATE)` keep-LAST 去重。state.json 记录 `last_report_date` + `last_update_date`（`total_rows`/`last_run`）。`compass-collectors sync` 已对三表默认 `run(incremental=True)`；手动全量枚举可运行不带 `--incremental` 的 fetch（仍 merge 导入）。**已知限制**：与 fin_indicators 相同——不做锚点前历史回补；fetch 与 import 应同日运行；API 下架/删除行不传播到 Dolt。

SEPA 采集器说明：
- `main_flow`：新浪 `MoneyFlow.ssl_qsfx_lscjfb` 逐股日频窗口（`daima=sh600519` 形式，num=20 只导 `trade_date > last_report_date` 的行）；字段映射 `main_net_inflow=r0_net+r1_net`、`main_net_inflow_rate=(r0_net+r1_net)/(r0+r1+r2+r3)×100`（百分数，除零→0）、r0_net/r1_net/r2_net/r3_net → super/large/medium/small、`trade_date=opendate`；按 (symbol, trade_date) merge 导入；0 新行删陈旧 CSV 且按交易日历判定为 no-op（#338）；采集目标按 `stock_basic` 活跃区间过滤（#348，退市股不再请求）、导入仅接受 `main_net_inflow` 非 NULL 行；逐股窗口建议在**盘后**运行（交易日数据发布前运行会 0 行并因日历含今日而失败，次日重跑可自愈）
- `dragon`：龙虎榜席位明细（RPT_BILLBOARD_DAILYDETAILSBUY/SELL），按 (symbol, trade_date, seat_type) 聚合
- `block_trade`：大宗交易（RPT_DATA_BLOCKTRADE）
- `institution_survey`：机构调研（RPT_ORG_SURVEYNEW，NOTICE_DATE 过滤）
- `index_daily`：指数/板块日线（官方指数白名单 + THS 行业板块，增量按 `MAX(trade_date)`；merge 导入；`index_type` 仅 `official`/`industry` 两种取值）
- `index_basic`：指数/板块名称表（官方指数 + THS 行业板块，版本快照，全量覆盖导出；`import index_daily` 时伴生写入）

**采集器字符串统一 TRIM（issue #235）**：所有写 Dolt 的采集器在 INSERT SELECT 中
对用户可见文本列统一 `TRIM()`（stock_basic 的 name/board/full_name/industry/region、
fin_indicators 文本列、财务三表文本列、institution_survey 的 org_name/survey_type、
block_trade 的 buyer/seller）。仅去 ASCII 空格（U+0020），全角空格 U+3000 保留
（`TRIM()` 不剥离，Dolt 实证）。Dolt 现库脏数据计数为 0，无需重导；Parquet 为旧
导出快照，若 GUI 仍见旧空格需重新 `export` 刷新。

`compass-collectors fetch stock_basic` 现在走 `crates/compass-collectors/src/stock_basic_official.rs`，
从三大交易所官网（SSE/SZSE/BSE）抓取股票基本信息，输出 `stock_basic_official.csv`（位于
`/data/compass-data/csv/`）。旧的东财采集器 `fetch_stock_basic.py` 已随 Python 采集层退役；
其 EM_FS m:0+t:81 段曾混入 6841 只新三板/老三板股票，不再使用。

### 代理层（proxy_pool，issue #294）

所有 HTTPS 采集默认走本地 proxy_pool（`http://127.0.0.1:5010`）代理：
**proxy-first**——有 https 可用代理必走代理；池空/API 不可达时打印醒目警告并写
`proxy_pool_state.json`（时间戳/池计数/是否降级）后**直连**，绝不因无代理失败。
坏代理自动 `delete` 出池并换下一个（有界重试后直连兜底）。可用环境变量：

- `COMPASS_PROXY_API_URL`：proxy_pool API 基址（默认 `http://127.0.0.1:5010`）
- `COMPASS_PROXY_DISABLE=1`：完全禁用代理层（测试/本地无池时）
- `COMPASS_CSV_DIR`：`proxy_pool_state.json` 所在目录（默认 `/data/compass-data/csv/`）

**保持池温（keepalive）**：`compass-collectors keepalive` 子命令后台常驻循环，每周期
从 freeproxy `json` 源灌 proxy_pool Redis（`use_proxy` hash），
GitHub raw 429/超时自动用本地 `/tmp/freeproxy.json` 快照兜底。
**keepalive 仅 json 单源**——realtime 源（依赖 Python `pyfreeproxy` 库）为 stub
（`run_realtime_cycle` 打印 "realtime source is not yet available in Rust; skipping"），
`--source realtime` 是 **freeproxy** 子命令的 flag（报
"freeproxy: --source realtime is not supported in Rust yet; use --source json"），
keepalive 无 `--source` 参数（B7 已接受的偏差，见 `.dsh/kb/dev/reflections.md`）：

```sh
cargo run -p compass-collectors -- keepalive --once           # 单轮（测试/冒烟）
cargo run -p compass-collectors -- keepalive --interval 600   # 常驻（每 10 分钟一轮）
nohup cargo run -p compass-collectors -- keepalive --interval 600 >> /data/compass-data/csv/proxy_keepalive.log 2>&1 &
```

> compose 部署（`scripts/proxy_pool/docker-compose.yml`）已把 Redis 映射到
> `127.0.0.1:6379`（仅 loopback），上述默认命令可直接使用（issue #296）。

采集器管线的完整描述见 `.dsh/kb/design/architecture.md`。

---

## 典型工作流

### 首次设置（从 Dolt）

```sh
# 1. 从 Dolt investment_data 导入全部数据
cargo run --bin compass-data -- import

# 2. 从 Dolt compass_data 导入公司概况
cargo run --bin compass-data -- import-compass --table stock_basic

# 3. 启动图表应用
scripts/run.sh
```

### 从东方财富获取新数据

```sh
# 1. 将最新数据获取到 Dolt compass_data（Rust 采集器）
cargo run -p compass-collectors -- sync

# 2. 将新表导入到 Parquet
cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-01-01
```

### 备份到百度云

```sh
cargo run --bin compass-data -- backup            # 上传 zip
cargo run --bin compass-data -- backup --keep-zip # 上传后保留本地 zip
```

---

## `sepa` — 东方SEPA 评分（计算 + 写回 Dolt）

对最新交易日运行 SEPA 五模块评分引擎（趋势/题材/资金/形态/风险），打印 TOP 榜并将计算表写回 Dolt `compass_data`（`technical_factor` / `industry_factor` / `capital_factor` / `final_score` / `market_temperature`，两段式 DELETE + `dolt table import -a`，幂等可重跑）。

**写回范围**：`score` 写回全部 5 张计算表（全量通过过滤的排序结果，非仅 TOP-N）；`temperature` 只写 `market_temperature` 一张表，绝不触碰 factor/score 表。

```sh
cargo run --bin compass-data -- sepa score --top 50    # 评分 + TOP50 表格 + 写回全量
cargo run --bin compass-data -- sepa score --top 30 --date 2026-07-31  # 指定日期
cargo run --bin compass-data -- sepa temperature       # 市场温度计 + 只写 market_temperature
cargo run --bin compass-data -- sepa temperature --date 2026-08-14  # 指定日期温度计
cargo run --bin compass-data -- sepa backfill-dates    # 自动补算缺失日期的全部 SEPA 派生表
cargo run --bin compass-data -- sepa backfill-dates --start 2026-08-13 --end 2026-08-25  # 指定窗口
```

`backfill-dates` 以 Parquet 中的交易日为基准，对比 Dolt 计算表已存在的日期，
对每个缺失交易日依次执行 score + temperature 写回（5 张计算表 +
`data_updates`），严格失败一票否决。

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--top` | `50` | 终端表格输出条数上限（不影响 Dolt 写回内容——写回总是全量计算集） |
| `--date` | 数据内最新交易日 | 计算日期（YYYY-MM-DD）；不传时取 Parquet 中最大 trade_date，周末/节假日运行不会写出非交易日行 |
| `backfill-dates --start/--end` | 自动判定 | 回补窗口；不传时自动覆盖 Parquet 中全部交易日 |

每日一键流水线见 `scripts/update-database.sh`：step 0 同步
`investment_data` 上游（`scripts/sync-investment-data.sh`）→ `cargo import`
→ `check-stock-daily` 缺口硬校验 → `compass-collectors sync`
（自动检测并回补日频源数据缺口；0 行日频 import 按交易日历判定 no-op）
→ Dolt commit（含 data_updates）→ import-compass 11 张表
（`stock_basic`/`index_basic` 全量覆盖，其余按锚点增量；data_updates 只读不导出）。
SEPA 派生表不再随每日管线自动计算；需要时手动运行
`compass-data sepa backfill-dates` → `sepa temperature` → `sepa score --top 50`。
**手动运行写回 Dolt 后必须自行提交并推送**（`sepa` 写回的表不在
update-database.sh 的 COLLECTOR_TABLES allowlist 内，脚本不会提交它们）：
`cd /data/compass-data/compass_data && dolt add <表> && dolt commit -m "..." && dolt push origin main`。

### 同步用时统计（issue #334）

每次运行 `scripts/update-database.sh` 会额外记录全链路计时：

- shell 步骤级（step 0~8）与总运行时长；
- `compass-collectors sync` 内每个采集器来源的 fetch/import 阶段耗时；
- 控制台打印人类可读摘要；
- 最终生成一个本地 JSON 文件（用于后续优化对比），不写入 Dolt、不输出 CSV。

```sh
# 默认输出目录
logs/sync-timings/YYYY-MM-DD-<run_id>.json
```

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `SYNC_TIMING_DIR` | `$PROJECT_ROOT/logs/sync-timings` | 最终 JSON 输出目录；测试/临时运行可覆盖 |
| `COMPASS_TIMING_FILE` | 临时 JSONL 文件 | Rust 采集器上报 timing 事件的路径；shell 也向同一文件追加步骤事件，最终合并后生成单个 JSON |

计时是附加能力：写入/合并失败只输出 warning，**不会阻断数据更新主流程**；失败步骤也会以 `status:"failed"` 记录。

### `sepa backtest` — 历史批量回测

逐日重算回测窗口内全市场 SEPA 评分（点内计算，不偷看未来），模拟"每日收盘后按评分取 TOP-N 等权持仓、持有 N 个交易日后换仓"策略，输出绩效指标（累计/年化收益、胜率、盈亏比、最大回撤、换仓次数），并与市值前 300 等权代理基准对比。权益曲线写回 Dolt `backtest_result` 表，也可导出 CSV。

```sh
cargo run --bin compass-data -- sepa backtest                                       # 默认 2025-01-01 至今，TOP50，5 日换仓，成本 0.1%
cargo run --bin compass-data -- sepa backtest --start 2026-07-01 --top 30 --days 10 --cost 0.0005 --csv /tmp/bt.csv
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--start` | `2025-01-01` | 回测窗口起始（YYYY-MM-DD） |
| `--end` | 数据内最新交易日 | 回测窗口结束（YYYY-MM-DD） |
| `--top` | `50` | 每期持仓数量 TOP-N（持仓股票数，非表格打印条数） |
| `--days` | `5` | 持有交易日数（换仓周期） |
| `--cost` | `0.001` | 单边交易成本比例（买入/卖出各收一次） |
| `--csv` | — | 权益曲线 CSV 输出路径（strategy_nav/benchmark_nav 两列） |

**输出**：stdout 摘要指标表（策略累计/年化收益、胜率、盈亏比、最大回撤、换仓次数、基准累计、超额收益、年化超额）；`--csv` 写权益曲线文件；Dolt `backtest_result` 表存每日策略/基准净值曲线（单快照全表替换，幂等可重跑，`data_updates` 同步登记）。

**已知限制**：概念成员/ST 状态为当前快照（历史回测存在轻微前瞻偏差，窗口 2025 起可控，报告中标注）；主力资金流无历史 → 资金模块降级为量价配合+筹码集中（大资金流入归 0）。架构细节与决策记录见 `.dsh/kb/design/backtest.md`。

---

## 排障

### 速率限制（采集器）

东方财富会限制激进请求。Rust 采集器在 `http.rs` 内建 `Throttle` 与有界重试
（`EM_MAX_RETRIES`/jitter），`sync` 串行执行各采集器；未提供 Python 的
`--concurrency`/`--delay-ms` 参数。如仍遇限流，通过环境变量调整代理池或
在 `crates/compass-collectors/src/http.rs` 的节流参数处理。

### Dolt 未找到

```sh
# 验证 Dolt 已安装且 investment_data/ 存在
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
```

### 导入速度慢

导入过程查询 Dolt 6000 多次 — 每次查询约 0.5 秒。总时间由 Dolt 查询速度决定，而非文件 I/O。日期过滤可加速：

```sh
cargo run --bin compass-data -- import --start-date 20240101
```

### 日志

设置 `RUST_LOG` 以获取详细输出：

```sh
RUST_LOG=debug cargo run --bin compass-data -- import
```

日志输出到 stderr 和 `logs/compass.log`（按日滚动）。
