# #360 F2 — 五角度审查（review findings + 处置）

审查对象：`ad850c4..824817d`（实现 `9777dd6` + evidence `824817d`），五角度并行子代理。
结论：**无 P0；3×P1（1 项数据面、2 项文档/台账）已修；P2/P3 逐条处置**。

## 1. 各角度结论

| 角度 | 子代理 | 结论 | P0 | P1 | P2 | P3 |
|---|---|---|---|---|---|---|
| 通用代码审查 | `dd74c21b` | 修改后通过（无 P0/P1） | 0 | 0 | 2 | 4 |
| 目标符合性 | `1cb4e989` | 8/8 验收标准 SATISFIED | 0 | 0 | 3 | 4 |
| 质量/文档同步 | `92edaf6a` | 修改后通过 | 0 | 2 | 3 | 5 |
| 安全/数据安全 | `af16856a` | Medium（P1 数据面） | 0 | 1 | 1 | 1 |
| 测试充分性 | `2afb3fa3` | 测试充分（无断言被削弱） | 0 | 1 | 3 | 3 |

共性正面结论（跨角度独立复核）：

- 生产代码零残留；`cargo check --workspace --tests` 干净；无依赖变更。
- 评分改动与锁定决策逐字一致：唯一数值变更 `10.0 → DRAGON_INSTITUTION_BONUS = 15.0`，cap 30 未动，
  `institution_buy` 构建条件（`institution_flag == Some(1) && net_amount > 0.0`）与 base 完全一致。
- i18n 三边一致（scoring `note_args` 3 元素 / `locales/{en,zh}` 3 占位 / GUI `factor_note_text` arg(0..2)，越界取 0.0 无 panic）。
- 既有断言未被削弱（QA 逐条核对删除的断言行，全部为「方法/表随之删除」的正当删除）。
- 无注入面回归（表名来自白名单枚举；`--since` 校验未放宽；shell 无新拼接面）；无凭证泄露；无 `dolt add .` / 通配删除。

## 2. P1 处置

| ID | 角度 | 问题 | 处置 |
|---|---|---|---|
| SEC-P1-1 | 安全 | 冒烟 `sepa backtest --start 2026-07-01` 触发 `crates/compass-data/src/backtest.rs:115` 的无条件 `DELETE FROM backtest_result`，把远端既有 384 行快照（2025-01-02..2026-08-03）替换为 51 行短窗口——超出 #360 声明范围的数据面副作用 | **已修复**：`backtest_result` 按设计即「单快照全表替换」，故以默认全窗口重跑 `sepa backtest`（2025-01-01..最新）恢复完整历史曲线（新公式口径），单独 Dolt commit+push；事实与影响写入 F3/F4 evidence 与本 PR 完成 comment；代码层加固（scoped delete / 快照保护）另开 issue |
| QUAL-P1-1 | 质量 | F1 evidence 声称 `architecture.md` 新增 #360 决策行，实际只在 `data-providers.md` | **已修复**：`architecture.md` 决策记录表补 `SEPA-1（#360）` 行（what/why/why-not 自包含），与 plan Must-have 及 evidence 声称对齐 |
| QUAL-P1-2 | 质量 | plan 台账 10 个 todo 全未勾选，与已完成的实现/evidence 脱节 | **已修复**：10 项全部勾选为 `- [x]`，文件头补状态行（2026-09-10），第 10 项改为指向 Final verification wave 的指针（消除与 F1-F4 的重复完成定义） |
| QA-P1-1 | 测试 | `sepa.note.big_capital` 的 **en** 模板被本次编辑却无任何测试渲染（zh 用例无法守 en 文本） | **已修复**：`crates/compass/src/citizens/sepa.rs` 的 `factor_note_text_preserves_numeric_precision` 增加 en 断言（`main 20 + dragon 15 + block +5` / `-5`），并新增「多余参数被忽略」守卫（防第 4 个 survey 槽位以位置漂移回归） |

## 3. P2/P3 处置

