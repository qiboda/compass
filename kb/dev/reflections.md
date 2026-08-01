# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

旧的条目若已驱动具体流程改进（gate 硬阻、pre-push hook、自举测试等），则退役 — 教训已融入流程。


## 2026-07-28 — ref #62 rewrite: ParquetReader 改为单文件 stock_daily.parquet

**What was done**: 将 ParquetReader 从 per-symbol 文件布局（`stock_daily/SZ000001.parquet`）
改为单一 `stock_daily.parquet`（带 `symbol` 列），使用 `WHERE symbol = ?` 参数绑定过滤。
`list_symbols()` 改为先读 `stock_daily.symbols.txt` 再回退 SQL DISTINCT。删除了
`parquet_path()` 和 `file_exists()` 方法，放宽了 `validate_symbol()` 的路径穿越检查。

**What went wrong**: 首次编译时 Cargo 使用了旧的构建缓存导致 duckdb.rs 测试报告类型不匹配
（文件实际已更新），刷新后通过。

**Lessons learned**:
1. 单文件 + `WHERE symbol = ?` 参数绑定比 per-symbol 文件更安全（消除路径穿越面）
2. `symbols.txt` 伴生文件提供 O(1) 符号枚举，避免每次都查询大 parquet 文件
3. 参数绑定（`params![symbol]`）优于字符串插值，在整个代码库中应保持一致

## 2026-07-28 — ref #62 Wave 3: 更新测试与文档以匹配单文件 parquet 格式

**What was done**: 将 export.rs 测试、parquet_bench.rs、integration_test.rs 从 per-symbol
文件布局迁移到单一 `stock_daily.parquet`（带 `symbol` 列）。同步更新 AGENTS.md 和
`kb/design/symbols.md` 中的文件树描述和文件名引用。

**What went wrong**: （无）— 测试修改简单直接，无意外。

**Lessons learned**:
1. 测试中的旧格式设置（单文件无 symbol 列）虽然"通过"但未真正测试数据路径 — 只是空操作。
   更新后测试实际执行了 export 流程，验证效果更好。
2. symbols.md 中有多处"Parquet 文件名"引用过时，需一并更新保持一致性。

## 2026-07-26 — ref #31 fix: DuckDbProvider 直读 parquet_data，消除 cache miss

**What was done**: 将 DuckDbProvider 从文件型 DuckDB (`compass.db`) 改为内存型 + Parquet 回退。
`fetch_bars` 先查内存表（`save_bars` 的 EastMoney 数据），miss 时通过 `read_parquet()` 直接读取
`parquet_data/stock_daily.parquet` (single file with symbol column). GUI 启动即可命中本地数据，不再每次走 EastMoney HTTP。

**What went well**: RED→GREEN 严格 TDD，3 个新测试精准覆盖首次查询、日期过滤、save_bars 优先级。

**What went wrong**: 初期方案考虑了 glob VIEW 方案，但用户明确指向直读 parquet 文件，避免了过度设计。

**Lessons learned**:
1. 直读 parquet 文件比 glob VIEW 更简洁，DuckDB 的 `read_parquet()` 对单文件查询已足够高效
2. 列名映射 (`tradedate` → `trade_date`) 和符号映射 (`000001` → `SZ000001` in symbol column) 是跨存储格式的核心细节
3. CLI 工具仍需文件型 DuckDB，保留 `new_file()` 构造函数是必要的分离

## 2026-07-26 — ref #24 refactor: integrate egui-mobius Level 3 citizen pattern

**What was done**: Replaced manual mpsc + Arc<Mutex<CompassState>> architecture with
egui-mobius Level 3 (AsyncDispatcher + typed signal/slot + Dynamic<T>). Converted
single-panel CentralPanel layout to 3-citizen DockArea (Control, Chart, Logger).
Removed dead code: bars_version, search_results, Cmd::SearchSymbols, retry_count.
Upgraded egui from 0.33 to 0.35, switched egui-charts to qiboda fork.

**What went well**: Grill-me locked all architectural decisions before implementation.
Plan-first approach produced a structured 14-task plan with wave-based parallelism.
Zero compilation errors on first build after each wave.

