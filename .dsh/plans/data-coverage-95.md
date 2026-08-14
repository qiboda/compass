# data-coverage-95 - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 数据层（Python collectors + 两个 Rust crate）测试覆盖率门槛提高到 95%，CI 强制 per-crate 阈值，防止覆盖率回退。Python collectors 补约 190 条语句的测试使本地覆盖率从 83% 到 95%。

**Why this approach:** 两个 Rust crate（compass-data 96%、compass-core 98%）已远超 95%，只需改 CI 门槛——零测试编写。唯一需要写测试的是 Python collectors（83%），其中 4 个文件占缺失量的 70%，按"最便宜的先做"排序攻坚；网络抓取层用 stub 真实测试而非打补丁跳过（符合锁定决策）。

**What it will NOT do:** 不改 Rust 测试；不把其余 4 个 crate 门槛提到 95；不重构 collectors 生产代码；不整层跳过网络代码（仅脚本入口块和客观不可达的死代码用 pragma）；不改 workspace 总门槛。

**Effort:** Medium（~190 stmts Python 测试 + 2 个脚本/工作流文件 + 2 个文档文件）
**Risk:** Low - 唯一风险点 fetch_stock_basic_official.py 网络层需新增 sync stub，但无生产代码改动
**Decisions to sanity-check:** ① pragma 仅限 `__main__` 块 + 客观不可达死代码；② check-coverage.sh 重构为内嵌阈值表（ci.yml 调用简化为单参数）

Your next move: 已获用户"自行走完全流程"授权，plan 产出后直接进入测试计划环节。

---

> TL;DR (machine): Medium effort, Low risk, Python collectors 83%→95% + CI per-crate 门槛 + 文档同步

## Scope
### Must have
- Python collectors 覆盖率 ≥95%（`--cov-fail-under=95` 通过）
- `scripts/check-coverage.sh` per-crate 阈值：compass-data、compass-core → 95，其余 crate → 80，workspace total → 80
- `.github/workflows/ci.yml` Python `--cov-fail-under=95` + coverage step 调用更新
- 4 处 `__main__` 块 + stock_basic_official 不可达死代码加 `# pragma: no cover` 并注明理由
- 独立测试计划文件 `.omo/plans/data-coverage-95-tests.md`（test agent 产出）
- `kb/dev/testing.md` + `AGENTS.md` 覆盖率门槛段落同步（≥80% → per-crate 描述）
- 检查 `kb/design/` 相关文件含 `## 决策记录` 章节

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不修改任何 Rust 测试代码（compass-data/compass-core 零测试编写）
- 不把 compass/compass-strategy/compass-types/compass-ui 阈值提到 95
- 不重构 collectors 生产代码（不做 main() session 注入等重构——用 stub 测现有接口）
- 不整层 pragma 网络代码（fetch_stock_basic_official.py L426"非测试范围"注释不作为 pragma 依据，网络层必须真实测试）
- 不改变 workspace total 80 阈值
- 不引入新 Python 依赖
- 不创建多个 issue（单 issue #163）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD - Python 测试先行（RED）再让实现通过（GREEN）；脚本/工作流改动以断言验证
- 框架: pytest + pytest-cov（`cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95 --cov-report=term-missing`）
- 证据: .omo/evidence/task-<N>-data-coverage-95.txt（每次 pytest 运行输出）

