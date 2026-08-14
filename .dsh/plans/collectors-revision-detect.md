# collectors-revision-detect - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 财报数据采集器（fin_indicators）的增量模式从"按报告期过滤"升级为"按数据更新时间增量"——上市公司修订历史财报（如五粮液 2025Q1）后，下次同步能自动检测并覆盖旧数据，无需定期全量重拉。

**Why this approach:** 东财数据源提供 UPDATE_DATE（每行数据的更新时间），API 实测支持按它过滤；Dolt 库的 data_updates 表记录上次成功导入日期，可作增量锚点。只抓"有更新的行"比全量重拉精准得多。

**What it will NOT do:** 不实现 `--refresh N` 无条件重拉（被本机制取代）；不改动其他三张财务表（它们用全表重建已能覆盖修订）；不新增 Dolt 表/列；不涉及 GUI 和 Rust 侧。

**Effort:** Medium
**Risk:** Medium - 锚点选择正确性（UPDATE_DATE 预标日期陷阱）+ UPSERT 写法在 Dolt 上的兼容性（已实测两种写法）
**Decisions to sanity-check:** (1) 锚点用 `min(data_updates.last_updated, state.json.last_update_date)` 而非 `MAX(update_date)`（预标日期会漏行；min 防跨日/单独 import 锚点超前）；(2) **UPSERT 写法 = SELECT 侧全列别名 + ODKU 无前缀别名引用**（Round 2 双审实证：Dolt 2.2.3 的限定源列引用 `_tmp_fin.COL` 对 TRIM 包装的文本列解析失败，报 `table _tmp_fin does not have column`；`VALUES()` 同样不支持。已独立实测通过的写法：`SELECT ... AS _nm, ... ON DUPLICATE KEY UPDATE name=_nm`——全列覆盖含 TRIM 文本列成功；备选 `REPLACE INTO` 亦可，但语义为 delete+insert）；(3) **C4 决策 B（用户确认）：不做过渡回补**——锚点直接取当前锚点（约 2026-08-03），仅对新修订生效；存量 stale 行风险自担（已记录为已知限制）

Your next move: 阅读下方执行细节后批准开始执行（`$start-work`）。

---

> TL;DR (machine): Medium effort, Medium risk — fin_indicators 增量模式改 UPDATE_DATE 时间锚点（min 双源）+ Dolt UPSERT（SELECT 别名引用写法）覆盖修订，替代 #27。

