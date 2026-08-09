# gui-i18n - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** The whole GUI (window, tabs, toolbars, panels, SEPA/screener tables, chart tooltips) becomes translatable between Chinese and English — switchable live from a toolbar dropdown and remembered across restarts. All text moves into one central dictionary; the chart library fork and the scoring engine hand over semantic keys so the same switch works everywhere.

**Why this approach:** One key-based dictionary (rust-i18n) with a single global language setting is the cheapest way to make every text site — including text inside the chart library fork and the SEPA scoring data — switch together instantly. Chinese stays the default so nothing about the current look changes unless you opt into English.

**What it will NOT do:** It will not translate market data (stock/industry/theme names stay Chinese — they're data, not labels), won't translate low-level technical error details, won't touch the CLI's Chinese output, and won't add any third language (the architecture allows it later, but only zh/en ship now).

**Effort:** XL
**Risk:** Medium - 4 crates + external fork + CLI data contract + ~150 test assertions migrate; key-typo silent failures are guarded by the completeness tests, and the SEPA CSV export must keep its exact values.
**Decisions to sanity-check:** (1) locale files per-language (v1) vs single-file (v2) — default v1 pending a Phase-0 spike; (2) language lives top-level in config.toml like `theme`; (3) SEPA factor notes get 2 extra keys the original design missed (big-cap flow, thermometer); (4) fork rev pinned to an exact commit, not the branch head.

Your next move: start work via `$start-work gui-i18n` (optionally `--worktree`/`--make-pr`). Full execution detail follows below.

---

> TL;DR (machine): XL effort, Medium risk — full GUI zh/en i18n via rust-i18n across compass+compass-ui+strategy+fork, 6 phases / 16 todos + F1-F4.

## Scope
### Must have
- GUI 全部用户可见文本走 rust-i18n `t!()` 键查找（compass / compass-ui / fork 可达文本 / strategy 数据标签），zh/en 双语言，键架构允许加语言
- 新 crate `compass-i18n`（单一 locales/ 全量键树、`init!()` 宏、re-export t/set_locale、`fallback = "zh"`）；compass-ui 与 compass 依赖它，`i18n!()` 指向同一 locales 数据（跨 crate 键解析一致）
- config.toml 顶层 `language = "zh"` 键（AppConfig 字段，缺省 zh，非法/空值回退 zh + warn）；`main()` 启动 `set_locale`（单一调用点，L52 与 L66 之间）
- 语言切换 UI：工具栏语言下拉（中文/English 母语名）+ `set_locale` + 重绘 + `ViewportCommand::Title` + Info toast + config 写回（保留 app/watchlist/screener 节）
- egui-charts fork 自带 rust-i18n + locales/ + t!()（tooltip/crosshair/time_formatter/labels/realtime）+ 暴露 `set_locale` wrapper；compass bump 到**精确 i18n commit**（非 branch head）
- compass-strategy 返回语义 key（SepaIndicator/SepaFactor/MarketThermometer 模型加 key 字段）；**compass-data SEPA CSV/backtest 输出同步更新**（保持数据契约不变）
- kittest 断言从字面中文迁移到 t!() 键解析；en 专项布局测试 LANG_LOCK 串行；键完整性测试（zh/en 键集对称 + 代码使用键全部存在）
- 文档同步：kb/design/ui.md、kb/user/config.md（language 键）、kb/user/gui.md、kb/design/ui-widgets.md（ColumnSpec/modal 变更）、AGENTS.md + scripts/check-coverage.sh（compass-i18n 覆盖率门槛）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 数据翻译：股票名（贵州茅台）/行业/题材名（白酒、茅指数）/代码/日期值/数值——数据源为中文 A 股数据，翻译破坏可检索性
- `%{e}` 底层 DataError Display 翻译（技术细节保持英文）
- **CLI/compass-data 输出翻译**（format_top_table 表头、"市场温度" println 保持中文/数据中性——不在 GUI 范围）
- fork 自身未激活 UI（src/ui/ feature 门控：top_toolbar/drawing_toolbar/connection_status/symbol_header）键化——`chart.legend.*` 键从键树删除或标记 reserved
- fork 的 LocaleTimeFormatter/RelativeTimeFormatter（死代码）键化
- 字体变更；除 zh/en 外第三语言实现；不 bump fork 到未 pin 的 commit
- 不依赖 rust-i18n missing-key fallback 作为正确性信号（键必须真实存在，用键完整性测试把关）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD（先写失败测试再实现）+ kittest 集成测试 + rstest 单元测试 + fork 侧 #[cfg(test)]
- Framework: egui_kittest（GUI）、rust-i18n t!() 键解析断言、cargo test / cargo llvm-cov（覆盖率：compass-i18n 新增 80% 门槛，其余不变：compass-core/data 95%，compass/strategy/types/ui 80%）
- Evidence: .omo/evidence/gui-i18n/（task-N 输出、测试日志、覆盖率报告）
- 每个 phase 结束跑 `cargo test` + `cargo clippy` + `cargo fmt`；fork 改动在 fork 仓库独立验证后 bump
- 键完整性测试：脚本 diff zh.yml/en.yml 键集对称；遍历代码中所有 t!() 键常量断言两语言均存在（跨 crate，含 compass-ui 渲染 compass 传入的键）

## Execution strategy
### Parallel execution waves
- Wave 1（Phase 0 基建，串行基石）：T1 compass-i18n crate + 全量 locales + 键完整性测试 + 覆盖率配置 + i18n!() 路径 spike；T2 config language 键；T3 set_locale 启动接线
- Wave 2（Phase 1 应用框架，可并行）：T4 测试断言助手 + 应用框架断言迁移；T5 main.rs/tabs/logger/chart/backend 键化（含 display-log 键）；T6 compass-ui 组件键化 + ColumnSpec 键化 + **sepa/screener COLUMNS 常量同步键化**（D2）
- Wave 3（Phase 2 strategy 键，D3 交换后先行）：T7 compass-types 模型加 key 字段；T8 compass-strategy 返回 key；T9 compass-data 更新（CSV 契约保持）
- Wave 4（Phase 3 消费，原 Phase 2 内容，swap 后）：T10 sepa.rs 渲染消费 t!()（表头已在 T6）；T11 screener.rs 渲染消费 t!()
- Wave 5（Phase 4 fork）：T12 fork i18n 接入；T13 fork 测试更新 + compass bump 精确 commit
- Wave 6（Phase 5 切换 UI + en 验收）：T14 语言下拉 + 写回 + 持久化测试；T15 en 布局 sweep 测试
- 最终验证 wave：F1-F4

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 compass-i18n crate + locales | — | T3, T4, T5, T6, T10, T11 | T2 |
| T2 config language 键 | — | T3, T14 | T1 |
| T3 set_locale 启动接线 | T1, T2 | T14 | — |
| T4 断言助手 + 应用框架迁移 | T1 | T5, T6, T10, T11 | — |
| T5 main.rs/tabs/logger/chart/backend 键化 | T1, T4 | T14 | T6 |
| T6 compass-ui 键化 + COLUMNS 同步键化 | T1, T4 | T10, T11 | T5 |
| T7 compass-types 模型加 key 字段 | — | T8, T9, T10 | T1 |
| T8 compass-strategy 返回 key | T7 | T9, T10 | — |
| T9 compass-data 更新 | T7, T8 | — | — |
| T10 sepa.rs 消费 t!() | T6, T7, T8 | — | T11 |
| T11 screener.rs 消费 t!() | T6 | — | T10 |
| T12 fork i18n 接入 | — | T13 | T1-T11（独立仓库） |
| T13 fork 测试 + bump 精确 commit | T12 | — | T14 |
| T14 语言下拉 + 写回 | T3, T5 | — | T13 |
| T15 en 布局 sweep | T5, T6, T10, T11, T14 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. 新建 compass-i18n crate（share-in-workspace 模式，全量 locales，键完整性测试）
  What to do / Must NOT do: 创建 /data/codes/compass/.worktrees/gui-i18n/crates/compass-i18n/，Cargo.toml（依赖 rust-i18n 4.2.1，加入 workspace members + workspace.dependencies `rust-i18n = "4.2.1"`）；**i18n!() 路径 spike 先行**（验证 rust-i18n proc-macro 是否支持父级相对路径 `i18n!("../../compass-i18n/locales")` 跨 crate 共享——若不支持，改为 compass/compass-ui 各自 i18n!("locales") + 复制 yml + 键完整性测试防漂移，记录 spike 结论到 evidence）。**locales/zh.yml + locales/en.yml（_version 按 spike 确认的格式——per-locale 文件 v1 或单文件 v2，二选一，plan 默认 per-locale v1 与设计键树对应）**，内容 = **全量键树**：.omo/designs/gui-i18n.md §1 全部键 + **Metis C5 补充 2 个 note 键**（sepa.note.big_capital 主力+龙虎+调研+大宗 4 参数、sepa.note.thermometer 温度计）+ **Metis C7 补充 logger 6 个 display-log 键**（logger.log_fetch_failed/fetch_completed/screener_failed/screener_completed/sepa_failed/sepa_completed）+ **删除或标记 reserved `chart.legend.*` 键**（fork src/ui/ 不可达，Metis C3）；zh 值 = 现状中文文案，en 值 = 设计文档对照。src/lib.rs 声明 `i18n!("locales", fallback = "zh")`（**fallback 必须是 zh**，Metis M5：kittest 直接构造 CompassApp 不调 main()）+ `init!()` 宏 + `pub use rust_i18n::t; pub use rust_i18n::set_locale;` + 键常量（`pub const KEY_APP_TITLE: &str = "app.title"` 等，供编译器检查键合法性）。**键完整性测试**：`cargo test -p compass-i18n` 断言 zh.yml/en.yml 键集对称（读 yml 文件 diff）+ 所有 KEY_* 常量在两语言均存在（t!() 无 panic）。Must NOT：不依赖 missing-key fallback 通过测试（Metis A7——键缺失时 t!() 返回 key 本身是静默假阳性，测试必须显式比对两语言文件）；不在此 todo 建依赖方。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T3, T4, T5, T6, T10, T11
  References: .omo/designs/gui-i18n.md §1（键树契约，含 C5/C7 补充）；~/.cargo/git/checkouts/egui-charts.../examples 无（参考 longbridge/rust-i18n examples/share-in-workspace 模式）；Cargo.toml（workspace members + workspace.dependencies）；scripts/check-coverage.sh（覆盖率门槛表）
  Acceptance criteria (agent-executable): `cargo check -p compass-i18n` 通过；`cargo test -p compass-i18n` 全绿（键完整性：zh/en 键集对称 + KEY_* 常量双语言存在）；check-coverage.sh 已加 compass-i18n 80% 门槛（Metis M7）；spike 结论记录在 .omo/evidence/gui-i18n/task-1-spike.md
  QA scenarios: happy — 键完整性测试绿 + 删一个 yml 键 → RED；failure — 跨 crate 路径 spike 失败则记录并改复制方案。Evidence .omo/evidence/gui-i18n/task-1.log
  Commit: Y | feat(i18n): add compass-i18n crate with zh/en locale dictionaries (ref #222)

- [ ] 2. config.toml 顶层 language 键 + 缺省回退（AppConfig 字段）
  What to do / Must NOT do: 在 crates/compass-core/src/model.rs AppConfig（L262-275）加 `#[serde(default = "default_language")] pub language: String,`（顶层键，镜像 theme，Metis C1）；`fn default_language() -> String { "zh".into() }`；**注意 AppConfig 还 derive Default（L261）——Rust derive 的 Default 会给 language = "" 而非 "zh"**（Metis C1 陷阱）：main.rs 两个 parse-failure fallback（L258-262、L267-271）走 AppConfig::default() 得到 ""，必须在消费处（T3 set_locale 前）用统一 guard 把 "" 也回退 zh + warn。main.rs FullConfig（L231-239）无需改（flatten AppConfig 自动含 language）。migrate_legacy_config 不动。Must NOT：language 不放 [app] 节（AppSection 只放 default_symbol/timeframe）；不改 save_*_config（T14 新增 save_language_config）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T3, T14
  References: crates/compass-core/src/model.rs:261-275（AppConfig + derive Default + theme 字段模式）；crates/compass/src/main.rs:231-274（FullConfig + load_config fallback）；kb/user/config.md（language 键文档，T14 或本 todo 同步）
  Acceptance criteria: `cargo test` compass-core model 测试新增：反序列化 `language = "en"` → "en"；缺键 → "zh"（serde default）；derive Default → ""（记录该行为，消费处处理）。`cargo clippy` 干净
  QA scenarios: happy — toml::from_str 各 case；failure — language="fr" 非 serde 校验场景（T3 的 guard 处理，此处仅验证字段存在与 default）。Evidence .omo/evidence/gui-i18n/task-2.log
  Commit: Y | feat(config): add top-level language key with zh default (ref #222)

- [ ] 3. main() 启动 set_locale 接线（单一调用点 + 空值 guard）
  What to do / Must NOT do: 新增语言规范化 guard（compass-i18n 或 main.rs）：`fn normalize_language(lang: &str) -> &'static str { match lang { "zh" => "zh", "en" => "en", _ => "zh" } }`（覆盖 "" 与非法值，非法时 tracing::warn!，Metis C1/C4）；main.rs main()（L50-185）在 load_config()（L52）之后、run_native（L66）之前调用 `compass_i18n::set_locale(normalize_language(&config.app.language))`——**单一调用点**（Metis M6：fork/compass-ui 的 t!() 通过同一进程级全局自动生效）。窗口标题 run_native 第一参 "Compass — Stock Chart"（L67）保持品牌英文不键化。Must NOT：不在 CompassApp 存 language 状态（T14 语言下拉才需要）；不改窗口标题。
  Parallelization: Wave 1 | Blocked by: T1, T2 | Blocks: T14
  References: crates/compass/src/main.rs:50-68；crates/compass-core/src/model.rs:261-275；.omo/designs/gui-i18n.md §2
  Acceptance criteria: `cargo test` 新增 normalize_language 单测（"zh"/"en"/""/"fr"/"ZH" → 期望值）；现有测试全绿（zh 默认 locale 下字面量断言暂不动）；RUST_LOG=debug 验证 warn 日志在非法值时出现
  QA scenarios: happy — normalize("") == "zh"；failure — normalize("fr") == "zh" + warn。Evidence .omo/evidence/gui-i18n/task-3.log
  Commit: Y | feat(i18n): wire set_locale at startup with language normalization (ref #222)

- [ ] 4. kittest 断言助手 tr() + 应用框架断言迁移
  What to do / Must NOT do: 新增测试辅助函数（如 crates/compass/src/tests_util.rs）：`pub fn tr(key: &str) -> String { compass_i18n::t!(key).to_string() }`；将 main.rs 测试中**应用框架相关**断言从字面中文改为 `tr("key")`：前复权（L1375）、本地数据源（L1732/L1765）、移除自选/移除/保留（L2004/L2006/L2049）、数据未就绪/知道了（L2081/L2082/L2099/L2110）、日志已导出/导出失败（L2171/L2193）、"--- Logger Export ---"（L2132，若键化则 tr 否则保留）、"自选"（L1816 自选股为空）、"搜索自选"（L1731/L1745/L1798/L1808/L2328）、"切换侧边栏"若断言、主题已切换（若断言）。tabs.rs（L196-277）、logger.rs（L112）、chart.rs（L355/356）同步迁移。**先写键存在性测试（RED）**：T1 的键完整性测试已覆盖，此处断言迁移后若 key 缺失 tr() 返回 key 字符串使测试 RED 暴露。Must NOT：不改生产代码；数据 fixture 名（贵州茅台/平安银行）保持字面；en 测试不在此 todo（T15）；HOME_LOCK 保护不变。
  Parallelization: Wave 2 | Blocked by: T1 | Blocks: T5, T6, T10, T11
  References: crates/compass/src/main.rs:1227（HOME_LOCK）；测试行号见 .omo/drafts/gui-i18n.md Findings；.omo/designs/gui-i18n.md §1 键树
  Acceptance criteria: `cargo test` 全绿（断言经 tr() 解析后仍等于当前 zh 文案）；grep 确认应用框架区域无残留字面中文断言（数据 fixture 除外）
  QA scenarios: happy — 全部测试绿；failure — 某 key 缺失 → tr() 返回 key 字符串 → 断言 RED 暴露缺失键。Evidence .omo/evidence/gui-i18n/task-4.log
  Commit: Y | test(i18n): migrate app-framework kittest assertions to tr() helper (ref #222)

- [ ] 5. main.rs/tabs/logger/chart/backend 键化（应用框架 + display-log）
  What to do / Must NOT do: main.rs 全部用户可见串改 t!()：窗口标题保持英文品牌不键化（L67）；启动 modal（L567-573）→ modal.startup.*；watchlist toasts（L897/L913）→ toast.watchlist_*；移除 modal（L928-933）→ modal.remove.*；日志导出（L948/L953）→ toast.log_*；状态栏（L974 加载中…/L997 本地数据源 · %{count} 只）→ statusbar.*/common.loading；工具栏（L1030 加载中…/Fetch → 「获取数据」+ common.loading；L1045 切换侧边栏；L1071 主题已切换；L1090 Data fetched successfully → toast.fetch_success；L1022 前复权 → toolbar.adjust）；自选组标题（L861）→ sidebar.group_watchlist。backend.rs 7 错误串（L84/95/100/144/157/208/231）→ error.* 模板（%{e} 透传）；**6 个 display-log（L122/L125/L178/L181/L251/L254）→ logger.log_* 键（Metis C7）**。tabs.rs TabKind::title（L60-67）→ 返回键常量（"tab.chart" 等），**Tab::title（L108-110）同步**、TabViewer::title（L149-151）渲染时 t!()（Metis M2：渲染消费点在 TabViewer 而非 TabKind）；MaKind::label（screener.rs L51-57）→ 返回键常量（渲染消费在 T6/T11）。logger.rs（L46/47）→ logger.*；chart.rs（L91/93）→ chart.*。ui_fixes_218.rs build_compass_app_with_timeframe（L35-121）同步任何签名变化。Must NOT：不改 ColumnSpec（T6）；不翻译 %{e} 内容；不键化 timeframe 标签 1d/1w/1M；zh 外观仅 Fetch 变化（用户已确认 Q2）。
  Parallelization: Wave 2 | Blocked by: T1, T4 | Blocks: T14
  References: crates/compass/src/main.rs:564-1093；crates/compass/src/backend.rs:84-254；crates/compass/src/tabs.rs:60-67,108-110,149-151；crates/compass/src/citizens/{logger,chart}.rs；crates/compass/src/citizens/ui_fixes_218.rs:35-140；.omo/designs/gui-i18n.md §1
  Acceptance criteria: `cargo test` 全绿（T4 已迁移断言）；`cargo clippy` 干净；zh 界面外观除 Fetch 外零变化（Metis C8 知悉：fork "Go to Realtime" 在 T12 才变）
  QA scenarios: happy — tr("modal.startup.title")=="数据未就绪"；failure — 换 en locale 后工具栏/状态栏/模态英文。Evidence .omo/evidence/gui-i18n/task-5.log
  Commit: Y | feat(i18n): key-ify compass app framework strings (main/tabs/logger/chart/backend) (ref #222)

- [ ] 6. compass-ui 组件键化 + ColumnSpec 键化 + sepa/screener COLUMNS 同步键化
  What to do / Must NOT do: compass-ui 依赖 compass-i18n，`i18n!("locales", fallback = "zh")` 指向共享 locales（spike 结论，Metis M1：**compass-ui 渲染 compass 传入的 sepa.table.*/screener.table.* 键，其 i18n!() 数据必须含全量键**——用共享目录或复制+完整性测试）。widgets/sidebar.rs（L94/L102/L113/L114/L204）→ widgets.sidebar.*；searchable_dropdown.rs（L346 无匹配结果；L421-426 " | " 分隔符保留）→ widgets.searchable_dropdown.*；dropdown.rs（L109 搜索…/L185 无匹配结果/L70 — 保留）→ common.search/no_matches；modal.rs（L110-111 Confirm/Cancel 默认）→ common.confirm/cancel + **modal.rs 测试 L586-587 英文默认断言改 tr()**；multi_select.rs（L77 全部/L79 已选 {} 个/L155 搜索…/L179 完成）→ widgets.multi_select.*；data_table.rs（L151 无符合条件/L157 共 {} 行）→ widgets.data_table.*。**ColumnSpec.header 键化**（data_table.rs L61-68）：header 字段语义改为「键」（&'static str 不变，值=键名），渲染处（L195）`col.header.to_string()` → `t!(col.header)`（每帧渲染天然即时切换）。**同步键化 sepa.rs COLUMNS（L38-87）与 screener.rs COLUMNS（L61-86）为键数组**（"sepa.table.rank" 等，Metis D2：API 键化与消费方同步，否则 missing-key fallback 静默显示 key==中文 literal 假阳性）。API 约束：borrowed &'a str 字段（Input::placeholder/IconButton::tooltip/EmptyState::new/Button::new/SectionTitle::new/Tag::new）用 `let s = t!(...); widget(&s)` 临时绑定（Metis M4：&t!() 不 coerces 到 &str）；插值串（已选 N 个/共 N 行）改 owned String 字段或渲染时构造。Must NOT：不改 compass-ui 对外 API 签名（除 ColumnSpec.header 语义）；不引入业务 crate 依赖。
  Parallelization: Wave 2 | Blocked by: T1, T4 | Blocks: T10, T11
  References: crates/compass-ui/src/widgets/*.rs（行号见 .omo/drafts/gui-i18n.md Findings）；data_table.rs:61-68,151,157,193-218；crates/compass/src/citizens/{sepa,screener}.rs COLUMNS 常量；.omo/designs/gui-i18n.md §1
  Acceptance criteria: `cargo test -p compass-ui` 全绿（8 个 widget 测试文件断言已 tr() 化）；`cargo test` compass 全绿（sepa/screener 表头经 t!() 渲染）；COLUMNS 键数组与键树一致（键完整性测试覆盖）
  QA scenarios: happy — sidebar 空态/占位/提示英文化；failure — 表头 key 缺失时 tr 断言 RED（Metis D2 防假阳性）。Evidence .omo/evidence/gui-i18n/task-6.log
  Commit: Y | feat(i18n): key-ify compass-ui widgets and ColumnSpec headers (ref #222)

- [ ] 7. compass-types 模型加 key 字段（SepaIndicator/SepaFactor/MarketThermometer）
  What to do / Must NOT do: crates/compass-types/src/lib.rs：SepaIndicator（L293-304）`label: String` → `label_key: &'static str`；`value_text: String` → `value: f64` + `unit_key: &'static str`；MarketThermometer（L307-317）`position: String` → `position_key: &'static str`；SepaFactor（L231-241）`label: String` → `label_key: &'static str`，`note: Option<String>` → `note_key: Option<&'static str>` + `note_args: Option<Vec<f64>>`（支持多参数，Metis C5——大资金 note 4 参数、温度计 1 参数）。**unit 精度契约**（Metis C6）：unit_key 枚举化或文档化——percent 1 位小数、count 整数、trillion 2 位小数（precision 常量放 compass-types 或由 UI 按 unit_key 决定，plan 采用：unit_key 值固定三选一，UI 按 key 选择 format spec，契约写进 compass-types 文档注释）。所有编译错误点（compass-strategy/compass/compass-data 消费方）列清单。测试 fixture 更新（temperature.rs L332-479、scoring.rs、compass-data sepa.rs L1046-1142、ui_fixes_221.rs L63/L704/L710、compass-strategy tests L708——Metis M3 全部枚举）。Must NOT：不引入 serde（compass-types 明确非 serde）；不在此 todo 改 strategy 生成逻辑（T8）；不改分数计算逻辑。
  Parallelization: Wave 3 | Blocked by: — | Blocks: T8, T9, T10
  References: crates/compass-types/src/lib.rs:231-241,293-317,320-328；.omo/designs/gui-i18n.md §1 契约（含 C5/C6 修正）；消费方清单见 Metis M3
  Acceptance criteria: `cargo check` workspace 通过（或列出全部编译错误点供 T8/T9 修复）；compass-types 单测全绿；精度契约在字段 doc comment 中写明
  QA scenarios: happy — 类型编译 + 现有数值断言不变；failure — 字段改名后消费方编译错误全列出并逐一处理。Evidence .omo/evidence/gui-i18n/task-7.log
  Commit: Y | refactor(types): add semantic key fields to Sepa types (ref #222)

- [ ] 8. compass-strategy 返回语义 key（temperature + scoring）
  What to do / Must NOT do: temperature.rs：5 indicator label（L179/185/191/197/203）→ label_key 常量（"sepa.indicator.hs300_trend" 等）；value_text（L192 涨停数 → value=limit_up as f64 + unit_key="sepa.unit.count"；L198 成交额 → value + unit_key="sepa.unit.trillion"；L180/186/204 百分比 → value + unit_key="sepa.unit.percent"）；position band（L167-173）→ position_key（"sepa.position.full/mid/low"）。scoring.rs：15 factor label → label_key；**7 note 变体 → note_key + args**（L601 drawdown；L607 momentum_percentile；L646 no_sector_data；L664 news_v1；L695 news_default；**L744-747 大资金 4 参数 → note_key="sepa.note.big_capital" + args 4 个**；**L840 温度计 → note_key="sepa.note.thermometer" + args**——Metis C5）。测试更新：temperature.rs L336/351 "2000 家"/"0 家" → 断言 value/unit_key；position 断言 → position_key；scoring.rs 数值断言不变。Must NOT：strategy 不调 t!()（保持零 UI 依赖）；不改变分数计算逻辑；键常量定义在 compass-types（T7）供引用。
  Parallelization: Wave 3 | Blocked by: T7 | Blocks: T9, T10
  References: crates/compass-strategy/src/sepa/{temperature,scoring}.rs（行号见 Metis M3/.omo/drafts Findings）；.omo/designs/gui-i18n.md §1 sepa.* 键（含 C5 补充）
  Acceptance criteria: `cargo test -p compass-strategy` 全绿（更新后 value/unit_key/position_key/note_key 断言）；`cargo clippy` 干净
  QA scenarios: happy — 断言 SepaIndicator{label_key:"sepa.indicator.limit_up", value:2000.0, unit_key:"sepa.unit.count"}；failure — 分数逻辑不变（对比基线数值）。Evidence .omo/evidence/gui-i18n/task-8.log
  Commit: Y | refactor(strategy): return semantic i18n keys from SEPA scoring (ref #222)

- [ ] 9. compass-data SEPA CSV/backtest 输出更新（保持数据契约）
  What to do / Must NOT do: compass-data/src/sepa.rs：**按中文 label 匹配改为按 label_key 常量匹配**（L163-182 thermometer_csv_row 的 `find("沪深300趋势")` → `find(|i| i.label_key == SepaIndicator::KEY_HS300_TREND)` 或常量）；`parse_pct/parse_count/parse_trillion`（解析 value_text）→ 直接使用 `value: f64`（精度按 unit_key 契约 format）；`csv_field(&tm.position)`（L179）→ `csv_field(&tm.position_key)` 或保持输出 "80%-100%"（**决策：CSV 导出保持中文/数据中性值**——position_key 需映射回原 band 字符串或输出 position_pct 区间，plan 采用：CSV 输出保持原值 "80%-100%" 等，由 position_key→值映射表提供，确保 backtest_result 数据契约不变，Metis C4/AC9）；"市场温度: {:.1} | 仓位建议: {}" println（L131-134）保持中文（CLI 输出 Scope OUT）。L143 format_top_table 中文表头保持。测试：CSV golden 值断言（80%-100%/2000 家/1.20万亿 不变）。Must NOT：不给 compass-data 加 rust-i18n 依赖；不翻译 CLI 输出。
  Parallelization: Wave 3 | Blocked by: T7, T8 | Blocks: —
  References: crates/compass-data/src/sepa.rs:131-134,143,163-182,1046-1142；.omo/designs/gui-i18n.md §1（position/unit 键）
  Acceptance criteria: `cargo test -p compass-data` 全绿（CSV golden 断言）；backtest_result 导出值与原实现一致（对比基线）
  QA scenarios: happy — CSV 行含 "80%-100%"（非 "sepa.position.full"）；failure — 导出含 key 字符串则 RED。Evidence .omo/evidence/gui-i18n/task-9.log
  Commit: Y | fix(data): update SEPA CSV export for keyed model fields (ref #222)

- [ ] 10. sepa.rs 渲染消费 t!()（温度计/indicator/详情/note）
  What to do / Must NOT do: sepa.rs 渲染处（表头已在 T6）：indicator_chip（L226/231 现 &ind.label/&ind.value_text）→ `t!(ind.label_key)` + 按 unit_key 精度 format value（percent 1 位/count 整数/trillion 2 位，Metis C6）+ `t!(ind.unit_key, v = ...)`；thermometer（L185 现 &t.position）→ `t!(t.position_key)`；detail_panel SepaFactor（L587/594）→ `t!(factor.label_key)` + `t!(note_key, args...)`（note 无键则跳过，args 按位置映射 %{0}..%{n} 或命名）。测试 fixture 改用 key 字段 + 断言 t!() 结果（ui_fixes_221.rs L63/L704/L710 同步）。Must NOT：不改 strategy 生成（T8 已完成）；不改 compass-types。
  Parallelization: Wave 4 | Blocked by: T6, T7, T8 | Blocks: —
  References: crates/compass/src/citizens/sepa.rs:161-210,226-241,542-605；.omo/designs/gui-i18n.md §1 契约
  Acceptance criteria: `cargo test` sepa 相关全绿；zh 界面外观零变化（t!(key) == 原中文）
  QA scenarios: happy — 温度计/详情面板 tr() 断言；failure — en 下 indicator/factor 标签英文。Evidence .omo/evidence/gui-i18n/task-10.log
  Commit: Y | feat(i18n): render SEPA indicator/factor keys via t!() (ref #222)

- [ ] 11. screener.rs 渲染消费 t!()（表单/按钮/卡片）
  What to do / Must NOT do: screener.rs（表头已在 T6）：L249 筛选 → screener.filter；L263 → error.screener_run；L284 筛选进行中… → screener.filtering；卡片标题 L328/335 → screener.card_basic/card_technical；表单标签 L358/363/368/373/374/391/394/401/409 → screener.*（含 min /max 前缀、不限/≥1年 选项）；技术面 L427/457/459/466/468/470/472/479/481/483 → screener.*（MaKind 下拉选项在 T6 已键化）。测试断言迁移（L625-937）→ tr()。Must NOT：不动 screener 表单逻辑/宽度常量（T15 处理 en 布局）。
  Parallelization: Wave 4 | Blocked by: T6 | Blocks: —
  References: crates/compass/src/citizens/screener.rs:236-488；.omo/designs/gui-i18n.md §1 screener.*
  Acceptance criteria: `cargo test` screener 相关全绿；`cargo clippy` 干净
  QA scenarios: happy — tr("screener.filter")=="筛选"；failure — en 下表单/筛选按钮英文。Evidence .omo/evidence/gui-i18n/task-11.log
  Commit: Y | feat(i18n): key-ify screener citizen strings (ref #222)

- [ ] 12. fork i18n 接入（独立仓库 qiboda/egui-charts compass 分支，精确 commit）
  What to do / Must NOT do: fork 仓库：Cargo.toml 加 rust-i18n 4.2.1 dep；src/lib.rs 顶部 `i18n!("locales", fallback = "zh")` + locales/zh.yml + en.yml（**zh 值 = 现状中文格式串**：tooltip 时间:/开盘:…、日期 %Y年%-m月%-d日/%-m月%-d日 %H:%M:%S/%-m月/%-m月%-d日、realtime 实时；en 值 = 设计文档对照）；tooltip.rs（L80/82/100/107-114/120/232-239/253）→ t!()；crosshair.rs（L218/221）→ t!()；time_formatter.rs DefaultTimeFormatter（L60/62）→ t!()（LocaleTimeFormatter/RelativeTimeFormatter 不动）；labels.rs legend（L328/354-378/401）→ t!()；realtime_btn.rs（L36 Go to Realtime → t!("chart.realtime") 默认 + 保留 with_realtime_button_text 覆盖）；暴露 `pub fn set_locale(locale: &str)` wrapper（内部 rust_i18n::set_locale，Metis M6 compass 单一调用点已覆盖）。**format! 捕获标识符修复**：tooltip.rs L120/L253、labels.rs L401 的 `{sign}`/`{change_pct:.2}` → 显式命名参数 `format!(t!(...), sign = sign, change_pct = change_pct)`（运行期格式串不支持捕获标识符）。fork 测试更新：tooltip 测试表（L530-536 6 项）+ 日期断言、crosshair 2、time_formatter 1、timescale_marks 2（Metis C9 实数 ≈10 处）→ t!() 解析。Must NOT：不动 src/ui/（feature 门控）；不动 LocaleTimeFormatter/RelativeTimeFormatter；不引入 egui 版本变更；**不建 chart.legend.* 键**（Metis C3）。
  Parallelization: Wave 5 | Blocked by: — | Blocks: T13
  References: ~/.cargo/git/checkouts/egui-charts-a14ffbf1d5a8ad83/a1531ac/src/chart/renderers/{tooltip,crosshair,labels}.rs + src/scales/time_formatter.rs + src/chart/rendering/overlays/realtime_btn.rs（行号见 .omo/drafts/gui-i18n.md Findings）；.omo/designs/gui-i18n.md §1 chart.* 键
  Acceptance criteria: fork 仓库 `cargo test` 全绿（t!() 化断言）；`cargo build` 通过；set_locale("en") 后 fork 渲染英文；fork diff 仅限 4 文件可达文本 + lib.rs + Cargo.toml + locales
  QA scenarios: happy — fork 单测断言 t!() 中文/英文；failure — 捕获标识符未修则编译失败。Evidence .omo/evidence/gui-i18n/task-12.log
  Commit: Y | feat(l10n): add rust-i18n with zh/en locales to chart rendering (ref #222)

- [ ] 13. fork 测试更新 + compass bump 精确 commit
  What to do / Must NOT do: fork 测试全绿后 push fork compass 分支（pin 精确 commit SHA，**不 pin branch head**——fork 头可能有无关 "brain: session" commit，Metis S3）；compass 侧 `cargo update -p egui-charts --precise <i18n-sha>`（Cargo.lock rev 更新，**精确 SHA 而非 branch head**）；`cargo build`（compass-ui 也依赖 egui-charts，Metis S3）+ `cargo test` 验证；确认新 rev 引入的 fork 改动与 T12 一致（review fork diff scope）。Must NOT：不手动改 Cargo.lock；fork 未合入不 bump；不引入 fork 无关 commit。
  Parallelization: Wave 5 | Blocked by: T12 | Blocks: —
  References: /data/codes/compass/.worktrees/gui-i18n/Cargo.toml:20；Cargo.lock:1728-1730（当前 rev a1531ac）；crates/compass-ui/Cargo.toml（同依赖）
  Acceptance criteria: `cargo build` 全 workspace 通过；`cargo test` 全绿（含 fork t!() 化后的图表测试）；Cargo.lock egui-charts source 含精确 i18n SHA
  QA scenarios: happy — bump 后 chart citizen 测试绿；failure — fork 未合入则 cargo update 拉到旧 rev。Evidence .omo/evidence/gui-i18n/task-13.log
  Commit: Y | build(deps): bump egui-charts to i18n commit (ref #222)

- [ ] 14. 语言切换 UI（工具栏下拉 + config 写回 + 持久化测试）
  What to do / Must NOT do: 工具栏 Group D（main.rs L1042-1074）主题 Dropdown 右侧加语言 Dropdown（宽度 ~76px，选项 中文/English 母语名，**不键化**，Metis：语言名惯例）；切换 → `compass_i18n::set_locale("zh"|"en")` + `ctx.request_repaint()` + `ctx.send_viewport_cmd(egui::ViewportCommand::Title("Compass — Stock Chart"))`（标题品牌英文不变，Metis S5）+ Info toast tr("toast.language_switched") + **save_language_config**（新增，复用 save_watchlist_config L413-440 的 toml::Value read-modify-write：读文件 → doc.insert("language", ...) → 写回，**保留 app/watchlist/screener/parquet/dolt 节**；注意该模式会丢失 config 注释——既有行为，Metis M5 知悉）。CompassApp 加 language 状态字段（当前 locale）。测试（HOME_LOCK + LANG_LOCK 双锁，Metis M9）：语言切换 kittest（选 English → 断言 toolbar.fetch 变 "Fetch"/状态栏英文 + config 文件 language="en"）；持久化 round-trip（config language="en" → load_config 返回 en → 重启模拟 UI 英文，Metis AC3）；非法值回退（AC4）。Must NOT：语言选项名不键化；不做自动重启；不改 save_watchlist_config/save_screener_config 现有函数。
  Parallelization: Wave 6 | Blocked by: T3, T5 | Blocks: —
  References: crates/compass/src/main.rs:413-440（save_watchlist_config 模式）、1042-1093（工具栏 Group D）、1227（HOME_LOCK）；.omo/designs/gui-i18n.md §2
  Acceptance criteria: `cargo test` 语言切换测试绿（kittest 点击 English → 界面英文 + config language="en"）；重启后仍 en；非法值回退 zh + warn；窗口标题保持英文品牌
  QA scenarios: happy — 切 en 断言 fetch 按钮/状态栏/主题 toast 英文；failure — 写回失败仅 warn 不崩溃。Evidence .omo/evidence/gui-i18n/task-14.log
  Commit: Y | feat(i18n): add language dropdown with config persistence (ref #222)

- [ ] 15. en 布局 sweep 测试（LANG_LOCK 串行）
  What to do / Must NOT do: 新增 en 专项布局测试：SEPA 1400px 详情面板测试（sepa.rs L914 pane_w=1400）在 en locale 复跑；screener 5 档宽度对齐测试（GROUP_ALIGNMENT_WIDTHS = 500/600/800/1000/1200，screener.rs L848）在 en 复跑；**具体阈值断言**（Metis AC6：哪些宽度必须过、断言精确、若需上调 technical_group 宽度常量 158/274/286/390 + label_w+176 则记录新常量值）；验证无面板 en 下破损（无坐标断言失败）。**LANG_LOCK**：新增 `static LANG_LOCK: Mutex<()>`（仿 HOME_LOCK main.rs L1227），**所有 set_locale 调用（含 zh 重置）持锁**（Metis M9），与 HOME_LOCK 组合时按顺序获取（HOME→LANG 或反之，统一约定）。Must NOT：不改布局算法（仅宽度常量微调）；en 测试串行执行。
  Parallelization: Wave 6 | Blocked by: T5, T6, T10, T11, T14 | Blocks: —
  References: crates/compass/src/main.rs:1227（HOME_LOCK 模式）；crates/compass/src/citizens/screener.rs:420-488,848-925（technical_group 宽度 + 5 档测试）；crates/compass/src/citizens/sepa.rs:914（1400px 测试）；.omo/designs/gui-i18n.md §3（布局风险评估表）
  Acceptance criteria: `cargo test` 全部绿（含 en 布局测试）；en 下无坐标断言失败；若调整宽度常量则记录新值并同步 kb/design/ui.md
  QA scenarios: happy — en 下 SEPA 表格 12 列无溢出、screener 表单无断行；failure — 宽度常量不足则坐标断言 RED。Evidence .omo/evidence/gui-i18n/task-15.log
  Commit: Y | test(i18n): add serialized en-layout sweep tests (ref #222)

- [ ] 16. 文档同步（kb/design/ui.md + kb/user/config.md + kb/user/gui.md + ui-widgets.md + AGENTS.md + check-coverage.sh）
  What to do / Must NOT do: kb/design/ui.md：设计变更记录表追加一行（日期 2026-08-09、i18n 全量中文化、来源 .omo/designs/gui-i18n.md、实现状态）+ 决策记录表追加 i18n 相关决策（语言键机制/切换 UI/fork 策略/strategy key 契约）；更新工具栏段（语言下拉）、图表日期段（fork per-locale，原「tooltip 前缀全中文化」改为「按 locale 键化」）。kb/user/config.md：language 键说明 + 默认值表（"（顶层）| language | zh"）+ 配置示例。**kb/user/gui.md**（Metis 补漏：AGENTS.md 映射表 GUI 变更 → gui.md 是主要文件）：窗口标题/toolbar Fetch/自选股为空/本地数据源/东方SEPA 等文案说明更新（含语言切换说明）。kb/design/ui-widgets.md：ColumnSpec.header 语义（持键）、modal 默认文案键化、multi_select/data_table 键化说明（L533 ColumnSpec、L556-560 Modal 段）。**AGENTS.md + scripts/check-coverage.sh**：compass-i18n 加入覆盖率门槛表（80%，Metis M7）。Must NOT：不改 .omo/designs/gui-i18n.md（过程归档不删不改）；不夸大实现（每个声称都以代码证据核实）。
  Parallelization: 收尾（独立） | Blocked by: T1-T15 | Blocks: —
  References: kb/design/ui.md（设计变更记录表 L215-222 + 决策记录表 L227+）；kb/user/config.md（默认值表 L70-80）；kb/user/gui.md；kb/design/ui-widgets.md；AGENTS.md（覆盖率门槛）；scripts/check-coverage.sh:22-27,76-81
  Acceptance criteria: 各 kb 文件含 language/ColumnSpec/语言切换相应更新；AGENTS.md 覆盖率表含 compass-i18n；grep 确认无遗漏（config.md 有 language 键、ui.md 有变更记录行）
  QA scenarios: happy — kb 文件内容与实现一致（抽查）；failure — AGENTS.md 未加覆盖率则 CI 审查发现。Evidence .omo/evidence/gui-i18n/task-16.log
  Commit: Y | docs(i18n): sync kb design/config/user docs with i18n changes (ref #222)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 逐 todo 核对：16 todo 全部完成；键树与设计文档 §1（含 Metis C5/C7 补充）全量一致；Scope OUT 无一违反（无 CLI 翻译、无 fork src/ui/ 键化、无数据翻译、无第三语言）
- [ ] F2. Code quality review — /review-work 5 并行 agent（goal/quality/security/QA/context）全过
- [ ] F3. Real manual QA — `cargo test` 全绿 + `cargo llvm-cov` 覆盖率达标（含 compass-i18n 80%）+ en/zh 双语言 GUI 冒烟（scripts/run.sh 切换语言验证 + fork 图表 tooltip/日期随语言切换）
- [ ] F4. Scope fidelity — fork 改动仅限可达文本（无 src/ui/）；compass-data CSV 契约不变（golden 值）；键完整性测试绿；zh 界面除已确认项（Fetch 获取数据、fork realtime 实时）外零变化

## Commit strategy
- 每 todo 独立 commit，message 含独立成行 `ref #222`（hook 校验 OPEN issue）
- commit → /review-work review（每 commit 后，最多 2 轮修复）
- fork 改动：fork 仓库独立 commit + push compass 分支（pin 精确 SHA）→ compass `cargo update -p egui-charts --precise <sha>` + bump commit
- 文档同步 commit（T16）与其他实现 commit 分开
- 全部 commit 完成后 → user 确认 push → /skwy-reflect 反思 commit（随 PR 推送）→ push
- push 前 rebase origin/master；push 后 issue #222 完成 comment + close

## Success criteria
- 默认中文 / 配置切换 zh|en / 资源集中管理（单一 locales/ 无散落硬编码——键完整性测试保证）/ CJK 无截断错位（en 布局 sweep 保证）
- 全 GUI（含 fork 图表 tooltip/日期）随 set_locale 即时切换语言
- kittest 断言全部键解析化，en 专项测试 LANG_LOCK 串行稳定
- 覆盖率门槛含 compass-i18n；zh 界面仅 2 处已确认外观变化；compass-data CSV 契约不变