**What went wrong**: egui_citizen not published on crates.io — git dependency required.
egui_lens 0.5.0 panic bug from `ReactiveEventLoggerState::default()` (max_logs=0) —
fixed by using `new()` instead.

**Lessons learned**:
1. Grill-me as step 0 is effective — 9 decisions locked before plan creation.
2. Wave-based plan decomposition enables true parallelism.
3. Never silently change a planned approach — added Scope Discipline rule to AGENTS.md.

## 2026-07-25 — ref #16 fix: pre-push hook new-branch range scans only branch commits

**What was done**: Changed issue-reference validation in `.beads/hooks/pre-push` to use
`git merge-base origin/master` for new branches instead of scanning all reachable history.

**What went wrong**: `$local_oid` scanned entire commit history, flagging closed issues from old master commits.

**Lessons learned**: `git log $sha` without range prefix scans all ancestors — use `merge-base..$sha`.


## 2026-07-25 — chore: add worktree management skill

**What was done**: Created `worktree` skill, standardized conventions, cleaned orphan worktree.

**Lessons learned**: Process docs reference skills, don't duplicate them. Worktrees need cleanup discipline.


## 2026-07-26 — feat: 创建 compass_data Dolt 仓库 #25

**What was done**: 在 `investment_data` 同级创建 `compass_data` Dolt 仓库，含 `stock_basic` 和
`fin_indicators` 两张表，验证跨库 JOIN（ts_code 直连 ts_a_stock_list，symbol 直连
final_a_stock_eod_price）。

**What went wrong**: Dolt SQL 里 `||` 是逻辑 OR 而非字符串拼接 — 转用 `CONCAT()` 解决。
investment_data 的 exchange 值并非文档中的 `SHSE/BSE` 而是 `SSE/BSE`，跨库映射需以实际值为准。

**Lessons learned**: Dolt 方言和 MySQL 一致，`||` 不等于拼接；跨库查询需从父目录运行 `dolt sql` 无 `--data-dir`。


## 2026-07-26 — 流程违规: import-compass/backup 等多项 feature 工作跳过 PRE-IMPLEMENTATION GATE

**What went wrong**: 当天完成了 import-compass 命令、Backup 上传、fin_indicators 增量、
CI 修复等多项 feature 工作，全部跳过了 PRE-IMPLEMENTATION GATE（无 GitHub issue、
无 plan、无 test-first）。

**Lessons learned**:
1. GATE 对 "所有代码变更" 的约束力不足 — 已补充 SELF-CHECK 4 问硬性规则
2. "feature 工作" 的边界太模糊，容易自欺"这不是 feature" — 改为白名单例外（仅 docs/lint/typofix）
3. 流程违规本身即是 bug — 记录到 reflections 并修复流程


## 2026-07-26 — ref #43 feat: GUI layout rework + SH/SZ/BJ exchange selection

**What was done**: Replaced the 3-citizen DockArea layout with a toolbar + 2-citizen
DockArea (Chart, Logger). Added Exchange enum (SH/SZ/BJ/All), searchable symbol dropdown
loaded from `stock_basic.parquet`, exchange-filtered symbol list, and exchange prefix
auto-prepending. Removed ControlCitizen and the outbox pattern from the main loop.

