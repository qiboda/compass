# data-trim-hook-batch - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 两个独立修复合入一个 PR：① 数据采集端所有写库的文本字段统一去除前后空格（修掉题材标签的"拉伸空格"根因）；② 提交/推送前的 issue 校验从"每个 issue 查一次 GitHub API"改为"一次批量查询"，消除限流误报。

**Why this approach:** ① 在 SQL 写入层做 TRIM 是单点修复——一处改动覆盖所有导入路径，且与已落地的题材表先例（concept_member）一致，不重导现有数据；② `gh issue list` 一次拉取全部 open issue 号、本地查集，把无界的 API 调用降为每次提交/推送一次，根治限流误报。

**What it will NOT do:** 不清理股票代码/日期类规范化字段（symbol、ts_code、code、日期列）；不动 GITHUB_TOKEN 处理逻辑；不重导现有数据库（脏数据计数为 0 的前提下）；不新增共享脚本（两个 hook 各自内联修改）。

**Effort:** Short（2 个独立模块，各为清晰的机械改动 + 测试）
**Risk:** Low-Medium — 主要风险是财务三表列清单不一致（现金流水/利润表无上市状态列，误用会整表导入失败）与调研发现的聚合去重陷阱，计划已逐一锁定
**Decisions to sanity-check:** ① 财务三表 TRIM 列清单逐表独立（勿复制）；② 机构调研表的去重键同步 TRIM（否则 'A'/'A ' 分两组残留）；③ 批量查询失败时拒绝全部提交（fail-closed，与现状一致）

Your next move: 批准本计划后按门禁继续（对抗性测试 → 需求测试 → 实现）。Full execution detail follows below.

---

