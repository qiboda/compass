# timeframe-theme-stocklist - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 三个独立修复：#46 timeframe 聚合（已实现，本批次补集成测试收尾）、#132 主题切换写回 config.toml、#71 股票列表只显示当前上市 A 股。

**Why this approach:** #46 已在 master 实现（2026-07-28 commit 2982b72b），补集成测试验证后关闭 issue；#132 镜像现有 save_language_config 模式；#71 在 GUI 层过滤（不碰数据层，避免影响 SEPA/screener universe）。

**What it will NOT do:** 不改 ParquetReader 聚合（设计如此）；不引入 Timeframe enum；不做财务数据覆盖 join；不改 SEPA/screener 的 load_all_stock_basics 消费；不改主题启动读取逻辑。

**Effort:** Short
**Risk:** Low - 三个修复相互独立，均有现成模式/测试模板可循
**Decisions to sanity-check:** #71 过滤层级选 GUI 层（非数据层）；#71 过滤条件 delist_date.is_none() 单条件（21 只 B 股全部已退市）

Your next move: 已获用户推进授权（"全部完成后 push"）。

---

> TL;DR (machine): 3 commits（#46 集成测试 / #132 主题写回 / #71 列表过滤），1 PR，Effort Short，Risk Low

## Scope
### Must have
- #132: `save_theme_config`（镜像 save_language_config）+ render_toolbar 主题切换后调用 + 失败 warn + RED 测试 + doc-sync
- #71: `load_stock_list` GUI 层 `delist_date.is_none()` 过滤 + RED 测试 + doc-sync
- #46: 端到端集成测试（已完成，commit b724b0b）
### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不引入 Timeframe enum（全链路 &str）
- 不做 ParquetReader::fetch_bars timeframe 聚合（parquet.rs:713 忽略是设计）
- 不改 SEPA/screener 消费的 load_all_stock_basics
- 不引入财务数据覆盖 join
- #132 不改主题启动读取逻辑（CompassTheme::from_config 已实现）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD + rstest/kittest（现有模式）
- Evidence: .omo/evidence/

## Execution strategy
### Parallel execution waves
- Wave 1: 委派 skwy-adversarial-test + skwy-requirement-test 写 RED（#132/#71 并行）
- Wave 2: 实现 #132 → 实现 #71（可并行，无依赖）
- Wave 3: doc-sync + 全量验证
### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (RED tests) | plan 批准 | 2,3 | - |
| 2 (#132 impl) | 1 | 4 | 3 |
| 3 (#71 impl) | 1 | 4 | 2 |
| 4 (验证/commit) | 2,3 | 5 | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. 委派 skwy-adversarial-test + skwy-requirement-test 写 RED 测试
  What to do / Must NOT do: 两个 agent 并行。skwy-adversarial-test 攻击边界/失败路径（config 写回失败、无 config 文件、delist_date 边界、B 股/退市剔除）；skwy-requirement-test 验证需求契约（主题切换→config.toml theme 键变+重启恢复、列表只含上市 A 股）。只写测试不写实现。
  Parallelization: Wave 1 | Blocked by: plan 批准 | Blocks: 2,3
  References: crates/compass/src/main.rs:403-492（save 函数）/ :1113-1138（主题切换）/ :504-521（load_stock_list）/ :1317-1320（HOME_LOCK）/ :3943（端到端模板）; crates/compass/src/citizens/ui_fixes_218.rs:41-153（build_compass_app）; crates/compass-core/src/data/parquet.rs:396-439（load_all_stock_basics）/ :766-787（fixture）
  Acceptance criteria: RED 测试失败输出存在
  QA scenarios: 无（测试编写阶段）
  Commit: N

- [ ] 2. 实现 #132 save_theme_config + 调用点
  What to do / Must NOT do: 新增 `fn save_theme_config(theme: &str) -> Result<(), String>` 逐行镜像 save_language_config（main.rs:469-492），key 为 "theme"；render_toolbar 主题切换分支（main.rs:1125 self.theme = ... 之后）加 `if let Err(e) = save_theme_config(name) { tracing::warn!(...) }`。不改启动读取逻辑。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: crates/compass/src/main.rs:469-492（save_language_config 模板）/ :1113-1138（主题切换）/ :103-107（warn 模式）
  Acceptance criteria: cargo test -p compass -- save_theme 全绿；lsp_diagnostics 无错误
  QA scenarios: happy（切换→config 写回）/ failure（父路径为文件→warn 不崩溃）
  Commit: Y | feat(gui): persist theme switch to config.toml

- [ ] 3. 实现 #71 load_stock_list 过滤
  What to do / Must NOT do: main.rs:504-521 load_stock_list 加载后 `into_iter().filter(|s| s.delist_date.is_none()).collect()`。不改 parquet.rs load_all_stock_basics（SEPA/screener 共用）。不引入符号前缀 B 股判断（delist_date 单条件已覆盖）。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: crates/compass/src/main.rs:504-521（load_stock_list）/ :574（stock_list 字段）; crates/compass-core/src/model.rs:48-70（StockBasic.delist_date: Option<NaiveDate>）; crates/compass-strategy/src/lib.rs:145（exclude_delisted 范式）
  Acceptance criteria: cargo test -p compass -- load_stock 全绿；lsp_diagnostics 无错误
  QA scenarios: happy（含退市/B 股 fixture → 列表剔除）/ failure（空列表 → 空 Vec 不崩）
  Commit: Y | fix(gui): filter stock list to currently-listed A-shares

- [ ] 4. doc-sync + 全量验证
  What to do / Must NOT do: kb/design/ui.md + kb/user/gui.md 主题持久化描述与实现一致（补充失败 warn 语义）；kb/design/ui.md 股票列表过滤语义；更新 handoff.md:40 错误假设（stock_basic 含退市/B 股）。cargo test 全绿 + clippy + fmt。
  Parallelization: Wave 3 | Blocked by: 2,3 | Blocks: 5
  References: kb/design/ui.md:38; kb/user/gui.md:159/226; .omo/handoff.md:37-42
  Acceptance criteria: cargo test 全 workspace 通过；cargo clippy -- -D warnings 通过；cargo fmt --check 通过
  QA scenarios: 文档内容核对
  Commit: Y | docs: sync theme persistence + stock list filtering docs

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 3 个独立 commit（各 ref #N）+ doc-sync commit 可并入对应实现 commit
- 一个 PR（fix/timeframe-theme-stocklist → master）

## Success criteria
- #46: 集成测试通过（已验证），issue 关闭
- #132: 切换 compass_light → config.toml theme 键变；重启恢复；失败 warn 不崩溃
- #71: 列表只含当前上市 A 股（delist_date.is_none()），退市/B 股不出现
