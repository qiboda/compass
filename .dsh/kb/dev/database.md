# 数据库开发信息

本文件是 compass 数据库相关开发操作的**权威文档** — 同步、提交、生成、
查询、布局。覆盖本地 `/data/compass-data/` 下的两个 Dolt 仓库（
`investment_data` 与 `compass_data`）、Parquet 与 DuckDB 产物。

> 跨库查询示例与表结构说明原位于 `.dsh/kb/dev/process.md`，已迁移至此
> （ref #157）。process.md 仅保留索引。

## 数据库布局总览

```
/data/compass-data/
├── investment_data/     # Dolt 仓库 — 只读第三方行情数据（上游 chenditc）
├── compass_data/        # Dolt 仓库 — 自有数据（公司概况/财务/SEPA 采集）
├── parquet_data/        # Parquet 文件 — GUI 实际读取的数据源
├── compass.duckdb       # DuckDB 数据库（export 产物，可选）
└── compass.duckdb.wal   # DuckDB WAL
```

### investment_data（只读，第三方）

- 上游：`origin` → `https://doltremoteapi.dolthub.com/chenditc/investment_data`
- 个人 fork：`skwy` → `https://doltremoteapi.dolthub.com/skwy/investment_data`
- 分支：`master`
- 核心表：`final_a_stock_eod_price`（18M+ 行，`(tradedate, symbol)` 主键）、
  `ts_a_stock_list`、`ts_trade_day_calendar` 等 14 张表
- **只读**：只从上游拉取，不直接修改；本地与 fork 仅是镜像

### compass_data（自有，可修改）

- 上游：`origin` → `https://doltremoteapi.dolthub.com/skwy/compass_data`
- 分支：`main`
- 18 张表，分三类：

| 类别 | 表 | 说明 |
|---|---|---|
| 基本面 | `stock_basic`、`fin_indicators`、`fin_balance_sheet`、`fin_income`、`fin_cash_flow` | 公司概况与三大报表 |
| SEPA 采集 | `block_trade`、`capital_main_flow`、`dragon_list`、`index_daily`、`index_basic`、`institution_survey` | 龙虎榜/大宗/主力资金/指数与板块日线/指数与板块名称表/机构调研 |
| 计算产物 | `final_score`、`market_temperature`、`capital_factor`、`industry_factor`、`technical_factor`、`data_updates`、`backtest_result` | SEPA 评分与因子输出、抓取状态、`sepa backtest` 回测净值曲线 |

> `backtest_result`（issue #327）：`sepa backtest` 写回的每日策略/基准净值曲线
> （单快照全表替换，幂等可重跑），schema 见 `crates/compass-data/src/backtest.rs`。

**data_updates 表（抓取/计算状态登记）**：schema 权威定义见
`crates/compass-data/src/sepa.rs:73-79`（table_name PK + last_updated + source +
row_count + last_report_date）。消费方：

- **采集器增量锚点**（`crates/compass-collectors/src/dolt.rs`）：大多数采集器以
  `last_report_date` 为增量起点，只抓 `>= 最新已抓报告期` 的窗口；**财务三表
  （`fin_balance_sheet` / `fin_income` / `fin_cash_flow`，issue #299）与
  `fin_indicators` 的增量路径**改用 `UPDATE_DATE` 锚点——解析规则为
  `min(data_updates.last_updated, state.json.last_update_date)`（
  `csv_dir()/{REPORT_NAME}.state.json`），双源皆缺时固定 `2020-01-01`
  全历史拉一次，以捕获历史修订并减少全量拉取；`last_report_date` 仍由
  import 写入，供新鲜度校验使用。
- **update-database.sh 增量锚点**（`scripts/update-database.sh` step 2/4）：step 0 先同步
  `investment_data` 上游（`scripts/sync-investment-data.sh`）；step 2 由
  `compass-collectors sync` 统一刷新全部 11 张 `compass_data` 表 + `data_updates`，
  并在开头自动检测/回补日频源表缺口（`ts_trade_day_calendar` 对比 Dolt 现有交易日）；
  step 2 内 4 张日频表的 0 行 import 按交易日历判定 no-op（#338）；
  step 4 对**逐表读取**各表自身的 `last_report_date`（含 `fin_*` 财务表与
  `index_daily`）；缺失/NULL 锚点的表走全量导入，不再用全局 MAX 锚点；
  `stock_basic` 与 `index_basic` 是版本快照/权威表，始终全量覆盖，不查询锚点；
  `data_updates` 仅由 step 2 写入、step 3 提交，step 4 跳过（不导出）；
  import 后还会用 `check-stock-daily` 对 Parquet 交易日历硬校验缺失；
  SEPA 派生表（`sepa backfill-dates`/`temperature`/`score`）不再自动计算，
  手动运行 `compass-data sepa …` 子命令
- **import-compass 新鲜度校验（ref #136）**：导入后读 `last_report_date`，过期
  仅 warn 不退出（财务表 120 天 / 行情表 7 天 / stock_basic 不检查）