**What went well**: All gate steps completed (issue #43, ulw-plan, RED phase tests, docs).
Momus review passed with no blocking issues. TDD approach caught sort order bug in
filter_stocks() before manual testing.

**Lessons learned**:
1. egui 0.35 doesn't have `TopBottomPanel` — used inline horizontal toolbar instead
2. Module visibility: `mod widgets` declared in main.rs makes `crate::widgets` accessible from test modules
3. The `#![warn(missing_docs)]` lint is aggressive — every public item needs a doc comment, even enum variants

## 2026-07-26 — ref #46 feat: DuckDbProvider timeframe aggregation (daily→weekly/monthly)

**What was done**: Implemented daily→weekly and daily→monthly OHLCV resample in
`DuckDbProvider::fetch_bars()` using DuckDB SQL `date_trunc` + `FIRST`/`LAST`/`MAX`/`MIN`/`SUM`
aggregates. The `_timeframe` underscore prefix was removed and timeframe is now used for
aggregation. Added `Dynamic<String> timeframe` to `SharedState` for dynamic chart label updates.
Fixed hardcoded `set_timeframe_label("1d")` in ChartCitizen.

**What went well**: TDD approach caught two pre-existing test failures immediately
(`save_and_fetch_preserves_symbol_and_timeframe` expected old ignore-timeframe behavior).
DuckDB's `FIRST(open)`/`LAST(close)` with ordered subquery worked correctly for
chronological OHLC within each time bucket.

**Lessons learned**:
1. `FIRST()`/`LAST()` in DuckDB respect subquery ORDER BY — essential for correct
   OHLC aggregation where open=earliest, close=latest
2. Pre-existing tests that saved/fetched bars with `"1w"`/`"1M"` but expected daily
   results revealed the artificial test assumptions made before aggregation was implemented
3. Merge conflicts in worktree branches exposed `.gitignore`'d symlink data directories
   tracked on master — needed manual resolution

## 2026-07-31 — ref #77 docs: 项目书重组（修正/去重/提取/格式 4 pass）

**What was done**: 项目书全面重组，6 个 commit：Pass 1 以代码为准修正全部过时描述
（GUI 纯本地、删除不存在的 download/merge 子命令与 import --overwrite、CachedProvider/
EastMoneyProvider/to_secid 整章删除）；Pass 2 去重收敛（AGENTS.md 442→310 行瘦身为索引
+硬规则，DDL/doc-sync/CLI/config 各归单一事实源）；Pass 3 程序性内容提取到已有 skill
（worktree/issue/TDD/gate 全部指向 skill，修复 fix.md/impl.md 对已删除 doc-sync 表的断链）；
Pass 4a 全部 kb/ 19 文件中文化；Pass 4b roadmap→backlog 需求池、friction/reflections 模板
统一、docs skill 清单 17→19 同步。

**What went wrong**: ① 修正 pass 发现 `import --overwrite`、`to_secid()`、`CachedProvider`
等文档大量记载已删除的功能，说明项目书长期未随重构同步（#31/#32/#46 之后均未清理）。
② Pass 4a 翻译首次派发漏了 kb/dev/ 两个文件，且首个 architecture.md 翻译子代理对纯翻译
任务也套 grill-me 流程导致停滞，重派后才完成。③ Pass 4b 的 roadmap.md 删除混入了 Pass 4a
的翻译 commit（git rm 暂存区未分离）。

**Lessons learned**:
1. 修正先行（Pass 1 前置）是关键决策 — 先以代码验证"哪个副本是对的"，去重时才不会把错误
   内容当保留副本扩散。本次发现 8 处事实性错误全部来自代码验证。
2. 文档引用是重组中的主要断链风险 — 去重/改名后必须全仓 grep 交叉引用（fix.md/impl.md
   引用的 doc-sync table 在 AGENTS.md 移除后失效，product skill 8 处 roadmap 引用需批量更新）。
3. 纯机械任务（翻译）的子代理 prompt 必须显式声明"不要 grill-me、不要提问、直接执行"，
   且翻译范围清单要在派发前一次列全（本次漏了 kb/dev 导致补派）。
4. docs 类工作可跳过 GATE，但 commit 仍须 ref #N；本次按用户"分别多次 review"要求拆 6 个
   commit，每个 pass 独立 review 确认，方向偏差在早期被纠正。

### Trends (last 10)
- 文档与代码脱节反复出现（#62 Wave 3、#31、#77）：docs 修改应以代码验证为准，
  重构后必须同步清理相关文档，而非等到用户抱怨"重复太多/描述不对"再集中处理
- 子代理对机械任务过度流程化（#77 翻译停滞）：skill 类指令应说明适用边界，
  纯翻译/机械任务直接执行，避免 grill-me 误用
- 多文件批量改动时引用一致性是最高风险点（#77 roadmap 引用 ×8、doc-sync 断链 ×2）：
  批量修改后用 grep 全仓校验引用完整性

## 2026-08-01 — ref #78 stock_basic 数据源切换三大交易所官网

**What was done**: stock_basic 从东财 push2（EM_FS t:81 段污染 6,841 只新三板/老三板）切换到三大交易所官网（上交所 JSON / 深交所 xlsx×2 / 北交所 form POST），Dolt 表加 delist_date/board/full_name/total_share、删 5 个东财残留列，新建 fetch_stock_basic_official.py 采集器，全链路（采集→Dolt→parquet→GUI）验证 5,888 行含 354 退市。

**What went wrong**: ① 采集器 T3 交付后真实采集仅 2,787 条（深交所全部缺失）——单元测试用裸 `<row>` fixture 而真实 xlsx 带 `r="1" s="1"` 属性，正则 `_ROW_RE` 不匹配，单测全绿掩盖了真实数据 bug；② 同类问题：SZSE 总股本含千分位逗号 `19,405,918,198`，`float()` 直接抛 ValueError，同样只在真实数据出现；③ T7 按计划用 `NULLIF(col,'')` 转换空值，但 Dolt 类型化 CSV 导入已自动转 NULL，NULLIF 反报 "Incorrect datetime value" —— 计划假设与实际 Dolt 行为不符；④ review 发现 duckdb.rs/import_dolt.rs 遗留旧 schema（issue #80）。

**Lessons learned**:
1. 网络数据采集器的测试必须包含"真实响应样本 fixture"，不能只用构造的最小片段——真实 xlsx 的属性顺序/千分位/编码差异是单测盲区，T3 后必须真实跑一次比对行数（本次真实跑立即暴露 2,787 vs 5,888 差异）。
2. 涉及 Dolt/数据库导入的转换逻辑（NULLIF 空值处理），先验证工具实际行为再写代码——Dolt table import 的类型推断会自动处理空串，plan 中的 SQL 假设需实测确认（T7 的 deviation 事后被证明正确）。
3. 跨语言管线（Python 采集 → Dolt schema → Rust parquet）改动时，遗留旧路径（duckdb.rs/import_dolt.rs）即使不在本 PR 范围也要 grep 标注并记 issue，避免文档与代码分裂。

### Trends (last 10)
- 测试与真实数据脱节反复出现（#78 T3 xlsx 属性、#62 数据验证）：网络/数据源解析的测试 fixture 应取自真实响应，并在实现后立即做真实端到端验证（行数/字段比对），不能依赖构造数据单测全绿
- 计划假设与工具实际行为不符（#78 NULLIF/Dolt 导入、#77 文档与代码脱节）：涉及外部工具（Dolt/API）的转换逻辑，实施前先实测工具行为或先写最小验证脚本，避免按文档假设写代码
- 多模块跨语言改动后遗留路径检查（#78 duckdb.rs/import_dolt.rs、#77 引用断链）：主路径完成 ≠ 全仓一致，交付前 grep 旧 schema/旧引用并记 issue

## 2026-08-01 — ref #79 feat: 强制 80% 测试覆盖率门禁（Rust workspace/每 crate + Python）

**What was done**: 实现 CI 覆盖率门禁——coverage job 移除 continue-on-error，4 条 `cargo llvm-cov --fail-under-lines 80`（workspace + 3 crate）+ Python `--cov-fail-under=80`。补齐 +5900 行测试（GUI 从 28.87%→93.21% 用 egui_kittest 无头集成测试、Python 从 18%→91.69% 用 stub AsyncSession），3 处可测性重构（baostock 脚本路径注入、main.py/main.rs 提取 dispatch），文档同步。5-agent review 发现 2 MAJOR + 1 BLOCKING + 1 IMPORTANT 全部修复后通过。

**What went wrong**:
1. **T5/T6/T7 三个执行 agent 卡在 grill-me 自我访谈**——delegation prompt 未声明"禁止 grill-me"，agent 加载 AGENTS.md 后在 background（无用户可问）自我问答浪费一整轮，强制续跑后才实现
2. **review 发现 3 个实质问题**：ImportCompass double-logging（map_err 闭包内 error! + main 再打）、测试名误导（input: Some 未测回退路径）、pre-existing 日期依赖测试（Utc::now() 周六跨 ISO 周边界）
3. **pre-push 阻断 2 次**：rustdoc `private-intra-doc-link`（pub item doc 链接到 pub(crate) item）、cargo fmt 未过——这些本地 cargo test/clippy 未暴露
4. **agent 测试代码 clippy --all-targets 14 个 lint 错误**（await_holding_lock/expect_fun_call 等），覆盖率数字达标但质量检查不过

**Lessons learned**:
1. **delegation prompt 开头必须显式声明"任务已完全指定，禁止 grill-me/访谈，直接实现"**——本仓库 AGENTS.md 强制 grill-me，background agent 无用户可访谈会卡死；T5/T6/T7 首次全部空转
2. **本地提交前验证应包括 pre-push 全套（cargo doc -D warnings + fmt --check），不只 cargo test/clippy**——rustdoc 链接错误和格式问题在本地常规检查漏掉，推送到 pre-push hook 才暴露，浪费 push 轮次
3. **时间依赖测试是隐藏 flake**：测试 fixture 用 `Utc::now()` 做基准日期会导致周末/月末跨周期边界失败（本次周六复现）——固定基准日期（确定周几）避免
4. **agent 产出不能只看覆盖率数字**：需抽查断言真实性 + `clippy --all-targets` 全量质量关，否则 5900 行测试带 14 个 lint 错误进入分支

### Trends (last 10)
- **测试相关教训反复出现**（#62 Wave 3、#46、#79）：测试正确性/确定性/质量是高频失败点——#79 再次印证"测试数量≠测试质量"，需把 clippy --all-targets 和断言抽查纳入 agent 测试任务的验收标准
- **gate/流程违规历史条目**（07-26 流程违规、本次 agent 卡 grill-me）：AI agent 加载 AGENTS.md 后可能做出违背任务意图的行为——delegation prompt 的显式禁令是必要防呆（本次已验证有效）
- **hook/工具链问题**（#16 pre-push hook、#79 pre-push 阻断）：pre-push hook 是质量最后防线，本地验证命令应与其对齐（fmt/doc 全套），避免 push 时才暴露

## 2026-08-01 — ref #75/#88/#89/#92/#83 合并批次：多 PR rebase 落地 + CI 全绿

**What was done**: 一次合并 3 个 PR（#88 ci-fix 移除自动 /fix、#92 rust-cache 仅 master save、#83 覆盖率 80% 门禁），全部 rebase 到新 master 后合并，master CI 全绿。修复 #75 flaky 测试（周六日期跨 ISO 周边界）、setup-uv@v9 不存在的 tag（→@v9.0.0）、pre-push hook rebase 误拒（#95）、ci-fix 既存问题（#87）。落地"目标分支修复工作流"（从 feature 分支切修复分支 cherry-pick 回，各 PR 互不阻塞）。关闭 4 个陈旧 CI Failure issues + #54 架构 issue。

**User corrections**（原 friction 条目合并，friction.md 今日条目已删除）:
1. 修复方案先问"最小可行是什么"再考虑自动化——GITHUB_TOKEN 触发链断裂的根因是平台限制，与其建复杂链路（内联 fix agent），不如砍掉自动修复回归人工（更简单可靠）
2. CI 修复的传播路径要提前设计——master 级 bug 单独直推 master，feature 分支问题从该分支切修复分支，避免所有分支排队等 master

**What went wrong**:
1. **#75 flaky 测试再次中招**：master 上 `save_and_fetch_preserves_symbol_and_timeframe` 用 Utc::now() 生成日期，周六跨 ISO 周边界 → PR #88/#92 的 CI 全挂——#79 反思已记录此教训，但修复（固定基准日期）未及时进 master，导致后续所有分支 CI 连锁失败
2. **rebase 后 force push 被 pre-push hook 误拒**：hook 的 range 含 master 已合并 commits（含已关闭 issue 的 ref）——暴露 hook 的 merge-master 场景缺陷（#95）
3. **rebase 3 处深度冲突**：main.py（官方源迁移 × dispatch 提取）、duckdb.rs（#75 修复双版本）、reflections.md、pre-push hook（双版本）——跨 PR 架构性冲突，需逐文件判断保留哪侧
4. **PR #83 rebase 后 clippy --all-targets 暴露 3 类问题**：expect_fun_call、unused Path、await_holding_lock + load_config 测试 HOME env 竞态（Mutex 串行化修复）
5. **关闭 issue 触发 8 个 opencode workflow skipped run 刷屏**——平台行为（issue_comment 事件级触发，无法按 body 过滤）

**Lessons learned**:
1. **flaky 测试修复必须立即进 master**（不等 PR 合并批次）——#75 修复在分支上滞留导致所有后续分支 CI 连环挂；master 级 bug 应单独直推 master（"目标分支工作流"的"何时不用"场景）
2. **rebase 大 PR 前先跑 `cargo clippy --all-targets` + 全量 pytest**——rebase 后 master 变更（官方源迁移）会让旧测试/旧 lint 失效，先本地暴露再 push，避免 CI 轮次浪费
3. **跨 PR 冲突解决原则**：保留"已在 master 验证过"的版本（如 #75 修复 2026-08-05 vs 2026-01-01 基准日期），功能性冲突（dispatch × 官方源）需理解双方意图后合并而非二选一
4. **hook 类代码的 commit message 避免在正文写 `ref #N` 字面量**（commit-msg hook 会误提取已关闭 issue）——用"issue 75"等不带 ref 前缀的表述

### Trends (last 10)
- **hook 链路的次生 bug**（#16 pre-push range、#79 pre-push 阻断、#95 rebase 误拒、commit-msg 误提取正文 ref）：hook 自身成为新的故障源——hook 改动需带测试/模拟验证，且 commit message 规范应明确"正文勿用 ref #N 字面量"
- **跨 PR 合并成本被低估**（#78 × #79、#75 × #83）：并发 PR 共享文件时，rebase 冲突解决 + 测试适配是主要工作量——提前识别共享文件（main.py/duckdb.rs/reflections.md）可预判冲突
- **时间/环境依赖测试反复出现**（#75、#79 review、#62）：测试必须用固定基准（日期/路径/端口），agent 生成的测试要专项检查"是否依赖 now()/环境变量"

---

## 历史摩擦记录（并入自 friction.md，2026-08-01）

> friction 机制已合并入本文件的 User corrections 章节。以下为历史摩擦条目，
> 记录用户纠正 AI 行为的时刻，保留防重犯价值。

## 2026-07-30 — #69 grill-me

**User corrections**: 摩擦记录不应局限于 grill-me，应该是**任何「我做了/说了 X，你纠正为 Y」的场合**——包括执行方向偏离、意图误解、约束遗漏等所有纠正型交互。

**教训**: 不要被用户给出的例子锚定（anchoring bias）。用户举的例子是示意，不是边界定义。正确做法是追问范围边界，而非默认例子就是全部。

## 2026-07-30 — 三张财务报表管线 review 修复

**User corrections**: 「发现问题首先应该写测试，而不是直接写代码」。TDD 的 RED→GREEN 流程：先写能复现 bug 的测试，确认它失败，再修代码让测试通过。

**教训**: 任何 bug 修复都必须先有失败测试。即使问题看起来"简单清楚"，跳过测试直接改代码就是违反 test-first 纪律——这会丢失回归保护，也无法证明修复真正有效。修复与测试应成对出现，测试先行。

## 2026-07-31 — worktree opencode 启动失败 (#76)

**User corrections**: 「开启新的 opencode，要先解绑当前的 opencode 的 session」。opencode 将 worktree 目录映射到与 master 相同的 project_id（`git_worktree` 关联），master 实例仍绑定该 project 的 session 时，worktree 新实例无法启动。该经验已写入 worktree skill 的 Post-Creation MANDATORY 步骤。

**教训**: 涉及 opencode/git 工具的跨目录操作，先确认工具对 worktree 的特殊处理（session/project 绑定模型），再执行启动动作。教训应沉淀到 skill 文档本身，确保后续所有 agent 在流程上不会重犯。
