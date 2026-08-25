# 数据管线（CLI）

## 概述

`compass-data` 通过四个子命令管理 A 股 OHLCV 数据：

```
Dolt investment_data ──import─────────► parquet_data/
Dolt compass_data ────import-compass──► parquet_data/
parquet_data/ ────────export──────────► duckdb / csv / parquet-dir
parquet_data/ ────────backup──────────► 百度云（zip）
```

东方财富数据由 Python 采集器（`collectors/`）获取，存入 Dolt `compass_data`，再通过 `import-compass` 导入。Rust CLI 本身从不与东方财富通信。

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
| `--since` | （无） | 增量导入：仅导入 report_date >= since 的数据（YYYY-MM-DD，如 2026-08-21；`import` 命令仍用 YYYYMMDD） |

**指数/板块表（epic #255）**：`index_daily` / `index_basic` 存指数与板块数据
（官方指数 + 概念/行业板块，来源：东财，采集器 `collectors/fetch_index_daily.py`）：

- `index_daily`（指数/板块日线，含 `index_type` 列）：**增量 merge**——按 parquet 侧 PK
  `(symbol, tradedate)` 与既有 parquet 去重合并，`--since` 后新行并入、旧行不丢
  （与 `capital_main_flow` 同款 `import_append_table` 语义）；导出列含
  `adjclose = close` 占位（指数无复权，供 GUI 查询对齐）
- `index_basic`（指数/板块名称表）：**全量覆盖**——每次导入镜像 Dolt 当前状态
  （仿 `concept_member` 版本快照语义），上游删板块即从 parquet 消失

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
- **增量 merge**：校验"不丢数据"——merge 后 parquet 行数 ≥ 旧 parquet 行数，否则报错退出；DuckDB merge 失败走 fallback 时改为**不带 `--since` 的真全量导出**写回（保留历史），并对全量 Dolt COUNT 校验（ref #298）
- **新鲜度（仅 warn，不退出）**：读 `compass_data` Dolt 的 `data_updates.last_report_date`，超过阈值仅告警——财务表（fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow）阈值 120 天；行情表（capital_main_flow/dragon_list/block_trade/institution_survey/concept_member/index_daily/index_basic）阈值 7 天；stock_basic 不检查（其 last_report_date 为 NULL，collectors 写库时不填）
- **⚠️ `--overwrite --since`**：显式覆盖时不再走增量 merge，而是用 `--since` 过滤后的导出**整体替换** parquet；该组合会丢掉过滤条件之外的历史行（与 `import --since` 同义）。无特殊需求应避免同时传这两个 flag（ref #298 外层根因提醒）。

---

## `export` — Parquet → DuckDB

将 Parquet 主数据库导出为 DuckDB 文件（GUI 的读库路径）。

```sh
cargo run --bin compass-data -- export [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--input` | 来自配置 `[parquet].dir` | Parquet 数据目录 |
| `--format` | `duckdb` | 输出格式（当前仅实现 `duckdb`；其他值会警告并跳过） |
| `--output` | `/data/compass-data/compass.duckdb` | 输出路径 |
| `--overwrite` | `false` | 替换已有数据而非跳过 |

### 示例

```sh
# 导出到 DuckDB
cargo run --bin compass-data -- export

# 强制覆盖
cargo run --bin compass-data -- export --overwrite
```

> **注意（ref #176）**：export 经 `fetch_bars` 读取，输出为**前复权价**
> （`factor_i = adjclose_i / close_i` 已烘焙，`adjclose` 列 == close）。
> 如需原始未复权价，请直接读 Dolt `investment_data` 或 Parquet 源文件。

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

## Python 采集器（数据源 → Dolt）

`collectors/` 目录包含 Python 脚本（uv + curl_cffi），从各数据源获取数据并写入原始 CSV（默认输出到 `/data/compass-data/csv/`，可用 `COMPASS_CSV_DIR` 环境变量覆盖），再导入 Dolt `compass_data`。财务表与 SEPA 资金/题材表来自东方财富；`stock_basic` 已切换到三大交易所官网：

```sh
cd collectors/
uv sync                                # 首次：安装依赖

uv run python main.py fetch stock_basic   # 上交所/深交所/北交所官网
uv run python main.py fetch fin_indicators
uv run python main.py fetch main_flow     # SEPA: 主力资金流（push2 当日截面）
uv run python main.py fetch dragon        # SEPA: 龙虎榜席位
uv run python main.py fetch block_trade   # SEPA: 大宗交易
uv run python main.py fetch institution_survey  # SEPA: 机构调研
uv run python main.py fetch concept_member      # SEPA: 概念板块成分
uv run python main.py sync             # 获取 + 导入全部
uv run python main.py sync-investment --restart
```