`last_report_date` 语义（采集器写库时按表类填写）：`fin_*` 财务表
（fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow）= `MAX(report_date)`；
行情表 capital_main_flow/dragon_list/block_trade/index_daily = `MAX(trade_date)`、
institution_survey = `MAX(survey_date)`；index_basic = `CURDATE()`；
stock_basic = NULL（写库只填 4 列，见 `crates/compass-collectors/src/stock_basic_official.rs`）。

**运行统计（issue #334）**：`scripts/update-database.sh` 每次运行在
`logs/sync-timings/` 下生成一个 JSON 计时文件（`SYNC_TIMING_DIR` 可覆盖），
记录 run 元信息、step 0~4 耗时和 `compass-collectors sync` 各来源 fetch/import
耗时；测试/临时运行可设置 `COMPASS_TIMING_FILE` 指定 Rust 上报文件。计时失败仅
warning，不写入/不修改任何 Dolt 表。

## investment_data 同步（pull → push → import）

investment_data 是第三方只读库，**每次使用前都应同步**：从 chenditc 上游
拉取最新行情，同步到自己的 skwy fork，再重新生成 Parquet 供 GUI 使用。

```sh
cd /data/compass-data/investment_data

# 1. 拉取上游最新数据（含 fetch + fast-forward）
dolt pull origin master

# 2. 同步到个人 fork（备份 + 其他机器可用）
dolt push skwy master

# 3. 检查数据新鲜度（应等于最近交易日）
dolt sql -q "SELECT MAX(tradedate) AS latest FROM final_a_stock_eod_price"
```

同步后必须重新生成 Parquet，否则 GUI 仍读旧数据。**注意：`import` 总是
全量直写**——`--since` 只做日期过滤并**覆盖整个文件**（非追加），
用 `--since` 同步会导致历史数据丢失（见 `.dsh/kb/dev/toolchain.md` 排查卡）：

```sh
cd /data/codes/compass
cargo run --bin compass-data -- import   # 全量重建 stock_daily.parquet（推荐）
```

> **为什么 push 到 skwy？** investment_data 是 chenditc 的只读上游仓库，
> 本地机器只有一份。push 到自己的 fork（`skwy`）既是备份，也让其他机器
> /CI 能从 `skwy` 拉取。AGENTS.md 中"每次数据变更后 commit & push"的
> 规则适用于 `compass_data`；`investment_data` 的"变更"来自上游，同步动作
> 就是 pull + push 到 fork。

### 状态检查

```sh
# 本地是否落后上游（非空说明有未拉取的更新）
dolt log --oneline HEAD..origin/master

# 本地与 fork 是否同步
dolt log --oneline skwy/master..master   # 非空 = 有未 push 的本地 commit
dolt log --oneline master..skwy/master   # 非空 = fork 领先（不应发生）
```

## compass_data 提交推送

自有数据每次修改后（import-compass、SEPA 采集、schema 变更、data_updates
更新、CLI/程序写回如 `sepa backtest` 的 `backtest_result`）都必须**及时**
提交并推送——写库完成后立即收尾，禁止数据滞留工作区（ref #190 教训：
`sepa backtest` 写回后未 commit，backtest_result 384 行滞留一天）：

```sh
cd /data/compass-data/compass_data
dolt status                            # 确认变更范围
dolt add <table>...        # or `dolt add .`
dolt commit -m "feat: ..." # describe the data change
dolt push origin main
dolt status                            # 确认工作区干净、与 origin 同步
```

**程序写回路径同样受约束**：任何代码向 `compass_data` 写表
后，流程必须在同一 session 内执行 `dolt commit` + `dolt push`（手动命令或
内置到 CLI 的收尾步骤），不得只写数据不提交。

### Dolt CLI 方言注意（#348 实测）

dolt 与 git 命令族有若干方言差异，按 git 习惯直用会踩坑：

- `dolt status` 不支持 `--short`（git 习惯会报 usage 错）——用 `dolt status`。
- `dolt config` 不支持 `--data-dir`，且 `--local` 必须在仓库目录内
  （`current_dir` 指向 repo）执行；写测试临时库身份时严禁 `--global`
  （污染宿主 `~/.dolt/config_global.json`，见 toolchain.md 排查卡）。
- `dolt sql -r csv` 输出语义：真 NULL = 空字段；字符串 "NULL" = 字面值
  （无引号）；空串 = `""`。解析 CSV 需区分三者。
- `dolt table import -c` 把 CSV 空字段导入为 NULL（MySQL 语义）。
- `CAST('' AS DATE)` 返回 NULL（MySQL 语义）——日期 SQL 判断空串必须
  显式 `col = ''`，不能依赖 `IS NULL OR CAST(...)<=x` 覆盖。
- `dolt push` 到 dolthub 远程较慢（>60s 常见）：用后台 job + ≥300s 超时，
  不要前台默认 60s 超时（会 SIGTERM 被杀）。

## Parquet / DuckDB 生成

GUI 只读本地 Parquet（DuckDB 查询），数据管线命令在 `compass-data` bin：