## Scope
### Must have
- `fetch_fin_indicators.py --incremental`：增量模式改为按 UPDATE_DATE 时间锚点过滤抓取（替代 REPORTDATE 枚举）
- 锚点读取：**`min(data_updates.last_updated, state.json.last_update_date)`**（双源取较早者——防跨日 fetch/import 或单独 import 导致的锚点超前漏抓；Oracle 双审发现 `import_replace_table` 无条件推进 last_updated=CURDATE()，若只跑 import 或跨日则锚点会越过 fetch-import 间隙内更新的行）。两源皆无 → fallback 全量 REPORTDATE 枚举。经 `common.dolt_dir()`/`dolt_sql_csv`（env-aware）
- `main.py _import_fin_indicators`：INSERT IGNORE → UPSERT，修订值覆盖旧 PK 行。**写法（Round 2 双审实证，Dolt 2.2.3）**：SELECT 侧给每个输出列加唯一别名（`AS _sym/_rpt/_nm/...`，与目标列名不同，防 ambiguity），ODKU 用**无前缀别名引用**（`ON DUPLICATE KEY UPDATE revenue=_rev, name=_nm, ...`）——限定源列引用 `_tmp_fin.COL` 对 TRIM 包装文本列解析失败、`VALUES()` 不受支持，均禁止。UPDATE 子句必须**覆盖全部 35 个非 PK 值列**（DDL 37 列 − 2 PK，无一遗漏——漏列会静默混合新旧值）；TRIM 由 SELECT 侧完成（TRIM 后的值经别名赋给 UPDATE，天然一致）
- CSV 写入：每 PK 唯一（整文件 keep-LAST 去重，所有写入路径适用），去重键 (SECURITY_CODE, REPORTDATE)
- state.json：新增 `last_update_date`，与 `last_report_date` 双写（向后兼容）
- 测试：RED first（修订覆盖端到端、锚点三分支、UPSERT 写法、CSV 去重、非增量不变、回归）
- 文档：`kb/user/cli.md`、`kb/design/data-providers.md`（决策记录修订）、fetch_fin_indicators.py docstring（移除 KNOWN LIMITATION/TODO）、**common.py/main.py 的 import_replace_table / _import_fin_indicators docstring（INSERT IGNORE 描述改为 UPSERT，Oracle 双审发现会过期）**
- 覆盖率：Python `--cov=. --cov-fail-under=95` 通过

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不实现 `--refresh N`（#27 被 #135 替代，不单独实现）
- 不做过渡期历史回补（C4 决策 B，用户确认 2026-08-12）：锚点直接取当前双源 min（约 2026-08-03），存量 stale 修订行不主动回补（已在 docstring/文档注明该限制）
- 不改模块化三表（income/balance/cash_flow）的 fetch/import 语义（replace 已覆盖修订）
- 不改 Dolt schema（不加 `data_updates.last_update_date` 列，不加任何列）
- 不涉及 GUI / Rust 侧代码
- 不改其他 SEPA 表的 import 语义（main_flow/dragon/block_trade/institution_survey 保持 INSERT IGNORE）
- 锚点读取禁止复制 `_last_report_date` 的 repo 相对路径模式（`Path(__file__).parent.parent/"compass_data"`——实测两处 checkout 均无此目录，该分支从未生效）
- 不改变非增量（默认）模式的抓取语义（REPORTDATE 枚举不变；仅 CSV 写入统一去重）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（RED first）+ pytest-asyncio + stub AsyncSession
- Evidence: `.omo/evidence/task-<N>-collectors-revision-detect.<ext>`（每 todo 的 QA 场景落盘）
- 覆盖率：`cd collectors && uv run pytest --cov=. --cov-report=term-missing --cov-fail-under=95 tests/`
- Lint：`uv run ruff check .`

## Execution strategy
### Parallel execution waves
- **Wave 1 — RED 测试（并行，独立写测试，不碰生产代码）**
  - T1: 修订覆盖端到端 RED（stub 模拟 UPDATE_DATE 变化 → CSV 去重 + Dolt UPSERT → 断言新值生效）
  - T2: 锚点解析四分支 RED（data_updates / state.json / min 双源 / 全量 fallback）
  - T3: UPSERT 写法验证 + merge 语义测试适配（`VALUES()` 失败、限定引用对 TRIM 列失败、**SELECT 别名引用全列覆盖成功**、幂等）
  - T4: CSV 整文件 keep-LAST 去重 RED（增量 + 全量路径、BOM、既有重复）
  - T5: 非增量不变 + 其他表 INSERT IGNORE 回归 RED
- **Wave 2 — GREEN 实现（并行，依赖 Wave 1）**
  - T6: fetch_fin_indicators.py 增量模式重构（UPDATE_DATE 锚点 + 新 fetch 函数 + state.json 双写）
  - T7: main.py `_import_fin_indicators` UPSERT（**SELECT 别名 + ODKU 无前缀别名引用，禁用限定引用/VALUES()**）
  - T8: CSV 去重写实现（common.py 或 fetch 内）
- **Wave 3 — 全量验证 + 文档**
  - T9: 全量 pytest + ruff + 覆盖率 ≥95
  - T10: kb/user/cli.md + kb/design/data-providers.md（决策记录）+ fetch/import docstring 更新
- **Wave 4 — 工作流收尾（主 agent）**
  - T11: /review-work → 用户批准 → rebase → /reflect → push → PR → #135 收尾（+ #27 关闭）