| ID | 角度 | 问题 | 处置 |
|---|---|---|---|
| GEN-P2-1 | 通用 | 历史 `final_score`/`capital_factor` 仍为旧公式产物，与当日新口径混表 | 记录（#360 未要求历史重算）：F3 §4 + PR/issue 完成 comment 显式登记；统一历史口径的 `sepa backfill-dates` 列入后续 issue |
| GEN-P2-2 | 通用 | F1 evidence 的 commit/文件计数未标时点，与 HEAD 核对冲突 | **已修复**：F1 补「时点 + HEAD 计数」说明 |
| SEC-P2-1 | 安全 | 仓库外旧采集中间产物残留：`/data/compass-data/csv/institution_survey.progress.json`、`RPT_ORG_SURVEYNEW.csv`；且 `progress` 子命令的 glob 枚举会把它显示为有效 target | **已修复**：两个文件已删除（`csv` 目录零命中）；F4 grep 范围补 `*.json`/`*.csv` 与仓库外数据目录 |
| QUAL-P2-1 | 质量 | `toolchain.md` 性能卡写成「6 份数据」，实际当前读取 5 份 | **已修复**：改为「5 份（…）；历史为 7 份：另含 concept_member（#283）、institution_survey（#360）」 |
| QUAL-P2-2 | 质量 | `data-providers.md` #306 决策行「其余 9 表」未标时点 | **已修复**：补「（当时计数；#360 后为按锚点增量的 8 表）」 |
| QUAL-P2-3 | 质量 | `architecture.md` C5 行把已删除的 `concept_member` 计入「5 个接入者」，与同处注（5 模块/7 文件）成员定义冲突 | **已修复**：C5 行标注改为「#360 后接入者减为 4 个：main_flow/block_trade/index_daily/dragon——concept_member 早随 #283 移除、institution_survey 随 #360 移除」 |
| GOAL-P2-1 | 目标 | `just check` 全绿无法独立复核（无日志落盘） | **已修复**：修复 commit 后于 HEAD 重跑 `just check` 并将输出落盘 `.dsh/evidence/remove-institution-survey/360-F3-just-check.txt` |
| GOAL-P2-2 | 目标 | 覆盖率/测试跑在数据面变更之前 | **已修复**：修复 commit 后于 HEAD 重跑 `cargo llvm-cov nextest`，F3 覆盖率表按新报告更新 |
| QA-P2-1 | 测试 | collectors CLI/sync 的 target 集合移除无测试（仅编译保障） | **已修复**：`crates/compass-collectors/src/orchestrate.rs` 新增 `removed_and_unknown_targets_are_rejected`——断言 `fetch`/`import_target` 对 `institution_survey`/`institution-survey`/未知名全部拒绝（未知分支在任何 I/O 前短路，纯分派契约测试） |
| QA-P2-2 | 测试 | RED 证据仅以散文记录（`/tmp` 原始日志未入库） | 记录为已知边界（RED 无法在实现后重放；F1 增列「证据强度」说明，反思中提出改进） |
| QA-P2-3 | 测试 | 删除 `institution_survey_requirement_drift_guard_preserves_full_pk_rows` 降低了 compass-data 的重量级 Dolt 子进程覆盖 | 记录（该表已移除，drift guard 无对象）；在 PR 完成 comment 说明 |
| QUAL-P3-1 | 质量 | `backtest.md` 成本口径与 `toolchain.md` 不一致 | **已修复**：同步为 5 份/4 张（注明 #360 前为 7 份/6 张） |
| QUAL-P3-2 | 质量 | `data-providers.md` #150 行引用已不存在的 note 字符串 | **已修复**：标注「当时为『主力+龙虎+调研+大宗』，该分解已随 #360 变更」 |
| QUAL-P3-3 | 质量 | `import_compass.rs` 注释残留 Python 时代文件引用 `main.py:79-85` | **已修复**：改为现行落点 `compass-collectors/src/stock_basic_official.rs` |
| QUAL-P3-4 | 质量 | `data-providers.md` #202 行「与 institution_survey 长文本表同模式」未加注 | **已修复**：补「该长文本表已随 #360 移除，宽表场景仍适用」 |
| QUAL-P3-5 | 质量 | plan 的 Todos 与 F-wave 完成定义重复 | **已修复**：见 QUAL-P1-2 |
| GEN-P3-1 | 通用 | GUI 注记测试 fixture 用旧常量 10（`[75, 10, 5]`） | **已修复**：改为新公式可复现组合 `[20, 15, ±5]` + 多余参数守卫（同 QA-P1-1） |
| GEN-P3-2 | 通用 | `cov.json` 未跟踪且未被忽略 | **已修复**：`.gitignore` 增补 `cov.json` |
| GEN-P3-3 | 通用 | `.dsh/handoff.md`、`.dsh/plans/handoff.md` 保持未提交 | 有意保留（worktree 会话本地上下文，不入 PR；反思中说明） |
| GEN-P3-4 | 通用 | 实现 commit message「requirement (6)」为两文件之和，易误读 | 不再 amend（commit hash 已被 evidence 引用，改哈希成本高于收益）；F1 逐文件列明 7/4/2 计数 |
| GOAL-P3-1 | 目标 | 「仅调研、无机构龙虎买入」标的净失 5 分未量化 | 完成 comment 明确记录：该类标的资本分净减 5（cap 30 无法吸收）；历史 `final_score` 为旧口径 |
| GOAL-P3-2 | 目标 | 「grep 零残留」的字面口径与测试字面量例外 | F4 例外表逐条说明（契约测试/历史记录）；完成 comment 采用「生产代码/脚本/locales 零残留」口径 |
| QA-P3-1 | 测试 | `capital ≈ 0.20 × big_capital` 断言与相邻 score 断言代数重复 | 保留（低信息但无害，锁定模块权重） |
| QA-P3-2 | 测试 | 新的 data 侧测试未重复断言「未知表名」通用边界 | 既有 `compass_table_from_str_invalid_variant` 覆盖；保留 |
| QA-P3-3 | 测试 | dragon 机构买入无「多日窗口近因」语义测试（既有语义，本次仅改数值） | 记录进后续 issue（评分口径硬化候选） |
| SEC-P3-1 | 安全 | Dolt 数据 commit 身份为 `Test <test@compass.local>`（全局 dolt 配置污染，ref #348 同类） | 数据仓库本地身份已对齐既有 CI 约定（`CI <ci@compass.local>`）；全局污染与代码层加固记入后续 issue |

## 4. 复审

修复提交后按 `.dsh/evidence/remove-institution-survey/360-F3-*.md` 记录重跑的门禁结果（`just check` 日志落盘 + llvm-cov
覆盖率复测 + 真实数据二次冒烟）。代码面修复仅涉及测试/注释（`sepa.rs` 测试、`orchestrate.rs` 测试、`import_compass.rs` 注释），
无生产逻辑变更，故无需第二轮全角度复审；文档面修复由 F1/F4 逐条复核。