## Execution strategy
### Parallel execution waves
- **Wave 1 — 测试攻坚（4 个并行 todo）**: T1 main.py dispatch 尾部 30 stmts（纯 Mock）；T2 fetch_concept_member.py 20 stmts；T3 fetch_fin_indicators.py 30 stmts；T4 fetch_stock_basic_official.py 网络层 107 stmts（含 conftest sync stub）
- **Wave 2 — 收尾门槛（2 个并行 todo）**: T5 pragma 补齐（__main__×4 + 死代码）；T6 check-coverage.sh per-crate 重构 + ci.yml 更新（一个 todo，因 ci.yml 调用依赖脚本接口）
- **Wave 3 — 文档（1 个 todo）**: T7 kb/dev/testing.md + AGENTS.md + kb/design 决策记录检查

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 main.py dispatch 测试 | — | — | T2, T3, T4 |
| T2 concept_member 测试 | — | — | T1, T3, T4 |
| T3 fin_indicators 测试 | — | — | T1, T2, T4 |
| T4 stock_basic_official 测试（conftest stub） | — | — | T1, T2, T3 |
| T5 pragma 补齐 | — | — | T1-T4（可并行但涉及同文件时注意冲突，建议在 T1-T4 之后） |
| T6 check-coverage.sh + ci.yml | — | — | T1-T5 |
| T7 文档 | — | — | T1-T6 |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. main.py dispatch_fetch/dispatch_import 补齐 5 个未测 target 分支
  What to do: 在 collectors/tests/test_main.py 的 TestDispatchFetch/TestDispatchImport 中，为 dragon/block_trade/institution_survey/concept_member/main_flow 各补 dispatch 测试（复制现有 L62-240 的 Mock 模式：`patch("main.asyncio.run")` + `patch("main.fetch_X.run"/"fetch_X.import_to_dolt")` 断言被调用）。覆盖 main.py:260-278（dispatch_fetch 尾部）与 :302-316（dispatch_import 尾部）。
  Must NOT do: 不改 main.py 生产代码；不 mock 未涉及的已有测试。
  Parallelization: Wave 1 | Blocked by: — | Blocks: —
  References: collectors/main.py:260-278,302-316; collectors/tests/test_main.py:62-240（现有 5 target 的 Mock 模式）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_main.py -q` 全过；`uv run pytest tests/ --cov=. --cov-report=term-missing` 中 main.py 缺失行不再含 260-278,302-316
  QA scenarios: happy — 每 target 断言 fetch_X.run/import_to_dolt 被调一次；failure — 无（纯 dispatch 冒烟）。Evidence .omo/evidence/task-1-data-coverage-95.txt
  Commit: Y | test(collectors): cover main.py dispatch tail branches for 5 targets

- [ ] 2. fetch_concept_member.py 补 429/多页/guard 分支测试
  What to do: 在 collectors/tests/test_concept_member.py 补：① 429 分支（fetch_board_list :89-92、fetch_board_members :158-161，用现有 429 canned 模式，参考 test_fin_indicators.py:102-130 的 _get 闭包）；② 多页循环 :116（canned total > 每页数量）；③ guard 分支 :179-187（data=None/success=False/result=None）。用现有 StubSession + URL-dispatching stub.get 模式。
  Must NOT do: 不改 fetch_concept_member.py 生产代码。
  Parallelization: Wave 1 | Blocked by: — | Blocks: —
  References: collectors/fetch_concept_member.py:89-92,116,158-161,179-187; collectors/tests/test_concept_member.py:1-99（现有 run() 测试模式）; collectors/tests/test_fin_indicators.py:102-130（429 模式）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_concept_member.py -q` 全过；term-missing 中 fetch_concept_member.py 缺失行不含上述行号
  QA scenarios: happy — 429 重试后成功；failure — data=None 打印 "No data returned"。Evidence .omo/evidence/task-2-data-coverage-95.txt
  Commit: Y | test(collectors): cover concept_member 429, pagination and guard branches