> TL;DR (machine): Short effort, Low-Medium risk — 7 commits + docs: SQL-layer TRIM on all Dolt text columns (issue #235) + batch `gh issue list` hook validation (issue #213).

## Scope

### Must have
- **C1 (#235)**：collectors 写 Dolt 的 VARCHAR 文本列在 INSERT SELECT 中加 SQL 层 `TRIM()`，覆盖：
  - `stock_basic`（main.py:63-73）：`name, board, full_name, industry, region`（5 列）
  - `fin_indicators`（main.py:147-171）：`name(←SECURITY_NAME_ABBR), industry(←PUBLISHNAME), board_name(←BOARD_NAME 同源高风险), trade_market, trade_market_zjg, security_type, data_type, qdate, date_label, dividend_plan, dividend_year`（文本类列）
  - `fin_balance_sheet`（fetch_balance_sheet.py:770-777）：`SECURITY_NAME_ABBR, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, CURRENCY, OPINION_TYPE, LISTING_STATE`（7 列，**含 LISTING_STATE**）
  - `fin_cash_flow`（fetch_cash_flow.py:702-707）：`SECURITY_NAME_ABBR, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, CURRENCY, OPINION_TYPE`（6 列，**无 LISTING_STATE**）
  - `fin_income`（fetch_income.py:531-536）：`SECURITY_NAME_ABBR, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, CURRENCY, OPINION_TYPE`（6 列，**无 LISTING_STATE**）
  - `institution_survey`（fetch_institution_survey.py:134-150）：`org_name(←TRIM(RECEIVE_OBJECT)), survey_type(←TRIM(RECEIVE_WAY_EXPLAIN))`，**且 GROUP BY 键须为 `HEX(TRIM(RECEIVE_OBJECT))`**（否则 'A'/'A ' 分两组残留）
  - `block_trade`（fetch_block_trade.py:146-153）：`buyer(←TRIM(BUYER_NAME)), seller(←TRIM(SELLER_NAME))`
- **C1 回归测试（RED first）**：仿 test_concept_member.py:153-182 模式，空白输入 → Dolt 落库值无空格；覆盖上述全部表
- **C1 现库脏数据验证**：`dolt sql` 逐列 `WHERE col <> TRIM(col)` 计数，0 则维持"不重导"决策；非 0 则向用户提出重导决策
- **C2 (#213)**：commit-msg + pre-push 的 `gh issue view` 逐条循环改为**单次 `gh issue list --repo qiboda/compass --state open --json number --limit 5000`** 构建 OPEN 集合 + 本地 `grep -qx` 查集（fail-closed：list 失败 → 拒绝全部 ref）
- **C2 测试（RED first）**：更新 pre-push-no-ci-check-test.sh:53 断言（`gh issue view` → `gh issue list`）+ 新增 mock-gh 行为测试（fake gh 命令注入 PATH，断言确切命令与 OPEN 判定）
- **C2 防漂移**：hook-standalone-ref-test.sh mirror-drift guard 增加对 `gh issue list --repo qiboda/compass --state open --json number --limit` 片段的两文件 grep 断言
- **文档同步**：kb/design/data-providers.md（采集器清洗行为）+ kb/user/cli.md（若涉及）+ kb/dev/process.md（hook 行为变更）
- **GUI 冒烟验收（handoff 要求）**：`scripts/run.sh` 验证题材 Tag 无拉伸空格；本地无显示服务器时记录"跳过"或以像素采样证据替代，如实记录
- **批次**：1 分支 + 1 PR + 7 commit（各 `ref #N`）；PR 标签 #235→A-Data,C-Bug；#213→A-CI,C-Chore

### Must NOT have (guardrails, anti-slop, scope boundaries)
- **#235**：不 TRIM symbol/code/ts_code 及派生标识符列（symbol 由 CONCAT 派生，stock_basic 的 ts_code/code 已规范化）——避免误伤前缀逻辑
- **#235**：不 TRIM 三表的 `SECUCODE, SECURITY_CODE, SECURITY_TYPE_CODE, ORG_CODE, NOTICE_DATE, UPDATE_DATE`（标识符/日期类列）
- **#235**：不 TRIM stock_basic 的 `list_date, delist_date`（DATE 类型列，TRIM 会类型报错）
- **#235**：不 TRIM dragon_list.seat_type（Python 派生固定枚举）
- **#235**：三表不得按同一 COLS 清单处理（cash_flow/income 无 LISTING_STATE）
- **#235**：不重导 Dolt 现库（除非脏数据计数非 0 且用户批准）
- **#213**：不动 GITHUB_TOKEN unset 逻辑（保持 `unset GITHUB_TOKEN 2>/dev/null;` 前缀）
- **#213**：不提取共享脚本（F2=B，用户确认内联重复）
- **#213**：不允许用外部 jq——必须用 gh 内建 `--jq '.[].number'`
- **C2 验收不得只依赖字符串存在性断言**（check_present 会放过 `--limit` 参数遗漏）——必须有行为级 mock 测试
- 不创建新 worktree/新分支（本 worktree 承载全部）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（RED→GREEN）。Python: pytest（collectors/tests，`uv run pytest tests/ -q`）；Shell: `bash scripts/tests/*.sh`
- Evidence: `.omo/evidence/task-<N>-data-trim-hook-batch.<ext>`
- 提交前完整门禁：`cd collectors && uv run ruff check *.py tests/ && uv run pytest tests/ -q`；`bash scripts/tests/hook-standalone-ref-test.sh && bash scripts/tests/pre-push-ref-regex-test.sh && bash scripts/tests/pre-push-no-ci-check-test.sh && bash scripts/tests/gh-issue-list-test.sh`
- **真实数据冒烟（提交前强制）**：对受影响表跑 `dolt sql` 脏数据计数 SQL（逐列 `WHERE col <> TRIM(col)`），记录计数证据到 .omo/evidence/；非 0 即 STOP 向用户报告

## Execution strategy

### Parallel execution waves
- **Wave 1（C2 #213，独立小改动先行）**：Todos 1-4
- **Wave 2（C1 #235，数据 TRIM）**：Todos 5-9
- Wave 1 与 Wave 2 无共享文件（.githooks/ vs collectors/），理论可并行，但**推荐顺序执行**——先完成 #213 使 hook 走批量查询，后续 commit 校验更稳

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (C2 RED) | 门禁 3.5/4 步 RED 测试产出 | 2,3 | — |
| 2 (commit-msg) | 1 | 4 | 3 |
| 3 (pre-push) | 1 | 4 | 2 |
| 4 (C2 回归+文档) | 2,3 | — | — |
| 5 (C1 RED) | 门禁 3.5/4 步 RED 测试产出 | 6,7,8 | — |
| 6 (main.py TRIM) | 5 | 9 | 7,8 |
| 7 (三表 TRIM) | 5 | 9 | 6,8 |
| 8 (institution/block TRIM) | 5 | 9 | 6,7 |
| 9 (脏数据验证+GUI 冒烟+文档) | 6,7,8 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. C2 RED：更新 pre-push-no-ci-check-test.sh:53 断言 + 新增 mock-gh 行为测试（gh-issue-list-test.sh）
  What to do / Must NOT do: ① 将 `scripts/tests/pre-push-no-ci-check-test.sh:53` 的 `check_present "ref #N validation" 'gh issue view'` 改为 `'gh issue list'`（先改测试制造 RED）② 新建 `scripts/tests/gh-issue-list-test.sh`：定义 fake `gh` 函数注入 PATH（记录调用参数 + 返回 canned JSON），分别对 `.githooks/commit-msg` 与 `.githooks/pre-push` 断言：调用确为 `gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number'`（**含 `--repo qiboda/compass`——与现有 L43/L148 的 `--repo` 模式一致、与 todo 2/3 实现逐字一致**；含 `--limit 5000` 与 `--jq`）、OPEN issue 通过、非 OPEN/MISSING 拒绝、gh 失败（fake 返回非零）拒绝全部（fail-closed）、gh 成功但 open_set 为空（fake 返回空 JSON）→ 拒绝（fail-closed）。Must NOT: 不在此 todo 改 .githooks/ 文件；不依赖外部 jq；不 mock GITHUB_TOKEN unset 逻辑（fake gh 前置即可）；断言不得放宽为子串匹配（否则放过参数遗漏）。
  Parallelization: Wave 1 | Blocked by: 门禁 3.5/4 步 RED 测试 | Blocks: 2,3
  References: `.githooks/commit-msg:42-58`、`.githooks/pre-push:147-162`、`scripts/tests/pre-push-no-ci-check-test.sh:50-54`（check_present 用法）、`scripts/tests/hook-standalone-ref-test.sh:17-61`（FAIL/check 模式）、`gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number'`（已验证 12 个 open issues 逐行输出，见会话记录）
  Acceptance criteria (agent-executable): `bash scripts/tests/pre-push-no-ci-check-test.sh` 在未改 hook 前 FAIL（断言 gh issue view 不再存在）→ 证明 RED；`bash scripts/tests/gh-issue-list-test.sh` 在未改 hook 前 FAIL（hook 仍调 gh issue view）
  QA scenarios (name the exact tool + invocation): happy: fake gh 返回含 235 的 JSON → commit-msg 接受含 `ref #235` 的 commit msg（`bash scripts/tests/gh-issue-list-test.sh` 全 PASS）。failure: fake gh 返回空/非零 → 断言 hook 拒绝（exit 1）。Evidence: `.omo/evidence/task-1-data-trim-hook-batch.txt`（RED 失败输出）
  Commit: N（随 2/3 一起提交）

- [ ] 2. C2 实现：.githooks/commit-msg:42-58 改为批量查询
  What to do / Must NOT do: 将 `for n in $issues` 循环内的 `gh issue view "$n"` 逐条调用替换为：循环前执行一次 `open_set=$(unset GITHUB_TOKEN 2>/dev/null; gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number' 2>/dev/null || echo "GH_FAIL")`；若 `open_set == "GH_FAIL"` → 报错退出（fail-closed，提示"gh issue list 调用失败，请检查网络/凭据"，与现状 MISSING 拒绝语义一致）；若 `open_set` 为空（成功但 0 个 open issue）→ 报错退出（fail-closed，提示"无 OPEN issue"）；否则循环内 `echo "$open_set" | grep -qx "$n"` 判定。错误信息保持区分："不在 OPEN 集合"→ 提示 reopen 或不存在（保持现 commit-msg:49-53 文案语义，若无法区分则合并为"不在 OPEN issue 集合"）。Must NOT: 不动 L20 存在性检查与 L35-39 提取管道；不引入外部 jq；不删除 unset 前缀；不得用 `grep -c` 之外的方式验证——验收用 `grep -c 'gh issue view'` 期望输出 0（exit 1 属预期，勿误判为失败）。
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 4
  References: `.githooks/commit-msg:34-58`（现逐条 view 循环）、`.githooks/pre-push:147-162`（同构代码，逐字相同）、`kb/dev/toolchain.md:289-304`（MISSING 误报根因）、handoff.md #213 方案
  Acceptance criteria (agent-executable): `bash scripts/tests/gh-issue-list-test.sh` 中 commit-msg 相关断言全 PASS；`bash scripts/tests/pre-push-no-ci-check-test.sh` PASS（断言 gh issue list 存在）；`grep -c 'gh issue view' .githooks/commit-msg` 返回 0
  QA scenarios: happy: fake gh 返回含 235 → 含 `ref #235` 的 commit 通过。failure: fake gh 失败 → commit 拒绝 exit 1。Evidence: `.omo/evidence/task-2-data-trim-hook-batch.txt`
  Commit: Y | fix(hooks): batch-query open issues in commit-msg validation (ref #213)

- [ ] 3. C2 实现：.githooks/pre-push:147-162 改为批量查询（与 2 同构）
  What to do / Must NOT do: 在 `while read` 循环（L73-163）**之外**执行一次 `open_set=$(unset GITHUB_TOKEN 2>/dev/null; gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number' 2>/dev/null || echo "GH_FAIL")`（每 push 一次 API 调用，而非每 ref 一次）；若 `open_set == "GH_FAIL"` → 显式报错 + has_error=1（fail-closed，提示"gh issue list 调用失败"，避免报出误导性的 "issue #N is GH_FAIL"）；若 `open_set` 为空 → 显式报错 + has_error=1（fail-closed）；否则循环内 `echo "$open_set" | grep -qx "$n"` 判定 + has_error=1 累积（不退出，收集全部错误，保持现 pre-push 语义）。Must NOT: 不动 L106-123 malformed-ref 检测与 L136-145 无 ref 拒绝；不引入外部 jq；验收用 `grep -c 'gh issue view'` 期望输出 0（exit 1 属预期）。
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 4
  References: `.githooks/pre-push:71-163`（while read 循环 + 现 view 调用）、`.githooks/commit-msg:42-58`（同构实现）、handoff.md #213
  Acceptance criteria (agent-executable): `bash scripts/tests/gh-issue-list-test.sh` 中 pre-push 相关断言全 PASS；`grep -c 'gh issue view' .githooks/pre-push` 返回 0；29 个既有用例（hook-standalone-ref-test.sh + pre-push-ref-regex-test.sh）全 PASS
  QA scenarios: happy: fake gh 返回 235/213 → push 含两 ref 通过且**仅一次** `gh issue list` 调用（fake gh 计数器断言）。failure: fake gh 失败 → push 拒绝 has_error=1。Evidence: `.omo/evidence/task-3-data-trim-hook-batch.txt`
  Commit: Y | fix(hooks): batch-query open issues in pre-push validation (ref #213)

- [ ] 4. C2 收尾：mirror-drift guard 防漂移 + 全量回归 + 文档同步
  What to do / Must NOT do: ① 在 `scripts/tests/hook-standalone-ref-test.sh:159-171` 的 mirror-drift guard 循环中增加对两 hook 文件 grep `gh issue list --repo qiboda/compass --state open --json number --limit` 片段的断言（防一改一漏，**含 --repo**）② 运行全部 hook 测试回归 ③ 更新 kb/dev/process.md 中 hook 行为描述（若有 `gh issue view` 字样 → 改为批量查询说明）。Must NOT: 不改 hook 逻辑本身；不创建共享脚本。
  Parallelization: Wave 1 | Blocked by: 2,3 | Blocks: —
  References: `scripts/tests/hook-standalone-ref-test.sh:159-171`（现 guard 只查 STANDALONE_REGEX）、`kb/dev/process.md:88-111`（pre-push 检查清单）
  Acceptance criteria (agent-executable): `bash scripts/tests/hook-standalone-ref-test.sh && bash scripts/tests/pre-push-ref-regex-test.sh && bash scripts/tests/pre-push-no-ci-check-test.sh && bash scripts/tests/gh-issue-list-test.sh` 全部 exit 0
  QA scenarios: happy: 全量 29+ 用例 PASS。failure: 任一脚本非零退出即 FAIL。Evidence: `.omo/evidence/task-4-data-trim-hook-batch.txt`
  Commit: Y | docs: sync hook batch-validation behavior (ref #213)

- [ ] 5. C1 RED：TRIM 回归测试（各表空白输入 → 落库无空格）
  What to do / Must NOT do: 仿 `collectors/tests/test_concept_member.py:153-182`（`TestImportToDolt` 类 + dolt_env + csv 写入 + 断言 SELECT 落库值）为下列表新增/扩展测试，输入含前导/尾随空格 → 断言落库无空格：`stock_basic`（name/board/full_name/industry/region）、`fin_indicators`（name/board_name/industry）、三表（SECURITY_NAME_ABBR 等文本列）、`institution_survey`（org_name/survey_type，含 'A'/'A ' 合并断言）、`block_trade`（buyer/seller，含 DISTINCT 合并断言——**注意**：DISTINCT 合并仅当输入行其余列全等时成立，测试输入须构造其余列相同的两行，仅 buyer/seller 空格不同）。**每个表至少 1 个用例用 ASCII 空格（U+0020），另至少 1 个用例用全角空格 U+3000（如 '机构\u3000'）**。**U+3000 用例断言语义（Dolt 实证：`TRIM()` 与 `TRIM(BOTH ' ' FROM col)` 均不除 U+3000，须显式 `REPLACE` 才去除）：本计划锁定纯 `TRIM()`（与 #235 决策一致），故 U+3000 用例断言**保留 U+3000**（即 `SELECT col` 返回值等于 `'机构' || CHAR(0x3000)`）——作为**已知盲区锁定**（防未来实现静默改变 TRIM 语义），该用例为 PASS 的 characterization test，**不构成 RED**；RED 仅由 ASCII 空格用例承担。若后续决定扩展为 REPLACE 变体（需用户批准），再改此断言为"去除"。**测试写法注意**：stock_basic/fin_indicators 走无参导入函数（`_import_stock_basic()`/`_import_fin_indicators()` + `collectors/tests/conftest.py` 的 `_isolate_csv_dir` autouse fixture 指向 tmp_path，写固定文件名 stock_basic_official.csv / RPT_LICO_FN_CPD.csv，仿 test_main.py:877-960）；三表/block_trade/institution_survey 走 `import_to_dolt(csv_path)`（仿 test_concept_member.py）。Must NOT: 不改生产代码（RED 阶段）；不 mock 网络（用 stub session）；**不得让 U+3000 用例断言"无空格"**（纯 TRIM 下永远失败，会使 todos 6/7/8 验收"全 PASS"不可达）。
  Parallelization: Wave 2 | Blocked by: 门禁 3.5/4 步 RED 测试 | Blocks: 6,7,8
  References: `collectors/tests/test_concept_member.py:153-182`（dolt_env 模式）、`collectors/tests/test_main.py:811-960`（stock_basic/fin_indicators 无参导入测试模式）、`collectors/tests/conftest.py`（_isolate_csv_dir autouse fixture/StubSession，注意是 `collectors/tests/conftest.py` 非 `collectors/conftest.py`）、`collectors/main.py:46-87`（_import_stock_basic）、`collectors/main.py:133-176`（_import_fin_indicators）、`collectors/fetch_balance_sheet.py:755-783`（import_to_dolt(csv_path)）、`collectors/fetch_institution_survey.py:120-159`、`collectors/fetch_block_trade.py:138-158`
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/ -q` 出现 FAIL（新 TRIM 断言失败，因生产代码未 TRIM）→ 证明 RED
  QA scenarios: happy: 每表带空格输入 → 断言 `SELECT col FROM table` 无空格（RED 失败输出）。failure: 断言本身错误（如列名错）→ 测试报错而非断言失败，需修正。Evidence: `.omo/evidence/task-5-data-trim-hook-batch.txt`
  Commit: N（随 6/7/8 一起提交）

- [ ] 6. C1 实现：main.py 的 stock_basic + fin_indicators INSERT SELECT 补 TRIM
  What to do / Must NOT do: ① `_import_stock_basic`（main.py:63-73）SELECT 中 `name, board, full_name, industry, region` 包 `TRIM()`（5 列；**不得** TRIM symbol/ts_code/code/list_date/delist_date）② `_import_fin_indicators`（main.py:147-171）SELECT 中文本类列包 TRIM：`TRIM(SECURITY_NAME_ABBR), TRIM(PUBLISHNAME), TRIM(BOARD_NAME)`（对应 name/industry/board_name）+ `TRIM(TRADE_MARKET), TRIM(TRADE_MARKET_ZJG), TRIM(SECURITY_TYPE), TRIM(DATATYPE), TRIM(QDATE), TRIM(DATEMMDD), TRIM(ASSIGNDSCRPT), TRIM(PAYYEAR)`（**不得** TRIM SECUCODE/CONCAT 派生 symbol）。Must NOT: 不改 _tmp 表 DDL；不改 DOLT 表 DDL。
  Parallelization: Wave 2 | Blocked by: 5 | Blocks: 9
  References: `collectors/main.py:46-87`（stock_basic）、`collectors/main.py:133-176`（fin_indicators FIN_INDICATORS_DDL:89-130 确认文本列类型）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/ -q` 中 stock_basic/fin_indicators TRIM 测试全 PASS；`uv run ruff check main.py` 干净
  QA scenarios: happy: 测试断言落库无空格 PASS。failure: 误 TRIM symbol → symbol 值异常（测试捕获）。Evidence: `.omo/evidence/task-6-data-trim-hook-batch.txt`
  Commit: Y | fix(collectors): trim stock_basic and fin_indicators string columns (ref #235)

- [ ] 7. C1 实现：三表 INSERT SELECT 补 TRIM（逐表独立，不得共享清单）
  What to do / Must NOT do: 三表各自在 import_to_dolt 的 insert_sql SELECT 中包 TRIM，**逐表核对列清单**：`fin_balance_sheet`（fetch_balance_sheet.py:770-777）7 列 = SECURITY_NAME_ABBR, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, CURRENCY, OPINION_TYPE, **LISTING_STATE**；`fin_cash_flow`（fetch_cash_flow.py:702-707）6 列 = SECURITY_NAME_ABBR, ORG_TYPE, REPORT_TYPE, REPORT_DATE_NAME, CURRENCY, OPINION_TYPE（**无 LISTING_STATE**）；`fin_income`（fetch_income.py:531-536）6 列同上（**无 LISTING_STATE**）。实现方式：由于 INSERT 是 `SELECT CONCAT(...), CAST(REPORT_DATE AS DATE), {COLS}`（COLS 常量含数值列），TRIM 须在 SELECT 列上对文本列单独包 `TRIM(col) AS col` 展开，**不得**对整个 COLS 常量套 TRIM（数值列会报错）。Must NOT: 不 TRIM SECUCODE/SECURITY_CODE/SECURITY_TYPE_CODE/ORG_CODE/NOTICE_DATE/UPDATE_DATE；不把 balance_sheet 清单复制到另两表。
  Parallelization: Wave 2 | Blocked by: 5 | Blocks: 9
  References: `collectors/fetch_balance_sheet.py:362`（COLS 常量，含 LISTING_STATE）、`collectors/fetch_cash_flow.py:291-300`（COLS，无 LISTING_STATE）、`collectors/fetch_income.py:240-241`（COLS，无 LISTING_STATE）、`collectors/fetch_balance_sheet.py:355-357`（OPINION_TYPE/LISTING_STATE VARCHAR DDL）、`collectors/common.py:197-295`（import_replace_table）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_balance_sheet.py tests/test_cash_flow.py tests/test_income.py -q` 中 TRIM 测试全 PASS；ruff 干净
  QA scenarios: happy: 每表带空格输入 → 落库无空格 PASS。failure: 误对 cash_flow 应用 LISTING_STATE TRIM → SQL 报错（列不存在），测试失败暴露。Evidence: `.omo/evidence/task-7-data-trim-hook-batch.txt`
  Commit: Y | fix(collectors): trim financial statement string columns (ref #235)

- [ ] 8. C1 实现：institution_survey + block_trade 补 TRIM
  What to do / Must NOT do: ① `fetch_institution_survey.py:134-150`：SELECT 中 `MAX(TRIM(RECEIVE_OBJECT))`（org_name）、`MAX(TRIM(RECEIVE_WAY_EXPLAIN))`（survey_type），**且内层子查询 `HEX(TRIM(RECEIVE_OBJECT)) AS gk`**（GROUP BY 键同步 TRIM，保证 'A'/'A ' 合并为单组）② `fetch_block_trade.py:146-153`：`TRIM(BUYER_NAME), TRIM(SELLER_NAME)`（SELECT DISTINCT 语义下 'ABC'/'ABC ' 自动合并）。Must NOT: 不 TRIM symbol_expr/日期列；不改 DDL。
  Parallelization: Wave 2 | Blocked by: 5 | Blocks: 9
  References: `collectors/fetch_institution_survey.py:120-150`（MAX 聚合 + gk=HEX(RECEIVE_OBJECT) 陷阱）、`collectors/fetch_block_trade.py:140-158`（SELECT DISTINCT）
  Acceptance criteria (agent-executable): `cd collectors && uv run pytest tests/test_institution_survey.py tests/test_block_trade.py -q` 中 TRIM 测试全 PASS（含 'A'/'A ' 合并断言）
  QA scenarios: happy: '机构专用 ' 与 '机构专用' 输入 → 落库单行 '机构专用'。failure: gk 未 TRIM → 'A'/'A ' 分两组各留一行 → 测试断言行数/去重失败。Evidence: `.omo/evidence/task-8-data-trim-hook-batch.txt`
  Commit: Y | fix(collectors): trim institution survey and block trade strings (ref #235)

- [ ] 9. C1 收尾：现库脏数据计数 + GUI 冒烟 + 文档同步
  What to do / Must NOT do: ① 对每个 TRIM 列跑 **两组** Dolt 脏数据计数 SQL（Dolt 库 /data/compass-data/compass_data）：(a) ASCII 空格集 `SELECT COUNT(*) FROM <table> WHERE <col> <> TRIM(<col>) AND <col> IS NOT NULL`；(b) 宽字符类补充查询（**注意 Dolt 实证：`[[:space:]]` POSIX 类为 ASCII-only（Go RE2），不匹配 U+3000——必须叠加引擎无关谓词**）`SELECT COUNT(*) FROM <table> WHERE <col> REGEXP '^[[:space:]]+|[[:space:]]+$' OR <col> LIKE CONCAT('%', CHAR(0x3000), '%')`（后者覆盖 U+3000 全角空格，CHAR() 构造规避客户端编码问题；可选追加 `CHAR(0x00A0)` 覆盖不间断空格）。全部 0 → 记录证据、维持"不重导"；任一非 0 → **STOP** 向用户报告（含 `SELECT HEX(<col>)` 确认字符类——**注意 Dolt 的 HEX() 返回码点十六进制如 '3000'/'4E2D'，非 MySQL 字节序 'E38080'，勿据此构建字节模式谓词**）并请求重导决策（不静默重导）② GUI 冒烟：本地无显示服务器 → 记录"跳过（无显示服务器）"；有则 `scripts/run.sh` 验证题材 Tag 无拉伸空格，像素采样证据存档——注意：冒烟属**症状回归**（验证的是 concept_member 旧先例的显示效果），新增 TRIM 表的端到端验证由 pytest 承担；且"不重导"前提下 Parquet 可能过期，若冒烟显示残留空格先核对 Parquet 数据源是否陈旧 ③ 文档同步：kb/design/data-providers.md（采集器行为变更：SQL 层 TRIM 策略 + 列清单 + **TRIM 语义仅去 ASCII 空格、U+3000 需 REPLACE 的说明**）、kb/user/cli.md（若含采集器说明）、kb/dev/process.md（#213 已由 todo 4 处理，此处核查）。Must NOT: 不重导数据除非用户批准；不跳过脏数据计数直接声明"无需重导"；**不得只用 `col <> TRIM(col)` 单一谓词、也不得只用 ASCII-only 的 REGEXP（均会漏 U+3000）**。
  Parallelization: Wave 2 | Blocked by: 6,7,8 | Blocks: —
  References: `/data/compass-data/compass_data`（Dolt 库）、`kb/design/data-providers.md`（Schema/清洗章节）、`kb/user/cli.md`、`kb/dev/process.md`、handoff.md:27-28（GUI 冒烟验收）
  Acceptance criteria (agent-executable): 脏数据计数证据落盘 `.omo/evidence/task-9-data-trim-hook-batch.txt`（每列计数行）；GUI 冒烟记录（跳过或像素证据）；kb/ 更新 diff 可见
  QA scenarios: happy: 全部计数 0 → 记录并继续。failure: 计数非 0 → STOP 报告用户。Evidence: `.omo/evidence/task-9-data-trim-hook-batch.txt`
  Commit: Y | docs: sync collector trim behavior and hook batch validation (ref #235, #213)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 逐 todo 核对验收标准证据（.omo/evidence/ 文件齐全、RED→GREEN 输出存在、脏数据计数落盘）
- [ ] F2. Code quality review — `/review-work` 或同等并行审查（Python ruff/pytest、shell 测试、hook 语义）；重点：三表列清单无跨表复制错误、institution_survey gk 键（**核验 GROUP BY 是否含第二列如 RECEIVE_WAY_EXPLAIN，若有则其 'A'/'A ' 分组合并需同样处理**）、fail-closed 语义
- [ ] F3. Real manual QA — 真实数据冒烟：`uv run pytest tests/ -q` 全绿 + hook 29+ 用例全绿 + `dolt sql` 脏数据计数 = 0 + GUI 冒烟记录
- [ ] F4. Scope fidelity — git diff 核对：无 symbol/code/list_date TRIM、无 LISTING_STATE 误入 cash_flow/income、无共享脚本、GITHUB_TOKEN unset 未动、29 用例未破坏

## Commit strategy
- 7 个 commit（2 hook 实现 + 1 hook docs + 3 collector 实现 + 1 收尾 docs；各含独立行 `ref #N`，独立成行）：
  1. `fix(hooks): batch-query open issues in commit-msg validation (ref #213)`（todo 2）
  2. `fix(hooks): batch-query open issues in pre-push validation (ref #213)`（todo 3）
  3. `docs: sync hook batch-validation behavior (ref #213)`（todo 4）
  4. `fix(collectors): trim stock_basic and fin_indicators string columns (ref #235)`（todo 6）
  5. `fix(collectors): trim financial statement string columns (ref #235)`（todo 7）
  6. `fix(collectors): trim institution survey and block trade strings (ref #235)`（todo 8）
  7. `docs: sync collector trim behavior and hook batch validation (ref #235, #213)`（todo 9）
- RED 测试（todo 1/5 的测试代码）随对应实现 commit 一起提交（测试先行，实现后同一 commit 收 GREEN）
- 每个 commit 后运行 `/review-work`；commit-msg hook 校验 ref 指向 OPEN issue（#235/#213 均 OPEN）
- **Never auto-push**：用户明确指示 push 才 push；push 前 rebase origin/master + `/skwy-reflect` 反思 commit

## Success criteria
- [ ] `cd collectors && uv run ruff check *.py tests/` 干净；`uv run pytest tests/ -q` 全绿（含新增 TRIM 测试）
- [ ] `bash scripts/tests/hook-standalone-ref-test.sh && bash scripts/tests/pre-push-ref-regex-test.sh && bash scripts/tests/pre-push-no-ci-check-test.sh && bash scripts/tests/gh-issue-list-test.sh` 全绿
- [ ] `.githooks/commit-msg` 与 `.githooks/pre-push` 无 `gh issue view` 残留；每 push 仅 1 次 `gh issue list` API 调用
- [ ] Dolt 现库各 TRIM 列脏数据计数 = 0（证据落盘）或已向用户提出重导决策
- [ ] GUI 题材 Tag 无拉伸空格（冒烟验证或如实记录跳过）
- [ ] kb/ 文档同步完成（data-providers.md / cli.md / process.md）
- [ ] 1 PR（含上述 commits），#235 → A-Data,C-Bug；#213 → A-CI,C-Chore
- [ ] push 后按流程关闭 issue（先 #213 后 #235 或按 PR 合并顺序，各附完成 comment）