```sh
cargo run --bin compass-data -- import                    # investment_data → Parquet（全量直写，推荐）
cargo run --bin compass-data -- import --since 20260725   # ⚠️ 日期过滤直写：仅导出 since 后数据并覆盖全文件，非追加（慎用）
cargo run --bin compass-data -- import-compass --table stock_basic  # compass_data → Parquet（--since 有 merge）
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

- `import-compass`/`export` 默认 merge/skip，`--overwrite` 覆盖
- `import` 总是全量直写
- SEPA 采集表（`block_trade` 等）通过 `import-compass` 生成对应 Parquet

完整选项见 `.dsh/kb/user/cli.md`。

## 常用维护查询

### Dolt 查询（investment_data，只读第三方）

```sh
cd /data/compass-data/investment_data
dolt sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
dolt sql -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001' ORDER BY tradedate DESC LIMIT 5"
dolt sql -q "SELECT * FROM ts_a_stock_list LIMIT 5"

# 新鲜度检查
dolt sql -q "SELECT MAX(tradedate) AS latest, COUNT(*) AS row_cnt FROM final_a_stock_eod_price"
```

### Dolt 查询（compass_data，自有数据）

```sh
cd /data/compass-data/compass_data
dolt sql -q "SELECT * FROM stock_basic WHERE symbol='SH600519'"
dolt sql -q "SELECT * FROM fin_indicators WHERE symbol='SH600519' ORDER BY report_date DESC"
```

### 跨库 JOIN（从父目录运行 dolt sql 启用跨库查询）

```sh
cd /data/compass-data
dolt sql -q "
SELECT sb.name, sb.industry_l1, ts.list_date
FROM compass_data.stock_basic sb
JOIN investment_data.ts_a_stock_list ts ON sb.ts_code = ts.ts_code
"

dolt sql -q "
SELECT sb.name, fi.report_date, fi.revenue / 1e8 AS rev_yi, fi.eps
FROM compass_data.stock_basic sb
JOIN compass_data.fin_indicators fi ON sb.symbol = fi.symbol
JOIN investment_data.final_a_stock_eod_price e ON sb.symbol = e.symbol
WHERE sb.symbol = 'SH600519'
ORDER BY e.tradedate DESC
LIMIT 3
"
```

### 跨表财务分析（compass_data 内 JOIN）

```sh
dolt sql -q "
SELECT sb.name, bs.report_date,
  bs.TOTAL_ASSETS / 1e8 AS total_assets_yi,
  inc.TOTAL_OPERATE_INCOME / 1e8 AS revenue_yi,
  cf.NETCASH_OPERATE / 1e8 AS operating_cf_yi
FROM compass_data.stock_basic sb
JOIN compass_data.fin_balance_sheet bs ON sb.symbol = bs.symbol
JOIN compass_data.fin_income inc ON bs.symbol = inc.symbol AND bs.report_date = inc.report_date
JOIN compass_data.fin_cash_flow cf ON bs.symbol = cf.symbol AND bs.report_date = cf.report_date
WHERE sb.symbol = 'SH600519'
ORDER BY bs.report_date DESC
LIMIT 3
"
```

### 用 DuckDB 查询 Parquet

本机未安装 `duckdb` CLI，用 python `duckdb` 模块（已验证可用）：

```sh
python3 -c "
import duckdb
c = duckdb.connect('/data/compass-data/compass.duckdb', read_only=True)
print(c.execute(\"SELECT * FROM stock_daily WHERE symbol='SH600519' ORDER BY date DESC LIMIT 5\").fetchall())
"
```

Rust 代码内调试（`duckdb` crate，内存连接）：

```rust
use duckdb::Connection;
let conn = Connection::open_in_memory()?;
conn.execute_batch("SELECT * FROM read_parquet('parquet_data/stock_daily.parquet') WHERE symbol = 'SH600519' LIMIT 5")?;
```

### 检查 Parquet 文件

```sh
ls -lt /data/compass-data/parquet_data/ | head    # 文件时间 = 生成时间
wc -l /data/compass-data/parquet_data/stock_daily.symbols.txt  # symbol count
python3 -c "
import pyarrow.parquet as pq
f = pq.ParquetFile('/data/compass-data/parquet_data/stock_daily.parquet')
print('rows:', f.metadata.num_rows)
"
```

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| investment_data 同步目标 | 仅 pull 上游 / pull + push 到 skwy fork | pull + push skwy | 本地单份拷贝，fork 作备份且供其他机器/CI 拉取；AGENTS.md 数据变更 push 规则的精神延伸 | 仅 pull 无法异地恢复；上游 chenditc 只读不可 push |
| database.md 与 process.md 关系 | 全量并入 process.md / 新建独立文件 + 迁移查询章节 | 新建独立文件，查询章节迁移 | 维护/同步是独立主题域，独立文件便于导航；避免同主题两处维护漂移 | 并入 process.md 使其臃肿且查询/维护混杂 |
| compass_data 表分类 | 不分类 / 按来源分类 | 按基本面/SEPA 采集/计算产物三类 | 18 张表来源与用途各异，分类便于理解数据管线 | 不分类则新贡献者难以判断表来源 |