### Dependency matrix
> 注 1：T1（端到端）的 Dolt 覆盖断言需 T6（抓取）+ T7（UPSERT）+ T8（去重）三者齐备才全绿——T1 标记 Blocks T6，但 T1 全绿以 Wave 2 收口（T9 全量门禁）为准。
> 注 2：Wave 1 并行写冲突防护——T1/T2/T4 均向 `tests/test_fin_indicators.py` 追加不同测试类（T1/T2/T4/T6 的验收命令均钉死此文件的 `::TestX` 路径），并行写同一文件有丢失更新/半写风险。**固定路径：T1/T2/T4 串行追加到 `tests/test_fin_indicators.py`（同 Wave 内按 T1→T2→T4 顺序逐个追加，先写先验，不并行写文件），T5 的 `pytest tests/` 全目录运行避开发起时正在写入的文件**。若某执行者确需拆独立测试文件，必须同步修改 T1/T2/T4/T6 的验收 pytest 路径（否则 `::TestX` 无法收集，RED 退化为 "no tests collected"）。
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 | — | T6 | T2, T3, T4, T5 |
| T2 | — | T6 | T1, T3, T4, T5 |
| T3 | — | T7 | T1, T2, T4, T5 |
| T4 | — | T8 | T1, T2, T3, T5 |
| T5 | — | T6, T8 | T1-T4 |
| T6 | T1, T2, T5 | T9 | T7, T8 |
| T7 | T3 | T9 | T6, T8 |
| T8 | T4, T5 | T9 | T6, T7 |
| T9 | T6, T7, T8 | T10, T11 | — |
| T10 | T9 | T11 | — |
| T11 | T9, T10 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. RED: 修订覆盖端到端测试（T1）
  What to do / Must NOT do: 在 tests/test_fin_indicators.py 增加测试：stub AsyncSession 模拟"同一 report_date 的 UPDATE_DATE 变化"（如五粮液 2025Q1 UPDATE_DATE 从 2025-04-26 变 2026-04-30、revenue 从 369.40 变 170.86）；断言增量模式抓取该行、CSV 中该 PK 唯一且为新值、Dolt import 后值已覆盖。不得修改生产代码（RED 阶段）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T6
  References: tests/conftest.py:57-112 (StubSession), tests/test_fin_indicators.py:376-587 (main() 测试模式), test_income.py:112 (UPDATE_DATE 注入格式), collectors/fetch_fin_indicators.py:299-308 (现状增量), collectors/main.py:147 (INSERT IGNORE)
  Acceptance criteria (agent-executable): `uv run pytest tests/test_fin_indicators.py::TestRevisionDetect -x -q` 失败（RED），失败信息指向未实现的修订覆盖行为
  QA scenarios: happy: 修订行被抓取并覆盖（断言 CSV 唯一 + Dolt 值==170.86）；failure: 断言测试在实现前必然失败（RED 证明），Evidence .omo/evidence/task-1-collectors-revision-detect.txt
  Commit: Y | test: fin_indicators revision-overwrite end-to-end RED (ref #135)
- [ ] 2. RED: 锚点解析测试（T2）
  What to do / Must NOT do: 增加锚点解析函数测试：① COMPASS_DATA_DIR 指向 temp Dolt，data_updates 表有 fin_indicators 行 last_updated=2026-08-03 → 锚点返回 "2026-08-03"；② data_updates 无行 + state.json 有 last_update_date=2026-07-01 → 返回 "2026-07-01"（较早有值源优先的 min 语义：data_updates 缺失时取 state）；③ 两源都有但 state 较早（如 state=2026-07-01, data_updates=2026-08-03）→ 返回 min 即 "2026-07-01"（防跨日/单独 import 锚点超前）；④ 两者皆无 → 返回 ""（触发全量 REPORTDATE 枚举）；⑤ **data_updates 有行但 last_updated 为 NULL/空 → 视为该源缺失**（与 common.py:183 last_report_date 模式对称）。断言请求 filter 为 `(UPDATE_DATE>='2026-08-03')`。不得修改生产代码。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T6
  References: common.py:76-95 (dolt_dir/csv_dir env-aware), common.py:171-185 (last_report_date 模式), tests/test_common.py dolt_env 模式（若存在）, tests/test_fin_indicators.py:213-370 (TestLastReportDate 现有模式)
  Acceptance criteria (agent-executable): `uv run pytest tests/test_fin_indicators.py::TestUpdateAnchor -x -q` 失败（RED）
  QA scenarios: happy: 五分支各返回正确锚点（min 语义 + NULL 视为缺失）；failure: temp Dolt 缺 .dolt 时 fallback 路径正确；Evidence .omo/evidence/task-2-collectors-revision-detect.txt
  Commit: Y | test: update-anchor resolution min-of-sources RED (ref #135)
- [ ] 3. RED: UPSERT 写法验证测试（T3）
  What to do / Must NOT do: 在 tests/test_main.py 或 test_common.py 增加测试（temp Dolt 实测）：① **SELECT 别名 + ODKU 无前缀别名引用**写法（`INSERT INTO t (...) SELECT ... AS _sym, ... FROM _tmp_fin ON DUPLICATE KEY UPDATE revenue=_rev, name=_nm`）插入同 PK 旧值后运行 → 断言全列覆盖（数值 369.40→170.86 + 至少两个 TRIM 文本列如 name/data_type 被覆盖，钉住"漏列会静默混合新旧值"）；② 断言 **限定源列引用 `_tmp_fin.COL`** 对 TRIM 包装文本列在 Dolt 报错（钉住兼容性约束，防实现时误用）；③ 断言 `VALUES()` 写法在 Dolt 报错；④ **全行 35 列 round-trip 断言**（35 列清单以 main.py FIN_INDICATORS_DDL 为唯一来源，不从实现代码提取——防循环验证；GREEN 后整行相等校验，机械钉住"无一遗漏"）。适配 test_common.py::TestImportReplaceTableMerge 现有 PIN 测试（若受 UPSERT 影响）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T7
  References: common.py:197-295 (import_replace_table merge 语义), main.py:147-171 (_import_fin_indicators insert_sql), 已验证（/tmp/opencode/dolt-r3 实测）：别名引用全列覆盖成功；限定源列引用对 TRIM 文本列报 `table _tmp_fin does not have column`；VALUES() 报 `__new_ins` 错
  Acceptance criteria (agent-executable): `uv run pytest tests/test_main.py -x -q` 与 `uv run pytest tests/test_common.py -x -q` 通过新增测试且现有 merge 测试保持绿（T3 是写法钉住测试，Wave 1 即绿——非传统 RED，因不依赖生产代码）
  QA scenarios: happy: 别名引用 UPSERT 全列覆盖（数值+2 个 TRIM 文本列+全行 35 列相等）；failure: 限定引用/VALUES() 断言报错；Evidence .omo/evidence/task-3-collectors-revision-detect.txt
  Commit: Y | test: UPSERT alias-ref formulation on Dolt (ref #135)
- [ ] 4. RED: CSV 整文件 keep-LAST 去重测试（T4）
  What to do / Must NOT do: 增加 CSV 去重函数测试：① 同 PK 旧值在前新值在后 → 去重后保留新值；② BOM（utf-8-sig）首行不污染；③ 增量写入与全量写入两路径都去重；④ 既有重复被清理。断言去重键为 (SECURITY_CODE, REPORTDATE)。不得修改生产代码。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T8
  References: fetch_fin_indicators.py:221-233 (write_csv), fetch_fin_indicators.py:341 (append 调用), CSV 实测: RPT_LICO_FN_CPD.csv 头列 SECURITY_CODE,REPORTDATE（无重复 PK，208684 行）
  Acceptance criteria (agent-executable): `uv run pytest tests/test_fin_indicators.py::TestCsvDedup -x -q` 失败（RED）
  QA scenarios: happy: 去重后每 PK 唯一且保留新值；failure: 空文件/无 PK 列文件不崩溃；Evidence .omo/evidence/task-4-collectors-revision-detect.txt
  Commit: Y | test: CSV whole-file keep-LAST dedup RED (ref #135)
- [ ] 5. RED: 非增量不变 + 其他表回归测试（T5）
  What to do / Must NOT do: 增加测试：① 无 --incremental 时 filter 仍为 `(REPORTDATE='...')`（抓取语义不变）；② main_flow/dragon/block_trade/institution_survey 的 import 仍 INSERT IGNORE（锚定现有 PIN 测试不回归）。不得修改生产代码。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T6, T8
  References: fetch_fin_indicators.py:299-308, main.py:288-315 (dispatch_import 各表), common.py:239-252 (merge INSERT IGNORE)
  Acceptance criteria (agent-executable): `uv run pytest tests/ -x -q -k "not revision and not UpdateAnchor and not CsvDedup and not Upsert"` 全部通过（排除 Wave 1 预期 RED/新测试类——TestRevisionDetect/TestUpdateAnchor/TestCsvDedup/T3 的 TestUpsert 等；现有测试不回归）
  QA scenarios: happy: 非增量 filter 断言；failure: 其他表 import 语义回归检测；Evidence .omo/evidence/task-5-collectors-revision-detect.txt
  Commit: Y | test: non-incremental semantics + other-tables regression guard (ref #135)
- [ ] 6. GREEN: fetch_fin_indicators.py 增量模式重构（T6）
  What to do / Must NOT do: 实现 UPDATE_DATE 时间锚点增量：新增锚点解析函数（读 `data_updates.last_updated` + `state.json.last_update_date` 取 **min**（较早者），via `common.dolt_sql_csv`/`dolt_dir`，env-aware；**`last_update_date` 键缺失视为该源缺失**（旧格式 state.json 只有 last_report_date——按无此源处理，退化为 data_updates 单源，保守安全）；两源皆无 → 返回 "" 触发全量；**不得用 MAX(update_date)——预标日期会漏行**）；新增按 UPDATE_DATE 过滤的 fetch 函数（filter=`(UPDATE_DATE>='{anchor}')`，sortColumns=UPDATE_DATE，500 页上限，分页日志）；增量模式下忽略 `--years/--periods`（文档注明）；state.json 双写 last_update_date + last_report_date（UPDATE_DATE 规范化取日期前缀；**0 行运行时不推进锚点——保留原锚点值，避免跳过晚到修订；空 UPDATE_DATE 行跳过 max 计算**）；锚点>今天 或 NULL 时按无更新处理（锚点取今天）。`--report-name` 非 RPT_LICO_FN_CPD 时保持旧行为（不启用 UPDATE_DATE 锚点，沿用 REPORTDATE 逻辑）。**已知行为（写入 docstring）：锚点可能停滞在 max 见过的 UPDATE_DATE（含预标日期），导致每次运行重抓该小窗口——UPSERT 幂等故安全，属预期，勿"优化"为推进到 CURDATE()**。禁止复制 _last_report_date 的 repo 相对路径模式；禁止改动非增量模式 filter。
  Parallelization: Wave 2 | Blocked by: T1, T2, T5 | Blocks: T9
  References: fetch_fin_indicators.py:72-105 (_last_report_date 坏路径——禁用), fetch_fin_indicators.py:146-215 (fetch_period 分页模式), common.py:76-95 (env-aware dolt_dir), common.py:120-131 (dolt_sql/dolt_sql_csv), 实测: API filter=(UPDATE_DATE>='2026-07-01') 可行
  Acceptance criteria (agent-executable): T1+T2 RED 测试转 GREEN；`uv run pytest tests/test_fin_indicators.py -x -q` 全绿；`uv run ruff check fetch_fin_indicators.py`
  QA scenarios: happy: stub 下 filter 为 UPDATE_DATE 锚点 + 修订行覆盖；failure: 锚点未来日期/0 行运行不崩溃且不推进锚点；Evidence .omo/evidence/task-6-collectors-revision-detect.txt
  Commit: Y | feat: fin_indicators incremental by UPDATE_DATE anchor (ref #135)
- [ ] 7. GREEN: main.py _import_fin_indicators UPSERT（T7）
  What to do / Must NOT do: insert_sql 改为 UPSERT。**写法（Round 2 双审实证，Dolt 2.2.3）**：SELECT 侧每个输出列加唯一别名（`AS _sym/_rpt/_upd/_nm/_dt/...`，**别名不得与目标列名相同**——否则会解析为已存在行（no-op）或报 ambiguous 错，两种均不可接受），ODKU 用**无前缀别名引用**（`ON DUPLICATE KEY UPDATE update_date=_upd, name=_nm, ...`）。**禁止** `_tmp_fin.COL` 限定源列引用（对 TRIM 包装文本列报 `table _tmp_fin does not have column`）和 `VALUES()`（报 `__new_ins`）。UPDATE 子句**覆盖全部 35 个非 PK 值列**（DDL 37 − 2 PK，清单以 main.py FIN_INDICATORS_DDL 为唯一来源）；TRIM 由 SELECT 侧完成（含别名的 TRIM 表达式），UPDATE 引用别名即得已 TRIM 值。保持 stock_basic 过滤、merge=True。验证 _tmp_fin 列名与 CSV 头一致（大写 API 名）。
  Parallelization: Wave 2 | Blocked by: T3 | Blocks: T9
  References: main.py:133-176 (_import_fin_indicators), main.py:89-130 (FIN_INDICATORS_DDL 35 值列清单), fetch_income.py:245-260 (TRIM 列清单参照), 已验证（/tmp/opencode/dolt-r3 实测）：别名引用全列覆盖含 TRIM 文本列成功（SZ000858 170.86/五粮液/一季报）
  Acceptance criteria (agent-executable): T3 测试 GREEN；`uv run pytest tests/test_main.py -x -q` 全绿；`uv run ruff check main.py`
  QA scenarios: happy: 修订 PK 被 UPSERT 全列覆盖（数值+文本列，与 SELECT 别名对应）；failure: 无重叠行时纯 INSERT 正常；Evidence .omo/evidence/task-7-collectors-revision-detect.txt
  Commit: Y | feat: fin_indicators import UPSERT for revision overwrite (ref #135)
- [ ] 8. GREEN: CSV 去重写实现（T8）
  What to do / Must NOT do: 实现 CSV 整文件 keep-LAST 去重：每次写入（增量 + 全量）后按 (SECURITY_CODE, REPORTDATE) 去重保留最后出现行；读写用 utf-8-sig（BOM 安全）；空文件/缺列不崩溃。可放 fetch_fin_indicators.py 或 common.py（若通用）。**若放 common.py 共享 write_csv：先验证 income/balance/cash_flow 的原始 CSV 头同为 (SECURITY_CODE, REPORTDATE)（RPT_LICO_FN_CPD 已实测；三表未核验——若键不同则去重会误删合法行），确认行为中性后再复用**。
  Parallelization: Wave 2 | Blocked by: T4, T5 | Blocks: T9
  References: fetch_fin_indicators.py:221-233 (write_csv), common.py:401-413 (write_csv 版本)
  Acceptance criteria (agent-executable): T4 测试 GREEN；`uv run pytest tests/test_fin_indicators.py::TestCsvDedup -x -q` 通过
  QA scenarios: happy: 增量+全量两路径去重；failure: 20 万行文件去重性能可接受（<30s）；Evidence .omo/evidence/task-8-collectors-revision-detect.txt
  Commit: Y | feat: CSV keep-LAST dedup on all write paths (ref #135)
- [ ] 9. 全量验证：pytest + ruff + 覆盖率（T9）
  What to do / Must NOT do: `cd collectors && uv run pytest --cov=. --cov-report=term-missing --cov-fail-under=95 tests/` 全绿；`uv run ruff check .` 无错误；`uv run mypy .`（若项目跑 mypy）。修复实现引入的问题（不修复预存问题，若有记录）。
  Parallelization: Wave 3 | Blocked by: T6, T7, T8 | Blocks: T10, T11
  References: collectors/pyproject.toml:21-26 (pytest 配置), AGENTS.md Testing 章节（覆盖率 ≥95）
  Acceptance criteria (agent-executable): pytest 全绿 + cov ≥95% + ruff clean
  QA scenarios: happy: 全量测试通过；failure: 覆盖率缺口列出并补测；Evidence .omo/evidence/task-9-collectors-revision-detect.txt
  Commit: Y | test: full collectors suite green with 95% coverage (ref #135)
- [ ] 10. 文档同步（T10）
  What to do / Must NOT do: 更新五处：① kb/user/cli.md 增量机制段（UPDATE_DATE 锚点替代 REPORTDATE 窗口、min 语义、UPSERT 语义、fetch/import 同日约束——跨日/单独 import 会致锚点超前漏修订，须注明）；② kb/design/data-providers.md 决策记录 L398-399（INSERT IGNORE → UPSERT + 锚点变更，补决策记录行，含 C4 决策 B 限制）；③ fetch_fin_indicators.py docstring（L20 "Primary source ... table MAX(report_date)" 描述过期 → 改为 data_updates.last_updated 锚点；L23-28 移除 KNOWN LIMITATION/TODO；补"锚点停滞属预期"行为说明；**补"API 侧行删除/下架不传播到 Dolt（UPSERT 只能覆盖不能删除）"已知限制**）；④ **common.py import_replace_table docstring（L197-228 merge 段 "must use INSERT IGNORE" → 注明 UPSERT 支持）与 main.py _import_fin_indicators docstring（L134-139 "INSERT IGNORE'd" → UPSERT）**。不写不相关文档。
  Parallelization: Wave 3 | Blocked by: T9 | Blocks: T11
  References: kb/user/cli.md:197-215 (增量机制+merge 段), kb/design/data-providers.md:398-399 (决策记录), fetch_fin_indicators.py:20-28 (docstring 头+KNOWN LIMITATION), common.py:197-228 (import_replace_table docstring), main.py:133-139 (_import_fin_indicators docstring)
  Acceptance criteria (agent-executable): grep 五处关键词（UPDATE_DATE 锚点、UPSERT、无 KNOWN LIMITATION、无 "MAX(report_date)" 残留描述、docstring 无 INSERT IGNORE 残留描述）通过
  QA scenarios: happy: 文档反映新行为；failure: 无（文档一致性人工复核）；Evidence .omo/evidence/task-10-collectors-revision-detect.txt
  Commit: Y | docs: collector incremental revision-detect semantics (ref #135)
- [ ] 11. 工作流收尾（T11）
  What to do / Must NOT do: /review-work（≤2 轮修复）→ 呈报用户 → 用户批准 push → rebase master → /skwy-reflect 反思 commit → push → PR → #135 完成 comment（逐项核实证据）+ 关闭 #135 + #27（comment 注明 #27 被 #135 替代）。不自动 push；不提前关 issue。
  Parallelization: Wave 4 | Blocked by: T9, T10 | Blocks: —
  References: AGENTS.md Workflow（commit→review、Never auto-push、push 后收尾）、kb/dev/process.md
  Acceptance criteria (agent-executable): PR 合并后 #135/#27 关闭，reflections.md 有记录
  QA scenarios: happy: 流程完整；failure: push 前用户未批准则停止；Evidence .omo/evidence/task-11-collectors-revision-detect.txt
  Commit: N（流程收尾，非单 commit）

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对本 plan 的 todos/scope/acceptance，实现证据落盘 .omo/evidence/
- [ ] F2. Code quality review: /review-work 5 agent（goal/quality/security/QA/context）全过
- [ ] F3. Real manual QA: `fetch fin_indicators --incremental` 真库运行后，抽查 **update_date >= 首轮运行锚点（2026-08-03，固定参照日）的行**（首轮运行实测 16 行：08-03 5 行、08-04 11 行；注意首轮后锚点会随 data_updates=当天/CURDATE 与 state.json=max 推进重算为 08-04，命中 11 行——校验以固定参照日 08-03 为准）与 API 现值一致；**不得用五粮液 2025Q1 作断言行——其 update_date=2026-04-30 < 锚点，增量运行不重抓它，断言平凡通过不验证功能**（双审发现；C4 决策 B 下该行为已知限制）
- [ ] F4. Scope fidelity: 确认无 `--refresh N`、无 schema 变更、无三表改动、GUI/Rust 未触及

## Commit strategy
- 每个 commit 独立成行 `ref #135`；pre-push hook 校验 issue open
- Commit 序列：c1-c5 测试 RED（Wave 1）→ c6-c8 实现（Wave 2）→ c9 测试验证 → c10 文档 → c11 反思（/reflect）
- 反思 commit 在用户确认 push 后、push 前提交（ref #119 教训）

## Success criteria
- [ ] 增量模式能检测同一报告期的数据修订（UPDATE_DATE 变化）并覆盖旧数据
- [ ] 只抓有更新的行（局部重抓），非全量刷新
- [ ] 替代 #27（不实现 --refresh N）；PR 合并后 #135 + #27 关闭
- [ ] 全部测试通过，覆盖率 ≥95%
- [ ] 文档同步（cli.md + data-providers.md 决策记录 + docstring）
