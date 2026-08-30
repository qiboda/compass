# Plan: fix/backfill-retry-import-history (#342 + #343)

> Worktree: `.worktrees/fix-backfill-retry-import-history`
> Issues: #342（main_flow backfill 无单股重试）/ #343（import-compass --since 增量合并丢失历史）
> 一个 PR 两个 commit 组：`fix:` 一个 (#342)、`fix:` 一个 (#343)，另加 docs/review-fix/reflection commits。

## 背景

1. **#342**：`crates/compass-collectors/src/main_flow.rs::backfill()`（line 336-401）逐股
   循环 `client.get_json_with_headers_and_proxy(...).await?`，无单股重试——2026-08-30
   auto-heal 时对 `bj920837` 一次瞬时连接错误中断整个 `update-database.sh`。
   每日路径 `fetch_symbol_window()`（line 187-227）有 3 次重试（2s/4s 指数退避），backfill 没有。
2. **#343**：`crates/compass-data/src/import_compass.rs::import_append_table()`（line 395-510）
   在 parquet 已存在且传 `--since` 时只导出 `date_col >= since` 切片与旧 parquet 合并。
   auto-heal 补进 Dolt 的**早于 since 的缺失/过期行**既不在增量切片也不在旧 parquet → 永久留缺
   （2026-08-30：capital_main_flow Dolt 118097 / parquet 49885）。且 merge 成功路径
   `COPY (SELECT * ...)` 把 `priority`/`rn` 内部列写进正式 parquet。

## 已锁定 grill 决策（handoff 契约，逐条对应）

| # | 决策 | 落地 |
|---|---|---|
| 1 | 一个 worktree/PR 覆盖 #342 + #343 | 本计划 |
| 2 | #342：单股重试 3 次（与每日路径一致）；重试耗尽后**整批中止（strict）**，错误带失败 symbol；不 skip-and-continue | 见 Issue #342 设计 |
| 3 | #343：merge 前先做安全校验（Dolt vs 旧 parquet 的 `date_col < since` 历史切片对比）；缺失/过期 → 降级全量导出（无 `--since`）；一致 → 快速增量 merge | 见 Issue #343 设计 |
| 4 | #343：清理 `priority`/`rn` 内部列，merge 成功路径不写内部列 | `SELECT * EXCLUDE (priority, rn)` |
| 5 | 验证：单元/集成测试 RED→GREEN + 真实数据定向冒烟 | 见测试计划 |

## Issue #342 — backfill 单股重试 + strict 失败

### 设计

**`crates/compass-collectors/src/error.rs`** — 新增显式错误变体：

```rust
/// A per-symbol Sina backfill fetch failed after exhausting retries; the
/// whole batch is aborted (strict failure, no skip-and-continue).
#[error("backfill: symbol {symbol:?} failed after {attempts} attempts: {source}")]
BackfillSymbolFailed {
    symbol: String,
    attempts: u32,
    source: String,
},
```

**`crates/compass-collectors/src/main_flow.rs`**：

- 新常量（契约，测试断言）：
  - `pub const SINA_BACKFILL_RETRIES: u32 = 3;`
  - `const SINA_BACKFILL_BACKOFF: Duration = Duration::from_secs(2);`
- 新纯函数（可测，替代现循环体内联的解析/过滤逻辑）：

```rust
/// Parse a Sina backfill page into in-range records (pure).
fn extract_backfill_window(symbol: &str, data: &Value, start: &str, end: &str) -> Vec<FlowRecord>
```

- 新通用重试 runner（可测，退避注入：

```rust
/// Retry a single-symbol Sina fetch up to `attempts` times with exponential
/// backoff (`backoff * 2^attempt`); Ok on first success, Err after exhaustion.
async fn retry_sina_backfill<F, Fut>(
    symbol: &str,
    attempts: u32,
    backoff: Duration,
    mut op: F,
) -> Result<Vec<FlowRecord>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<FlowRecord>>>,
```

  - 退避公式与 `fetch_symbol_window` 一致：attempt 0 → 2s，attempt 1 → 4s（`backoff * (1u64 << attempt) as u32`）。
  - 每次失败（`attempt + 1 < attempts`）打印 `retry n/3 for {symbol} in {wait:?}: {e}` 并 sleep；耗尽后返回
    `CollectError::BackfillSymbolFailed { symbol, attempts, source: e.to_string() }`。
- `backfill()` 逐股循环改为调用 `retry_sina_backfill(...)` 并以 `?` 传播（strict abort）：
  closure 内 = `throttle.acquire().await` + 组 params（page/num/SINA_BACKFILL_NUM/sort/asc/daima）+ fetch +
  `extract_backfill_window` 结果。现有 `seen` 去重、`seen.is_empty() → Err`、排序写 CSV 全部保留；
  任一 symbol 失败 → 函数 Err，**不写任何部分 CSV**（write_csv 仍在循环后）。

### 验收（#342）

1. 每日路径 `fetch_symbol_window` 行为不变（不动它）。
2. 瞬时失败（前 2 次）→ 重试后成功，CSV 正常写出。
3. 3 次全败 → `BackfillSymbolFailed` 携带 symbol，整批中止（sync/auto-heal 照常失败并打出 symbol）。
4. 退避序列 2s/4s 与每日路径一致。

## Issue #343 — 增量 merge 历史一致性校验 + 内部列清理

### 设计

**`crates/compass-data/src/import_compass.rs`**：

1. `AppendTableSpec` 新增字段：

```rust
/// Parquet-side date column that carries `date_col` after the export rename
/// (e.g. index_daily `trade_date` → `tradedate`). `None` → `date_col`.
parquet_date_col: Option<&'a str>,
```

   7 个构造点更新（index_daily 传 `Some("tradedate")`，其余 `None`）。

2. 新增校验函数：

```rust
/// #343: verify the old parquet mirrors Dolt for all rows older than `since`
/// before trusting the incremental merge. Ok(true) = identical row sets;
/// Ok(false) = divergent/unreadable → callers must fall back to full export.
fn incremental_history_matches(
    dolt_dir: &Path, path: &Path, table_name: &str,
    date_col: &str, parquet_date_col: &str, select_cols: &str, order_cols: &str,
    since: &str,
) -> Result<bool, Box<dyn std::error::Error>>
```

   - Dolt 侧：`SELECT {select_cols} FROM {table_name} WHERE {date_col} < '{since}' ORDER BY {order_cols}`
     经 `run_dolt_sql_parquet` 导出到 `unique_work_path("{table}.hist")`（临时 parquet，结束删除）。
   - DuckDB 内存中双向 EXCEPT 计数之和：
     `(SELECT * FROM read_parquet(hist)) EXCEPT (SELECT * FROM read_parquet(path) WHERE {parquet_date_col} < '{since}')`
     与反向；任一非 0 → `Ok(false)`（Dolt 多/少、数值过期、旧 parquet 多余行都算发散）。
   - 读取/类型错误（旧 parquet 不可读等）→ `warn!` 并 `Ok(false)`（保守 → 全量导出修复；与现有
     "corrupt parquet 触发 fallback" 语义一致）。SQL 拼接本身无用户输入（since 已过 validate_since_arg）。
   - 列名/类型对齐依据：生产 parquet 与 hist 均由同一 `run_dolt_sql_parquet` + 同一 select_cols 生成。

3. 抽取共用回退（现 merge-失败分支，line 456-481 重构抽出）：

```rust
/// Full-export recovery: preserve pre-merge parquet for diagnosis, run the
/// unfiltered export, validate against the full Dolt row count.
fn recover_full_export(dolt_dir: &Path, path: &Path, table_name: &str,
                       select_cols: &str, order_cols: &str)
    -> Result<(), Box<dyn std::error::Error>>
```

4. 历史一致性检查插入点（**实现位置修正**，2026-08-31 对抗测试阶段确认——必须早于
   tiny-data skip）：

   `import_append_table` 的 tiny-data skip（`if new_data.len() < 500 { warn!(...); return Ok(()); }`）
   位于 date_filter 导出**之后**、merge 分支**之前**。auto-heal 只回补早于 since 的历史行时，
   Dolt 的 `date_col >= since` 切片可为 0 行/极小（<500B parquet 字节）→ 当前实现直接 skip
   早退、不合并不改写 → 历史分叉永久留在 parquet（恰为 #343 现象）。因此检查必须放在
   `effective_since` 计算之后、date_filter 导出与 skip 之前：

```rust
// after effective_since computation, before date_filter/export:
if effective_since.is_some() && !overwrite && path.exists() {
    if !incremental_history_matches(...)? {        // divergent / unreadable
        warn!("{table_name}: history divergence before --since merge; "
              "falling back to full export");
        recover_full_export(...)?;                 // 含备份 + 全量 row-count 校验
        info!("  → {}", path.display());
        return Ok(());
    }
}
```

   随后 date_filter 导出 + tiny-data skip + merge 分支仅保留合并逻辑（检查不再重复运行）。
   代价：每次增量 import 多一次 `< since` 历史切片导出 + 双向 EXCEPT；与决策 1
   （成本一次性历史切片导出可接受）一致。

5. merge SQL 外层 `SELECT *` → `SELECT * EXCLUDE (priority, rn)`（DuckDB ≥0.7，本项目 1.10505.0）：
   `COPY (SELECT * EXCLUDE (priority, rn) FROM (SELECT *, ROW_NUMBER() OVER (... ) AS rn FROM (...) WHERE rn = 1 ORDER BY {partition_cols}) TO ...`
   —— 保持 rn 用于 WHERE，仅输出时剔除；`priority`/`rn` 不进正式 parquet。
   现有 row-count 守卫（merged < old → Err）、DuckDB 失败 fallback 全部保留。

### 验收（#343）

1. 历史一致 → 快速增量 merge，最终 parquet 行集 == Dolt（无内部列）。
2. auto-heal 补入早于 since 的缺失行 → 校验检出 → 全量导出，parquet 与 Dolt 对齐（行数/内容）。
3. 同 key 过期值（stale）→ 检出 → 全量导出修复。
4. 旧 parquet 不可读 → 全量导出恢复（不静默）。
5. merge 成功路径与全量路径 parquet 均无 `priority`/`rn` 列。
6. index_daily（重命名 `tradedate`）同样适用。

## 测试计划（门禁 3.5/4 — RED 先于实现）

### 3.5 对抗性测试（委派 subagent_skwy_adversarial_test）

- `retry_sina_backfill_succeeds_after_transient_errors`：op 先败 2 次后成功 → Ok，调用次数 3。
- `retry_sina_backfill_exhaustion_names_symbol`：全败 → `BackfillSymbolFailed { symbol, attempts: 3 }`，message 含 symbol。
- `retry_sina_backfill_exponential_backoff_sequence`：注入短退避（如 10ms），实际 elapsed ≥ 10+20ms（3 次尝试、2 次等待）。
- `sina_backfill_retries_constant_is_three`：`SINA_BACKFILL_RETRIES == 3`（与每日路径一致）。
- #343：`incremental_merge_corrupt_parquet_falls_back_to_full_export`（写坏 parquet → 全量修复且与 Dolt 对齐）。
- #343：`incremental_merge_extra_parquet_historical_row_falls_back`（旧 parquet 有多余历史行 → 全量导出删除）。
- #343：`incremental_merge_empty_dolt_history_is_fast_path`（Dolt 无 < since 行且旧 parquet 亦无 → 快速 merge，不落 fallback）。
- #343：`incremental_merge_index_daily_renamed_date_column`（tradedate 变体：重命名列上历史缺失 → 修复）。

### 4 需求验收测试（委派 subagent_skwy_requirement_test）

- #342：`retry_sina_backfill` 契约 + `extract_backfill_window` 范围过滤/坏行跳过（纯函数，现有 parse 语义保留）。
- #343：`incremental_merge_repairs_auto_healed_history`（Dolt 插入旧日期行 + `--since` import → parquet 含该行且与 Dolt 对齐）。
- #343：`incremental_merge_repairs_stale_history`（同 key 不同值 → 对齐 Dolt）。
- #343：`incremental_merge_fast_path_no_internal_columns`（历史一致 → 结果 == Dolt 且无 priority/rn 列；RED 当前实现留列）。
- #343：`incremental_merge_fast_path_keeps_new_rows`（since 后新行并入、旧行不丢、prefer_new 语义不变）。

## 文档同步（门禁 5b）

| 文件 | 变更 |
|---|---|
| `.dsh/kb/user/cli.md` | import-compass `--since` 行 + 增量 merge 校验说明：历史切片一致性校验、发散自动降级全量导出、内部列清理 |
| `.dsh/kb/design/data-providers.md` | merge 语义描述更新（#343 校验 + EXCLUDE）+ 决策记录表追加 2 行（#342 backfill 重试策略；#343 校验-降级策略） |
| `.dsh/kb/dev/toolchain.md` | #342/#343 条目补「处理」：已修复于本 PR（PR 号于创建后回填） |
| `.dsh/kb/design/architecture.md` | 审查后定；预计无需变更（逻辑局部） |

## 决策记录（门禁 5c）

- `data-providers.md` 已有 `## 决策记录`（line 513）→ 追加。
- 决策 1：#343 校验粒度 = 全行集合双向 EXCEPT（覆盖 missing/stale/多余/删除四类发散），
  而非仅 key 存在性——key 相同但值过期同样造成 Dolt/parquet 分歧，必须修复；成本一次性历史切片导出可接受。
- 决策 2：#342 用泛型 retry runner + 退避注入（而非复制 fetch_symbol_window 循环）——
  使重试策略可单元测试（网络路径不可测），生产退避与每日路径同公式同常量语义。

## 实现修正记录（review 后补，与 588b71a amendments 一并追溯）

- #342 契约漂移：`BackfillSymbolFailed` 错误字段设计稿为 `source`，实现为 `reason`——
  thiserror 2.0.19 会把字面名为 `source` 的字段自动当作 error source 处理（要求实现
  `std::error::Error`），`String` 不满足 → 编译失败；改名 `reason` 且 Display 措辞
  保持 `failed after {attempts} attempts: {reason}`（落盘测试契约不变）。
  review 发现 588b71a amendment 未记录此漂移，现补。
- #343 review P1-1 修复：`incremental_history_matches` 写 hist 前补
  `create_dir_all(temp_dir()/compass_parquet_work)`——目录缺失时 hist 写失败 → 保守
  降级 → 增量语义永久退化（且备份同样失败）；现置于检查块前，merge 分支的创建保留为防御。
- #343 review P2-2 决策：`pre_merge_backup` 备份**不自动轮转**——诊断文件的价值高于
  /tmp 占用，依赖系统临时清理；避免删除其它进程/唯一诊断记录（注释已记录）。
- #343 review P2-1 测试竞态：no-fallback 断言测试与同 stem 的 fallback 测试（均写
  `/tmp/compass_parquet_work` 的 pid 前缀 backup）在并行 cargo test 下窗口重叠 →
  引入 stem 级静态 Mutex（capital_main_flow / index_daily 各一）串行化相关测试。

## 实现后

1. `cargo test`（workspace）/ `cargo clippy -- -D warnings` / `cargo fmt --check` 全绿。
2. **真实数据定向冒烟**（提交前）：`import-compass --table capital_main_flow --since <锚点>`
   验证历史一致 → 快速 merge、无 priority/rn、行数 == Dolt；不跑 ~1h 全量 update-database.sh。
3. commit → 五角度 review → 修复（≤2 轮）→ rebase origin/master → skwy-reflect → 用户确认 push →
   push + PR → 合并后 issue 收尾（完成 comment + close）。

## 验证波（F1-F4，2026-08-31）

- **F1 evidence**：`.dsh/evidence/ref-342-343-backfill-retry-import-history.md` 已落盘，
  与 HEAD 82e8f2a 一致（8 commits，逐条列明）。实现 commit 完成后一次性写，无中途过期。
- **F2 审查**：两轮共 4 次 review（#342 0a9ad431/2905da88、#343 c25d61a0/8ec02954），
  无 P0/P1；全部 P2/P3 采纳并修复于 1000998/82e8f2a，无需第 3 轮。
- **F3 测试**：compass-collectors 98 lib、compass-data 113 lib + 108 bin + 37 集成全绿；
  fmt/clippy 干净；workspace `just check` 结果见 evidence。
- **F4 scope fidelity**：plan 验收逐条核对通过（#342 5 条、#343 6 条），
  详见 evidence F4 节；无范围外改动。
