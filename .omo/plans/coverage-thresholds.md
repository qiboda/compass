# coverage-thresholds - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 按可测试性重新设定每个代码库的覆盖率门槛：纯逻辑库提到 95%，图形界面主程序 90%，整体工作区 93%；同时补上 compass-types 和 compass-i18n 缺失的测试，让所有门槛首次全部真实达标。

**Why this approach:** 门槛 = 可测试性——纯类型/serde 代码最容易测，就该有高门槛；GUI 事件循环难测，放低到 90%。实测数据显示 compass-types 只差 15 行测试（全是 serde 默认值函数），compass-i18n 只差 3 行测试分支，都是极小成本；compass-data 今日实测已 96.92%，无需补测。

**What it will NOT do:** 不改任何生产逻辑（只加测试 + 改脚本阈值 + 同步文档）；不动 compass-data/strategy/core/ui 的代码；不改 Python 收集器门槛；不改 CI workflow 结构。

**Effort:** Short
**Risk:** Low - 阈值调整 + 两处小补测，实测全部 crate 均超新阈值（compass-types 补测后预期 100%）

**Decisions to sanity-check:** ① workspace 门槛 80%→93%（用户指定）；② compass-data 今日实测 96.92% 已超 96% 缓冲线，不再补测（取代 handoff 的"补测至 ~96%"）；③ compass-i18n 白名单分支抽纯函数 + 表驱动测试。

Your next move: 批准即执行。Full execution detail follows below.

---

> TL;DR (machine): Short effort, Low risk — check-coverage.sh 阈值表更新（types/i18n/strategy/ui=95, compass=90, workspace=93, core/data=95）+ compass-types/i18n 补测 + doc-sync。