- [ ] 3. fetch_fin_indicators.py 补 subprocess/Throttle/guard/incremental 分支测试
  What to do: 在 collectors/tests/test_fin_indicators.py 补：① _last_report_date Dolt subprocess 分支 :79-103（temp dolt dir 或 mock subprocess.run，参考 test_import_to_dolt.py 模式）；② Throttle.acquire wait 分支 :118-119（Throttle(min_interval>0) + mock asyncio.sleep）；③ fetch_period guard :196-205（success=False/result=None/items 空）；④ 默认 years :278（不传 --years，mock datetime）；⑤ incremental 模式 :298-306（sys.argv patch + monkeypatch _last_report_date）；⑥ main 循环错误路径 :334-336/:344。
  Must NOT do: 不改 fetch_fin_indicators.py 生产代码；不删现有测试。
  Parallelization: Wave 1 | Blocked by: — | Blocks: —
  References: collectors/fetch_fin_indicators.py:79-103,118-119,196-205,278,298-306,334-344; collectors/tests/test_fin_indicators.py:1-240; collectors/tests/test_import_to_dolt.py:92-211（temp dolt 模式）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_fin_indicators.py -q` 全过；term-missing 中 fetch_fin_indicators.py 缺失行不含上述行号
  QA scenarios: happy — incremental 模式打印 "No new report periods"；failure — subprocess.run 抛错走 fallback。Evidence .omo/evidence/task-3-data-coverage-95.txt
  Commit: Y | test(collectors): cover fin_indicators incremental, retry and guard branches

- [ ] 4. fetch_stock_basic_official.py 网络层真实测试（新增 sync StubSession）
  What to do: ① collectors/tests/conftest.py 新增 `SyncStubSession`/`SyncStubResponse`（同步 requests.Session 风格，支持 .get/.post + canned 响应 + raise_for_status），供 fetch_stock_basic_official.py 使用；② 在 collectors/tests/test_stock_basic_official.py 补：_with_retry :141-156（成功/失败重试/最终 raise）；fetch_sse :429-441（requests.get mock）；fetch_szse_xlsx :444-470（mock requests + zip BytesIO 构造）；fetch_bse :473-529（分页 + .post）；main() :535-624（sys.argv patch + 3 个 fetcher stub + 断言 CSV 输出）；解析器 guard :105/:194/:261/:308/:346/:356。
  Must NOT do: 不改 fetch_stock_basic_official.py 生产代码（L426"非测试范围"注释不更新、不 pragma 网络层）；不 mock 掉整个 requests 库的其它调用面。
  Parallelization: Wave 1 | Blocked by: — | Blocks: —
  References: collectors/fetch_stock_basic_official.py:127-156,429-529,535-628; collectors/tests/test_stock_basic_official.py:1-388（纯解析器测试现状）; collectors/tests/conftest.py:13-99（现有 async stub 参考）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_stock_basic_official.py -q` 全过；term-missing 中 fetch_stock_basic_official.py 缺失行 ≤15
  QA scenarios: happy — _with_retry 重试后成功返回；failure — 超过 MAX_RETRIES 抛异常；fetch_bse 分页聚合 2 页。Evidence .omo/evidence/task-4-data-coverage-95.txt
  Commit: Y | test(collectors): cover stock_basic_official network layer with sync stubs

