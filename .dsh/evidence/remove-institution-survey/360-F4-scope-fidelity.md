# #360 F4 — Scope fidelity（范围保真 + grep 零残留）

审计对象：worktree `remove-institution-survey`，实现 commit `9777dd6`（基点 `ad850c4`）。

## 1. 数值面：唯一数值变更是 dragon 机构买入 10 → 15

`git diff crates/compass-strategy/src/sepa/scoring.rs` 关键行：

```
+const DRAGON_INSTITUTION_BONUS: f64 = 15.0;
-       10.0
+        DRAGON_INSTITUTION_BONUS
-    let survey = if inputs.has_survey { 5.0 } else { 0.0 };
-    let big_capital = (main_flow + dragon + survey + inputs.block_adj).clamp(0.0, 30.0);
+    let big_capital = (main_flow + dragon + inputs.block_adj).clamp(0.0, 30.0);
-                note_args: Some(vec![main_flow, dragon, survey, inputs.block_adj]),
+                note_args: Some(vec![main_flow, dragon, inputs.block_adj]),
```

- cap 仍为 `30.0`（未改）；无其他评分公式变动（其余常量/权重未出现在 diff 中）。
- `has_survey` 信号链路（`CapitalInputs` → `MarketContext.surveyed` → `SepaWindow.surveys`）全删，无残留赋值。
- i18n 注记模板参数由 4 → 3（`%{main}/%{dragon}/%{block}`，zh: `主力/龙虎/大宗`），GUI `factor_note_text` 同步 3 元素。

## 2. 数据面：无备份、无越界表改动

Dolt 仓库 `/data/compass-data/compass_data`（`main`，已 push，working tree clean）：

| commit | 内容 |
|---|---|
| `696rp2t…` | `feat: refresh SEPA derived tables under dragon institution bonus 15 (ref #360)`（technical_factor / industry_factor / capital_factor / final_score / market_temperature / backtest_result / data_updates） |
| `c6krg9f…` | `feat: drop institution_survey table and its data_updates row (ref #360)` |
| `…`（第三次） | `chore: refresh market_temperature after re-run (parallel-sum FP noise, ref #360)` |
| `…`（第四次） | `fix: restore full-window backtest_result snapshot (410 rows 2025-01-02..2026-09-09) under new SEPA formula (ref #360)` — 见下方「非目标表副作用」 |

**非目标表副作用（诚实披露，ref #360 安全审查 SEC-P1-1）**：验收冒烟 `sepa backtest --start 2026-07-01`
触发了 `crates/compass-data/src/backtest.rs:115` 的**无条件全表 `DELETE FROM backtest_result`**，
把既有 384 行快照（2025-01-02..2026-08-03）替换为 51 行短窗口——超出本 issue 声明范围。
处置：以默认全窗口重跑 `sepa backtest`（411 日 / 82 次换仓）恢复为 **410 行、2025-01-02..2026-09-09**
的完整历史曲线（新公式口径），单独 Dolt commit + push；代码层加固（scoped delete / 替换前快照）
已建 issue #361 跟踪。其余派生表在本批 commit 中均为「仅新增当日行、0 行删除」
（technical_factor / capital_factor / final_score / industry_factor）。

清理验证：

- `dolt sql -q "SHOW TABLES"` → 无 `institution_survey`
- `SELECT COUNT(*) FROM data_updates WHERE table_name='institution_survey'` → `0`
- `rm -f /data/compass-data/parquet_data/institution_survey.parquet` → `ls | grep -i institution` 无输出
- `rm -f /data/compass-data/csv/institution_survey.progress.json /data/compass-data/csv/RPT_ORG_SURVEYNEW.csv`
  （仓库外采集中间产物，安全审查 SEC-P2-1：`progress` 子命令的 glob 枚举会把它显示为有效 target）
  → `ls /data/compass-data/csv/ | grep -ci "institution\|SURVEYNEW"` = `0`
- `dolt status` → up to date with origin/main、working tree clean
- **未创建任何备份**（无 `pre_merge_backup` 之外的额外副本、无归档表、无 CSV 留档）——符合用户锁定决策"存量数据直接删除不留备份"

## 3. grep 零残留（worktree 内全仓 + 仓库外数据目录）

```
grep -R -n "institution_survey\|InstitutionSurvey\|institution-survey\|机构调研" \
  --include=*.rs --include=*.toml --include=*.sh --include=*.yml --include=*.json --include=*.csv \
  (worktree root)
ls /data/compass-data/{parquet_data,csv} | grep -i "institution\|SURVEYNEW"   # 仓库外数据面
```

（安全审查 SEC-P2-1 要求：范围必须含 `*.json`/`*.csv` 且覆盖仓库外数据目录——已补。）

生产代码（`crates/**` 非测试、`scripts/update-database.sh`、`*.yml` 语言包）：**零命中**。

允许的例外（逐条说明）：

| 位置 | 性质 | 保留理由 |
|---|---|---|
| `scripts/tests/test-update-database.sh:431-435` | #360 RED 断言块 `COLLECTOR_TABLES: institution_survey must be removed` | 契约本身必须提及该名；断言其**不存在**，实现后转为 GREEN |
| `crates/compass-strategy/tests/institution_survey_removal_adversarial.rs` | 对抗性契约测试（7 用例） | 验收契约；断言 dragon=15 / survey 不参与评分 / cap=30 |
| `crates/compass-strategy/tests/requirement_institution_survey_removal.rs` | 需求验收契约测试（4 用例） | 同上 |
| `crates/compass-data/tests/requirement_institution_survey_removal.rs` | 需求验收契约测试（2 用例） | `institution_survey` 必须 parse 失败；其余 10 表仍 Ok |
| `.dsh/kb/**` 历史行加注（toolchain.md 性能卡/#298 卡/#343 卡、data-providers 决策记录 #139/#202/#235 行） | 历史事实记录 | 按项目惯例保留历史事实，行内注明"已随 #360 移除"；当前状态表述（清单/计数/决策行）已同步为不含该表 |
| `.dsh/kb/dev/reflections*.md` 历史反思 | 只读历史 | 不修改 |

`crates/compass-collectors/src/dragon.rs` 的 `institution_flag TINYINT` **保留**——它是 +15 加分的唯一数据来源，非 institution_survey 表残留。

## 4. 结论

F4 通过：唯一数值变更 dragon 10→15（cap 30 不变）、无备份、数据面删除该表 + 其 `data_updates` 行 +
仓库外 parquet/CSV 残留，并按新公式刷新 SEPA 派生表；生产代码 grep 零残留，例外项全部为契约/历史记录并逐条说明。
非目标表副作用（`backtest_result` 快照被冒烟替换）已修复并披露（见 §2）。
