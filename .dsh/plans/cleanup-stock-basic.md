# cleanup-stock-basic - Work Plan

## TL;DR (For humans)

**What you'll get:** 删除 stock_basic 遗留旧 schema 路径——duckdb.rs 旧 StockBasic 结构、import 命令的 5 列 stock_basic.parquet 导出、export 工具对 stock_basic 表的引用，并同步 4 处 kb 文档。`import` 不再生成会覆盖新 10 列 parquet 的错误列文件。

**Why this approach:** 旧路径零生产消费者（GUI 走 ParquetReader，CLI 导出只写 stock_daily），保留只会与 model.rs 新结构长期不一致；整体删除比"禁止覆盖/同步 schema"更彻底，且改动最小。

**What it will NOT do:** 不改 model.rs / parquet.rs / import_compass.rs 生产新路径；不新增依赖；不触碰 #96 相关文件。

**Effort:** Short
**Risk:** Low - 删除类变更，生产路径不受影响；覆盖风险：删除已测代码可能使覆盖率略降，需 cargo llvm-cov 验证 ≥80%
**Decisions to sanity-check:** duckdb.rs stock_basic 表 DDL 整体删除（而非同步新 schema）；import_dolt.rs stock_basic 导出整体删除（而非禁止覆盖）

Your next move: 执行 gate 3（RED 测试）→ 4a/4b → 实现 → 单 commit `ref #80` → review → reflect。

---

> TL;DR (machine): Short effort, Low risk — delete legacy stock_basic paths (duckdb DDL+struct+methods, import_dolt export, export TABLES entry) + sync 4 kb docs; single commit ref #80.

## Scope
### Must have
- C1: duckdb.rs 删 SCHEMA_SQL 中 stock_basic 表 DDL（L104-113）+ 本地 StockBasic struct（L599-610）+ upsert_stock_basic（L457-499）+ get_stock_basic（L501-538）+ 测试 upsert_and_get_stock_basic（L1328-1359）+ get_stock_basic_returns_none_for_unknown（L1361-1369）
- C2: import_dolt.rs 删 stock_basic 导出段（L122-134）；测试 run_completes_when_legacy_stock_daily_dir_exists（L933-941）改断言 stock_basic.parquet 不存在（RED→GREEN）
- C3: export.rs TABLES 删 ("stock_basic", "ORDER BY symbol")（L81）；测试 L175 列表移除 "stock_basic"；保留 export_all_tables
- C4: 文档 4 处：gui.md L131 / architecture.md L328 / testing.md L104 / data-providers.md L242-244 + DuckDB DDL 章节删 stock_basic 表
- C5: 单 commit ref #80 → /review-work → /reflect
### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不改 model.rs / parquet.rs / import_compass.rs（生产新路径，参考不改）
- 不改 main.rs
- 不新增任何 Rust 依赖
- 不触碰 #96 相关文件（worktree skill / open-worktrees.sh / AGENTS.md worktrees 章节）
- 不引入 #[allow(dead_code)] / #[deprecated] 掩盖

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD（RED→GREEN）+ 现有 rstest/tokio::test 框架
- Evidence: .omo/evidence/cleanup-stock-basic/

## Execution strategy
### Parallel execution waves
- Wave 1: T1（RED 测试）+ T2（C1 duckdb.rs）
- Wave 2: T3（C2 import_dolt.rs）+ T4（C3 export.rs）
- Wave 3: T5（C4 文档 4 处）
- Wave 4: T6（全量验证 + commit + review）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 RED 测试 | - | T3 | T2 |
| T2 duckdb.rs | - | - | T1 |
| T3 import_dolt.rs | T1 | T6 | T4 |
| T4 export.rs | - | T6 | T3 |
| T5 文档 | - | T6 | T3/T4 |
| T6 验证+commit | T1-T5 | - | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. T1-RED: import_dolt.rs 测试改断言 stock_basic.parquet 不存在（TDD RED）
  What to do / Must NOT do: 修改 `run_completes_when_legacy_stock_daily_dir_exists`（L933-941）——断言输出目录中 `stock_basic.parquet` 不存在（当前实现会创建 → 测试失败 RED）。不改生产代码。Must NOT: 不提前删除导出段。
  Parallelization: Wave 1 | Blocked by: - | Blocks: T3
  References: crates/compass-data/src/import_dolt.rs:933-941, 122-134
  Acceptance criteria (agent-executable): `cargo test -p compass-data import_dolt` 该测试失败（RED 证据）
  QA scenarios: 运行 `cargo test -p compass-data -- import_dolt` — 失败输出捕获到 .omo/evidence/cleanup-stock-basic/task-1-red.txt（happy: 测试红）
  Commit: N（与实现合并提交）