- [ ] 5. `__main__` 块 + 不可达死代码 pragma 补齐
  What to do: 在 4 处脚本入口加 `# pragma: no cover` 并注明理由（对齐 main.py:448 既有惯例）：fetch_stock_basic_official.py:628、fetch_fin_indicators.py:365、fetch_concept_member.py:293-299（_main + asyncio.run 块）、fetch_stock_basic.py:256。另 fetch_stock_basic_official.py:155-156（mypy 必需的不可达 assert/raise 死代码）加 pragma。注释格式：`# pragma: no cover — __main__ block, never executed under pytest` / `# pragma: no cover — unreachable mypy-required code`。
  Must NOT do: 不 pragma 其它可测代码；不改逻辑。
  Parallelization: Wave 2 | Blocked by: — | Blocks: —
  References: collectors/main.py:448（既有 pragma 先例）; collectors/fetch_stock_basic_official.py:628,155-156; collectors/fetch_fin_indicators.py:365; collectors/fetch_concept_member.py:293-299; collectors/fetch_stock_basic.py:256
  Acceptance criteria (agent-executable): grep 确认 5 处 pragma 均带理由注释；pytest --cov 通过且缺失行减少约 11 stmts
  QA scenarios: happy — grep -n "pragma" collectors/*.py 显示 5 处；failure — 无。Evidence .omo/evidence/task-5-data-coverage-95.txt
  Commit: Y | test(collectors): mark __main__ blocks and unreachable code no-cover

- [ ] 6. check-coverage.sh per-crate 阈值重构 + ci.yml 更新
  What to do: 重构 scripts/check-coverage.sh：移除单一 THRESHOLD 参数，改为内嵌阈值映射 `declare -A THRESHOLDS=( [workspace]=80 [compass-core]=95 [compass-data]=95 [compass]=80 [compass-strategy]=80 [compass-types]=80 [compass-ui]=80 )`，check() 接收 target 名查表；MISSING/FAIL 逻辑不变；用法注释更新。更新 .github/workflows/ci.yml:129-130 coverage step：`bash scripts/check-coverage.sh target/llvm-cov/coverage.json`（无阈值参数）；:156 Python `--cov-fail-under=80` → `--cov-fail-under=95`。
  Must NOT do: 不改变 workspace total 80；不改其它 crate 阈值；不删 jq 依赖检查。
  Parallelization: Wave 2 | Blocked by: — | Blocks: —
  References: scripts/check-coverage.sh:1-69; .github/workflows/ci.yml:129-130,156
  Acceptance criteria (agent-executable): `bash -n scripts/check-coverage.sh` 通过；用现有 cov.json（若存在）或构造最小样例验证 per-crate 判定；ci.yml YAML 语法有效
  QA scenarios: happy — 构造 data 90% 的假 cov.json，脚本输出 FAIL: compass-data；failure — 全 95%+ 输出全部 OK 退出 0。Evidence .omo/evidence/task-6-data-coverage-95.txt
  Commit: Y | ci: per-crate coverage thresholds (data/core 95) and raise python cov-fail-under to 95

- [ ] 7. 文档同步：kb/dev/testing.md + AGENTS.md + kb/design 决策记录检查
  What to do: 更新 kb/dev/testing.md 覆盖率章节（原"≥80%"→ per-crate 描述：workspace 80 / compass-data+core 95 / 其余 80；Python --cov-fail-under=95）；更新 AGENTS.md 覆盖率门槛段落（"每 crate 各自行覆盖率 ≥80%" → per-crate 阈值）；检查 kb/design/ 下相关文件（data-providers.md 若有覆盖率提及）含 `## 决策记录` 章节，缺失则补齐（记录本 PR 的 per-crate 阈值决策 what+why+why-not）。
  Must NOT do: 不改 kb/ 之外的文件；不删除既有文档内容。
  Parallelization: Wave 3 | Blocked by: — | Blocks: —
  References: kb/dev/testing.md（覆盖率章节）; AGENTS.md（Testing 段）; kb/design/data-providers.md（决策记录检查）; .omo/plans/data-coverage-95.md（决策表）
  Acceptance criteria (agent-executable): grep kb/dev/testing.md 与 AGENTS.md 确认 95/per-crate 描述就位；grep kb/design/ 相关文件确认 `## 决策记录` 存在
  QA scenarios: happy — grep 命中；failure — 无。Evidence .omo/evidence/task-7-data-coverage-95.txt
  Commit: Y | docs: sync coverage threshold docs (per-crate 95)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 逐 todo 核对实现与验收标准（grep 证据）
- [ ] F2. Code quality review — 测试遵循既有模式（StubSession/patch 风格一致），无 slop
- [ ] F3. Real QA — 全量跑 `cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95` 必须通过；`bash -n scripts/check-coverage.sh`；`cargo doc --no-deps` 无警告（Rust 侧未动代码，仅确认）
- [ ] F4. Scope fidelity — 确认无 Rust 测试改动、无生产代码重构、无整层 pragma、workspace total 仍 80

## Commit strategy
- 单 PR（ref #163），每 todo 一个 commit，`ref #163` 必须出现
- commit 顺序：test 类 commit（1-5）→ ci commit（6）→ docs commit（7）
- 全部 commit 后运行 /review-work（5 并行 agent），发现问题修复并重新 commit（最多 2 轮）
- push 前 /reflect 写反思 commit（ref #119 教训：反思随 PR 推送）
- push 后创建 PR → merge → 追加完成 comment → 关闭 issue #163 → 关闭 worktree

## Success criteria
- [ ] `cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95 --cov-report=term-missing` 退出 0（覆盖率 ≥95%）
- [ ] `bash scripts/check-coverage.sh <cov.json>` 对 data/core 用 95 阈值、其余 80、workspace 80
- [ ] `cargo doc --no-deps` 无警告
- [ ] kb/dev/testing.md 与 AGENTS.md 覆盖率门槛段落已更新
- [ ] 单 PR 合并到 master，issue #163 关闭，worktree 清理