## Scope
### Must have
- `scripts/check-coverage.sh` 阈值表：compass-types/i18n/strategy/ui → 95，compass → 90，workspace → 93，compass-core/data 保持 95；同步头注释（L9-10）与 fallback 注释（L45）
- compass-types 补测：`momentum = {}` / `volume = {}` 空表 serde 反序列化测试（覆盖 5 个 default_* 函数 15 行）
- compass-i18n 补测：白名单 OR 分支抽纯函数 + 表驱动测试（覆盖 L413-415 三分支）
- doc-sync：AGENTS.md 覆盖率门槛描述、kb/dev/testing.md 覆盖率章节、kb/dev/process.md L122、ci.yml L49 step name
- 验证：`cargo llvm-cov nextest --json --summary-only` + `bash scripts/check-coverage.sh` 全绿

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不修改 compass-data/compass-core/compass-strategy/compass-ui/compass 的生产代码
- 不降低 compass-core/compass-data 阈值（保持 95%）
- 不改 Python collectors（`--cov-fail-under=95` 不受影响）
- 不改 .github/workflows/ci.yml 的 workflow 结构（仅 step name 文本）
- 不触碰历史归档：kb/design/architecture.md L510 决策记录、kb/dev/reflections-archive.md、.omo/plans/*.md
- 不新增依赖、不改 Cargo.toml

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD — 先写失败测试（RED）确认因正确原因失败，再实现（GREEN）；compass-types/i18n 补测均为测试代码变更
- Evidence: `.omo/evidence/coverage-thresholds/` 目录，每次验证输出落盘

## Execution strategy
### Parallel execution waves
- Wave 1（3 todos，可并行）：① compass-types 补测 ② compass-i18n 补测 ③ check-coverage.sh 阈值表+注释
- Wave 2（2 todos，串行于 Wave 1）：④ doc-sync（AGENTS.md + kb/dev/testing.md + kb/dev/process.md + ci.yml） ⑤ 全量 llvm-cov 验证
- Wave 3（1 todo）：⑥ 决策记录检查（Step 5c）+ 收尾

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 compass-types 补测 | — | 5 验证 | 2, 3 |
| 2 compass-i18n 补测 | — | 5 验证 | 1, 3 |
| 3 check-coverage.sh 阈值 | — | 5 验证 | 1, 2 |
| 4 doc-sync | 1,2,3（阈值数字定稿） | — | — |
| 5 llvm-cov 全量验证 | 1,2,3 | 6 | — |
| 6 决策记录检查 + 收尾 | 5 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. compass-types: 补 serde 空表默认值测试（momentum/volume）
  What to do / Must NOT do: 在 `crates/compass-types/src/lib.rs` 的 `mod tests` 内新增 1-2 个测试，镜像现有 `empty_condition_table_uses_struct_default`（L426-433）模式：反序列化 `"momentum = {}\n"` 与 `"volume = {}\n"`，断言字段默认值符合契约（momentum: days=20/min_pct=0.0/max_pct=100.0；volume: days=20/times=2.0）。MUST NOT 修改任何生产代码（L1-345）、MUST NOT 添加新依赖、MUST NOT 改动现有测试。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5
  References (executor has NO interview context - be exhaustive): `crates/compass-types/src/lib.rs:426-433`（现有 empty-table 测试模式）、`crates/compass-types/src/lib.rs:49-73`（MomentumCondition + default fns）、`crates/compass-types/src/lib.rs:97-114`（VolumeCondition + default fns）、未覆盖行 63-73/108-114 由 `cargo llvm-cov -p compass-types --json` 实测确认
  Acceptance criteria (agent-executable): `cargo nextest run -p compass-types` 全绿；`cargo llvm-cov -p compass-types --json` 行覆盖 ≥95%（预期 100%，143/144+）
  QA scenarios (name the exact tool + invocation): happy — `cargo nextest run -p compass-types`（新测试 PASS）；failure — 注释掉新测试断言其一，确认 `cargo nextest run -p compass-types` 失败（RED 验证）；Evidence `.omo/evidence/coverage-thresholds/task-1-types.md`
  Commit: Y | test(compass-types): cover serde empty-table defaults for momentum/volume conditions

- [ ] 2. compass-i18n: 白名单 OR 分支抽纯函数 + 表驱动测试
  What to do / Must NOT do: 在 `crates/compass-i18n/src/lib.rs` 测试模块（L324-420）内：① 将 `zh_values_are_chinese`（L397-419）断言中的白名单 OR 链（L404-417）提取为私有纯函数（如 `fn is_allowed_zh_token(key: &str) -> bool`，可放测试模块内）；② 主测试调用该函数；③ 新增表驱动测试逐前缀覆盖 `sepa.unit`/`screener.ma`/`screener.years`（当前 L413-415 count=0）。MUST NOT 修改生产代码（L1-322）、MUST NOT 改动 locale yml、MUST NOT 触碰 L13 宏行（不可测，接受）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5
  References (executor has NO interview context - be exhaustive): `crates/compass-i18n/src/lib.rs:397-419`（zh_values_are_chinese 测试）、`crates/compass-i18n/src/lib.rs:324-340`（现有测试 helper locale_keys）、未覆盖行 413-415 由 `cargo llvm-cov -p compass-i18n --json` 实测确认
  Acceptance criteria (agent-executable): `cargo nextest run -p compass-i18n` 全绿；`cargo llvm-cov -p compass-i18n --json` 行覆盖 ≥95%（预期 65/66 = 98.5%）
  QA scenarios (name the exact tool + invocation): happy — `cargo nextest run -p compass-i18n`（新表驱动测试 PASS）；failure — 删掉一个前缀分支断言，确认覆盖跌回 <95%（RED 验证）；Evidence `.omo/evidence/coverage-thresholds/task-2-i18n.md`
  Commit: Y | test(compass-i18n): cover zh whitelist branch prefixes with table-driven tests

- [ ] 3. check-coverage.sh: 阈值表 + 注释更新
  What to do / Must NOT do: 更新 `scripts/check-coverage.sh`：THRESHOLDS 数组（L21-30）→ workspace=93, compass-core=95, compass-data=95, compass-i18n=95, compass=90, compass-strategy=95, compass-types=95, compass-ui=95；头注释（L9-10）改为新日期与新引用（ref #250）；L45 fallback `:-80` 注释标注默认值含义。MUST NOT 改 check() 逻辑、MUST NOT 改 jq 过滤器、MUST NOT 改 ci.yml。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5
  References: `scripts/check-coverage.sh:9-10,21-30,45`
  Acceptance criteria (agent-executable): `grep -A9 'declare -A THRESHOLDS' scripts/check-coverage.sh | grep -E 'workspace.*93|compass\]\=90|compass-(i18n|strategy|types|ui)\].*95'` 全匹配
  QA scenarios: happy — grep 断言通过；failure — 用旧阈值文件 `bash scripts/check-coverage.sh /tmp/opencode/cov-types.json` 确认仍按脚本逻辑运行（结构未破坏）；Evidence `.omo/evidence/coverage-thresholds/task-3-script.md`
  Commit: Y | chore(ci): update per-crate coverage thresholds to testability (types/i18n/strategy/ui=95, compass=90, workspace=93)

- [ ] 4. doc-sync: 覆盖率门槛文档同步
  What to do / Must NOT do: 更新 4 处文档：① AGENTS.md Testing 章节（L499 附近）— workspace 80%→93%、types/i18n/strategy/ui 80%→95%、compass 80%→90%、注明 ref #250；② kb/dev/testing.md L249-253 + L266-268 — 同步阈值表与日期；③ kb/dev/process.md L122 — 同步数字；④ .github/workflows/ci.yml L49 step name — "data/core 95%, others 80%" → "data/core/types/i18n/strategy/ui 95%, compass 90%, workspace 93%"（或等价简洁表述）。MUST NOT 改历史归档文件。
  Parallelization: Wave 2 | Blocked by: 1,2,3 | Blocks: —
  References: AGENTS.md:499、kb/dev/testing.md:249-253,266-268、kb/dev/process.md:122、.github/workflows/ci.yml:49
  Acceptance criteria (agent-executable): `grep -rn '80%' AGENTS.md kb/dev/testing.md kb/dev/process.md | grep -i coverage` 无残留旧阈值（除历史归档）；`grep -n 'coverage gate' .github/workflows/ci.yml` 显示新 step name
  QA scenarios: happy — grep 断言通过；failure — 逐文件核对数字一致性（workspace=93/compass=90/其余 95）；Evidence `.omo/evidence/coverage-thresholds/task-4-docs.md`
  Commit: Y | docs: sync coverage thresholds in AGENTS.md/testing.md/process.md/ci.yml

- [ ] 5. 全量覆盖率验证（llvm-cov + check-coverage.sh）
  What to do / Must NOT do: 运行 `mkdir -p target/llvm-cov && cargo llvm-cov nextest --json --summary-only --output-path target/llvm-cov/coverage.json`，然后 `bash scripts/check-coverage.sh target/llvm-cov/coverage.json`。MUST NOT 跳过任何 crate 的检查、MUST NOT 修改阈值以凑通过。
  Parallelization: Wave 2 | Blocked by: 1,2,3 | Blocks: 6
  References: kb/dev/testing.md:258（CI 等价命令）
  Acceptance criteria (agent-executable): check-coverage.sh 退出码 0 且 8 行全 OK（workspace/core/data/i18n/compass/strategy/types/ui）
  QA scenarios: happy — 全 OK 输出；failure — 若某 crate < 阈值，回查对应 todo 补测后重跑；Evidence `.omo/evidence/coverage-thresholds/task-5-verify.json`（复制 coverage.json）
  Commit: N（验证无代码变更）

- [ ] 6. 决策记录检查（门禁 Step 5c）+ 收尾
  What to do / Must NOT do: 检查 kb/design/ 下相关文件是否含 `## 决策记录` 章节；kb/design/architecture.md L510 有历史决策记录（80% 门槛），按项目规则为不可变历史记录，本次不修改，但需在 commit message 或 issue comment 中说明。MUST NOT 改写历史决策记录。
  Parallelization: Wave 3 | Blocked by: 5 | Blocks: —
  References: kb/design/architecture.md:510、AGENTS.md 决策记录章节
  Acceptance criteria (agent-executable): kb/design/ 相关文件确认含决策记录章节（缺失才补）
  QA scenarios: happy — 确认存在；failure — 若缺失则按格式补一节；Evidence `.omo/evidence/coverage-thresholds/task-6-decision.md`
  Commit: N（通常无代码变更）

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 6 todos 全部完成，evidence 落盘 `.omo/evidence/coverage-thresholds/`
- [ ] F2. Code quality review — 新增测试遵循现有风格（plain #[test] + TOML round-trip），无类型抑制，无新依赖
- [ ] F3. Real manual QA — 本地 `cargo llvm-cov nextest --json --summary-only` + check-coverage.sh 全绿（CI 等价）；`cargo nextest run` 全 workspace 测试通过
- [ ] F4. Scope fidelity — 只动 check-coverage.sh + compass-types 测试 + compass-i18n 测试 + 4 处文档；未触碰生产逻辑/其他 crate/Python

## Commit strategy
- 按 todo 拆分：3 个实现/测试 commit（todo 1/2/3 各自 commit）+ 1 个 docs commit（todo 4）
- 每个 commit 消息含独立成行的 `ref #250`
- 提交后运行 /review-work 审查（最多 2 轮修复）
- push 前用户确认；push 前先 `git fetch origin master && git rebase origin/master`
- push 后：追加 issue comment（实现摘要 + 验收状态 + commit 列表）→ 关闭 issue #250

## Success criteria
- [ ] check-coverage.sh 阈值表 = types/i18n/strategy/ui 95%、compass 90%、workspace 93%、core/data 95%
- [ ] compass-types 实测 ≥95%（预期 ~100%）
- [ ] compass-i18n 实测 ≥95%（预期 ~98.5%）
- [ ] 全部 crate + workspace 实测 ≥ 新阈值，check-coverage.sh 全绿（本地 llvm-cov 验证）
- [ ] AGENTS.md + kb/dev/testing.md + kb/dev/process.md + ci.yml 文档同步
- [ ] 历史决策记录/归档未被动（architecture.md L510 仅说明）
- [ ] 完成 commit 全部引用 ref #250；review 通过；push 后 issue #250 关闭