- [ ] 2. T2-C1: duckdb.rs 删除旧 StockBasic 路径
  What to do / Must NOT do: 删 SCHEMA_SQL 中 stock_basic 表 DDL（L104-113）、本地 `StockBasic` struct（L599-610）、`upsert_stock_basic`（L457-499）、`get_stock_basic`（L501-538）、测试 `upsert_and_get_stock_basic`（L1328-1359）+ `get_stock_basic_returns_none_for_unknown`（L1361-1369）。Must NOT: 不动 DuckDbProvider 其他方法（fetch_bars/save_stock_daily/DDL 其余表）；不加 #[allow(dead_code)]。
  Parallelization: Wave 1 | Blocked by: - | Blocks: T6
  References: crates/compass-core/src/data/duckdb.rs:104-113, 457-538, 599-610, 1328-1369
  Acceptance criteria (agent-executable): `cargo test -p compass-core` 通过；`cargo clippy -p compass-core -- -D warnings` 通过
  QA scenarios: `cargo test -p compass-core` + `cargo clippy -p compass-core -- -D warnings` — 全绿
  Commit: N

- [ ] 3. T3-C2: import_dolt.rs 删除 stock_basic 导出段（GREEN）
  What to do / Must NOT do: 删 L122-134 导出段（含 `basic_path` 写入与 info 日志）；保留其余导出流程。Must NOT: 不动 stock_daily 导出；不留下未使用变量。
  Parallelization: Wave 2 | Blocked by: T1 | Blocks: T6
  References: crates/compass-data/src/import_dolt.rs:122-134
  Acceptance criteria (agent-executable): `cargo test -p compass-data -- import_dolt` 全绿（T1 测试由红转绿）
  QA scenarios: `cargo test -p compass-data` — 全绿（happy）；`cargo clippy -p compass-data -- -D warnings`（failure: 无警告）
  Commit: N

- [ ] 4. T4-C3: export.rs 删 TABLES 中 stock_basic 条目 + 测试断言
  What to do / Must NOT do: TABLES 常量删 `("stock_basic", "ORDER BY symbol")`（L81）；测试 `export_all_tables_creates_parquet_files`（L175）empty_table 列表移除 `"stock_basic"`；保留 `export_all_tables` 函数与其余 3 表条目。Must NOT: 不删函数本身；不动 run_export。
  Parallelization: Wave 2 | Blocked by: - | Blocks: T6
  References: crates/compass-data/src/export.rs:78-83, 127-215
  Acceptance criteria (agent-executable): `cargo test -p compass-data export` 通过
  QA scenarios: `cargo test -p compass-data` — 全绿
  Commit: N

- [ ] 5. T5-C4: 同步 4 处 kb 文档
  What to do / Must NOT do: ① kb/user/gui.md L131 移除 `import` 来源；② kb/design/architecture.md L328 移除 stock_basic.parquet；③ kb/dev/testing.md L104 `get_stock_basic` 示例换 `get_stored_range`；④ kb/design/data-providers.md L242-244 import 管线图仅 stock_daily + DuckDB DDL 章节删 stock_basic 表。Must NOT: 不改其他 kb 文件；不新增章节。
  Parallelization: Wave 3 | Blocked by: - | Blocks: T6
  References: kb/user/gui.md:131, kb/design/architecture.md:328, kb/dev/testing.md:104, kb/design/data-providers.md:242-244,111-116
  Acceptance criteria (agent-executable): 4 处文档无旧路径引用；`grep -rn "stock_basic" kb/` 仅剩与新路径一致的引用
  QA scenarios: grep 验证无旧引用（failure: 有残留则修复）
  Commit: N

- [ ] 6. T6: 全量验证 + 单 commit ref #80
  What to do / Must NOT do: `cargo test` 全 workspace + `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo llvm-cov`（覆盖率 ≥80%）+ `cargo doc --no-deps`（rustdoc 4a 验证）；单 commit `ref #80`（禁止 fixes/closes）；commit 后 /review-work。Must NOT: 不 push；不 merge 其他分支。
  Parallelization: Wave 4 | Blocked by: T1-T5 | Blocks: -
  References: AGENTS.md commit 纪律
  Acceptance criteria (agent-executable): 全部命令 exit 0；commit 含 ref #80；review 无 blocking
  QA scenarios: 上述命令全绿 + review 报告
  Commit: Y | refactor(cleanup): remove legacy stock_basic schema paths

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — todos 全部完成，范围无漂移（Must NOT 未违反）
- [ ] F2. Code quality review — /review-work 无 blocking issues
- [ ] F3. Real manual QA — cargo test/clippy/fmt/llvm-cov 全绿 + RED→GREEN 证据链完整
- [ ] F4. Scope fidelity — kb 4 处同步无残留；无未授权文件改动

## Commit strategy
- 单 commit: `refactor(cleanup): remove legacy stock_basic schema paths` + body `ref #80`（不 push，等用户指令）

## Success criteria
- duckdb.rs 无旧 schema 残留（issue #80 验收 1）
- `import` 命令不再生成 stock_basic.parquet（issue #80 验收 2）
- 全部测试/clippy/fmt/覆盖率通过
- kb 4 处文档与代码一致