**抓取进度查询（`progress` 子命令，issue #267）**：6 个一次性写 CSV 的 SEPA 采集器
（main_flow/dragon/block_trade/institution_survey/concept_member/index_daily）抓取期间
实时写 `csv_dir()/<name>.progress.json`（tmp+os.replace 原子写，可安全跨进程读取）。
另一终端可随时查询，CSV 仍保持一次性写入语义：

```sh
uv run python main.py progress                  # 全部采集器进度（人类可读）
uv run python main.py progress dragon           # 单个采集器
uv run python main.py progress --json           # 全部（原始 JSON，供脚本消费）
uv run python main.py progress block_trade --json
```

进度文件在抓取结束后保留（status = `completed` / `failed`，failed 含 error 信息），
可复查上次运行结果。append 型采集器（income/balance_sheet/cash_flow/fin_indicators）
与 stock_basic 不产生进度文件（`progress` 不接受这些 target）。

关键概念：
- **curl_cffi** 用于 TLS 伪装（东方财富反爬虫；BSE 官网需要携带会话 cookie）
- **CSV 作为中间格式**，连接 API 与 Dolt
- **增量机制**：财务三表（fin_balance_sheet/fin_income/fin_cash_flow）与 fin_indicators 使用 **UPDATE_DATE 时间锚点**增量（见下方说明）；SEPA 时间序列表（main_flow/dragon/block_trade/institution_survey）继续用 Dolt `data_updates.last_report_date` 锚点，只抓 `>= 最新已抓报告期` 的窗口；任一天/板块抓取失败即整体中止（不推进 watermark，重跑补全）。财务三表自 ref #202 起改用 **F10 完整版报表**（RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW，203/319/254 字段），自 issue #299 起采用 **UPDATE_DATE 增量 + merge/ODKU 导入**（历史永不丢失、修订覆盖）；fin_indicators + 4 个时间序列表（main_flow/dragon/block_trade/institution_survey）**merge 导入**（CREATE IF NOT EXISTS + INSERT IGNORE 或 ODKU 按 PK 去重）——增量窗口 CSV 追加进已有表，绝不覆盖完整历史；concept_member 是全量重写（版本快照）。长文本表（institution_survey org_name 可达 ~800 字节）与宽表（财务三表 203-319 列超 Dolt `-c` 推断行尺寸上限）用显式宽 schema 建临时表导入（`dolt table import -u`，采集器 `create_sql` 参数），避免 dolt 类型推断按 varchar(200) 字节截断 UTF-8 或 65504 字节行尺寸超限。

**fin_indicators 增量修订检测（issue #135，替代 #27 的 `--refresh N`）**：`fetch_fin_indicators.py --incremental` 改用 **UPDATE_DATE 时间锚点**——filter=`(UPDATE_DATE>='{anchor}')`，锚点 = `min(data_updates.last_updated, state.json last_update_date)`（Dolt 为 source of truth，state.json 兜底；两源皆无则全量 REPORTDATE 枚举）。增量模式忽略 `--years/--periods`（锚点过滤跨报告期，旧报告期修订与新披露一体覆盖）。`_import_fin_indicators` 改 **UPSERT**（`INSERT ... SELECT ... ON DUPLICATE KEY UPDATE`，SELECT 侧全列别名 + ODKU 无前缀别名引用——Dolt 2.2.3 不支持限定源列引用与 `VALUES()`），修订后的行覆盖 Dolt 旧 PK 行；CSV 每次写入后整文件 keep-LAST 去重（键 `(SECURITY_CODE, REPORTDATE)`）。**已知限制**：不做历史回补（锚点前的存量修订不重抓，风险自担）；fetch 与 import 应同日运行（跨日/单独 import 会致锚点超前漏抓间隙修订）；API 侧下架/删除的行不传播到 Dolt（UPSERT 只能覆盖不能删除）。

**财务三表增量修订检测（issue #299）**：`fetch_balance_sheet.py --incremental`（income/cash_flow 同理）改用 **UPDATE_DATE 时间锚点**——filter=`(UPDATE_DATE>='{anchor}')`，锚点 = `min(data_updates.last_updated, state.json last_update_date)`（查 `fin_balance_sheet`/`fin_income`/`fin_cash_flow` 各自的 data_updates 行）。**无 anchor（首跑/无 state/无 data_updates）时固定 `2020-01-01`** 走一次 UPDATE_DATE 全历史拉取，不回退 REPORT_DATE 枚举。增量模式忽略 `--years/--periods`。导入改 `import_replace_table(merge=True)` + **`INSERT ... ON DUPLICATE KEY UPDATE`**（SELECT 侧全列唯一别名 + ODKU 无前缀别名引用），同 `(symbol, report_date)` 修订行覆盖旧值、历史永不丢失；CSV 写入后按 `(SECURITY_CODE, REPORT_DATE)` keep-LAST 去重。state.json 记录 `last_report_date` + `last_update_date`（`total_rows`/`last_run`）。`main.py sync` 已对三表默认 `run(incremental=True)`；手动全量枚举可运行不带 `--incremental` 的 fetch（仍 merge 导入）。**已知限制**：与 fin_indicators 相同——不做锚点前历史回补；fetch 与 import 应同日运行；API 下架/删除行不传播到 Dolt。

SEPA 采集器说明：
- `main_flow`：东财 push2 当日全市场主力资金流截面（f62/f184/f66/f72/f78/f84）；按 (symbol, trade_date) 累积每日截面（merge 导入）
- `dragon`：龙虎榜席位明细（RPT_BILLBOARD_DAILYDETAILSBUY/SELL），按 (symbol, trade_date, seat_type) 聚合
- `block_trade`：大宗交易（RPT_DATA_BLOCKTRADE）
- `institution_survey`：机构调研（RPT_ORG_SURVEYNEW，NOTICE_DATE 过滤）
- `concept_member`：概念板块成分（版本跟踪，全量重写非每日快照；导入时
  `TRIM(BOARD_NAME)` 去除 EastMoney 尾随空格，ref #217 验收）

**采集器字符串统一 TRIM（issue #235）**：所有写 Dolt 的采集器在 INSERT SELECT 中
对用户可见文本列统一 `TRIM()`（stock_basic 的 name/board/full_name/industry/region、
fin_indicators 文本列、财务三表文本列、institution_survey 的 org_name/survey_type、
block_trade 的 buyer/seller）。仅去 ASCII 空格（U+0020），全角空格 U+3000 保留
（`TRIM()` 不剥离，Dolt 实证）。Dolt 现库脏数据计数为 0，无需重导；Parquet 为旧
导出快照，若 GUI 仍见旧空格需重新 `export` 刷新。

`fetch stock_basic` 现在运行 `fetch_stock_basic_official.py`，从三大交易所官网
（SSE/SZSE/BSE）抓取股票基本信息，输出 `stock_basic_official.csv`（位于
`/data/compass-data/csv/`）。旧的东财采集器
`fetch_stock_basic.py` 仍保留但不再用于 stock_basic——其 EM_FS m:0+t:81 段混入
6841 只新三板/老三板股票。原先为东财分页设计的 `--resume` / `--max-pages` 标志已移除。

### 代理层（proxy_pool，issue #294）

所有 HTTPS 采集默认走本地 proxy_pool（`http://127.0.0.1:5010`）代理：
**proxy-first**——有 https 可用代理必走代理；池空/API 不可达时打印醒目警告并写
`proxy_pool_state.json`（时间戳/池计数/是否降级）后**直连**，绝不因无代理失败。
坏代理自动 `delete` 出池并换下一个（有界重试后直连兜底）。可用环境变量：

- `COMPASS_PROXY_API_URL`：proxy_pool API 基址（默认 `http://127.0.0.1:5010`）
- `COMPASS_PROXY_DISABLE=1`：完全禁用代理层（测试/本地无池时）
- `COMPASS_CSV_DIR`：`proxy_pool_state.json` 所在目录（默认 `/data/compass-data/csv/`）

**保持池温（keepalive）**：`collectors/proxy_keepalive.py` 后台常驻循环，每周期
从 freeproxy `json` 快照 + `realtime` 双源灌 proxy_pool Redis（`use_proxy` hash），
GitHub raw 429/超时自动用本地 `/tmp/freeproxy.json` 快照兜底：

```sh
cd collectors/
uv run python proxy_keepalive.py --once              # 单轮（测试/冒烟）
uv run python proxy_keepalive.py --interval 600      # 常驻（每 10 分钟一轮）
nohup uv run python proxy_keepalive.py --interval 600 >> /data/compass-data/csv/proxy_keepalive.log 2>&1 &
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
# 1. 将最新数据获取到 Dolt compass_data（Python 采集器）
cd collectors/
uv run python main.py sync

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
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--top` | `50` | 终端表格输出条数上限（不影响 Dolt 写回内容——写回总是全量计算集） |
| `--date` | 数据内最新交易日 | 计算日期（YYYY-MM-DD）；不传时取 Parquet 中最大 trade_date，周末/节假日运行不会写出非交易日行 |

每日一键流水线见 `scripts/sepa_daily.sh`（行情更新 → 采集 → Dolt commit → Parquet 导入 → 计算 → Dolt commit → TOP50）。

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

东方财富会限制激进请求。在 `collectors/` 中，降低并发并增加延迟：

```sh
uv run python main.py sync --concurrency 1 --delay-ms 3000
```

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
