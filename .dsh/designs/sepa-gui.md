# 东方SEPA 多因子评分选股面板 — GUI 设计方案

> 状态：待用户确认（过程归档，非权威；确认后由主 agent 同步到 `kb/design/ui.md`）
> 面向：实现 agent（compass GUI 扩展，不新增任何 UI 依赖）

## 目标

在 compass GUI 中新增「东方SEPA」多因子评分选股面板：

- 展示全市场每日 TOP N 排名（评分 = 趋势30% + 题材25% + 资金20% + 形态20% + 风险-5%）
- 展示评分详情（五模块得分 + 模块子项明细）
- 展示市场温度计（温度分 + 仓位建议 + 五项指标）
- 复用现有 citizen → Signal → AsyncDispatcher → SharedState 通道与图表联动机制
- 遵循 `kb/design/ui.md` 设计系统（compass-ui token、A 股红涨绿跌、思源黑体 + JetBrains Mono、egui-phosphor 图标）

## 现状

| 关注点 | 现状 | 文件 |
|---|---|---|
| 选股器面板 | 条件表单（两张 Card）+ 结果 `DataTable`，行点击联动图表 | `crates/compass/src/citizens/screener.rs` |
| 数据通道 | `wire_backend` 建两条 Signal/Slot 通道（bars + screener），`AsyncDispatcher` 跑 tokio，result slot 写回 `SharedState` | `crates/compass/src/backend.rs` |
| 消息类型 | `AppMessage` / `FetchRequest` / `RunScreenerRequest` / `RunScreenerResponse` | `crates/compass/src/messages.rs` |
| 共享状态 | `screener_result` / `screener_total` / `screener_loading` / `screener_error` 等 `Dynamic<T>` 字段 | `crates/compass/src/state.rs` |
| Dock 标签页 | `TabKind { Chart, Logger, Screener }`，中文标题 + Phosphor 图标，CitizenId 一一对应 | `crates/compass/src/tabs.rs`、`dispatcher.rs` |
| 表格组件 | `DataTable`：`ColumnSpec` + `DataCell { Text, Price, Count }`，表头点击排序，空态「无符合条件」，计数「共 N 行」 | `crates/compass-ui/src/widgets/data_table.rs` |
| 设计系统 | `ColorTokens`（dark/light）、`SpacingTokens`、`TypeTokens`、Tag/EmptyState/PriceText/Button/Segmented/Card 等组件 | `crates/compass-ui/src/tokens/`、`widgets/` |
| 线程模型 | UI 主线程渲染，I/O 与全市场计算在 tokio runtime；全市场扫描不冻结 UI | `kb/design/architecture.md` |

关键复用点（照抄 screener 的成熟模式，不做新机制）：

- 行点击联动图表：`dispatch_row_fetch`（`screener.rs:470-487`）——置 `shared_state.symbol` + 派发 `AppMessage::FetchBars`
- 状态切换：`results_area` 的 loading / error / empty 三段式分支（`screener.rs:270-291`）
- 错误 toast：`main.rs` 中 `screener_error` 的 None→Some 转换推送（`main.rs:536-542`）
- 主题切换：`set_tokens` 刷新面板内 stateful 组件（`screener.rs:461-467`）

## 设计方案

### 1. 面板结构：独立标签页「东方SEPA」

**独立 tab（`TabKind::Sepa`，CitizenId `"sepa"`），不并入选股器。** 二者心智模型不同：

- 选股器 = **查询型**：用户交互式构造条件 → 全量重算
- SEPA = **报告型**：每日预计算的固定排名，打开即读

合并会使选股器面板同时承载「条件表单 + 排名表 + 详情」三层，违背单一职责。

**初始 dock 位置：叠入 Chart leaf（顶部全宽 leaf，与「图表」共享同一 leaf 的 tab 栏）。**
理由：SEPA 表格 12 列 + 详情面板需要 ~900px 宽度，而底部行 leaf（日志 | 选股器 并排）只有
~600px 宽 × 25% 高，放不下。顶部 leaf 75% 高 × 全宽，温度计条 + 表格 + 详情均可容纳。
egui_dock 支持同一 leaf 多 tab，用户可随时拖出为独立 leaf。

布局草图（顶部 leaf 内）：

```
┌─ dock tab 栏: [图表] [东方SEPA] ──────────────────────────────────────┐
│ ┌─ ① 市场温度计 Card（横向条，恒显示）────────────────────────────┐   │
│ │ [温度计icon] 市场温度 72.0   [仓位建议 Tag: 半仓 50%]             │   │
│ │ [上涨占比 62% ▲] [涨停 45 ▲] [连板 4板 ▲] [成交 1.24万亿 ▼] ...  │   │
│ └─────────────────────────────────────────────────────────────────┘   │
│ ┌─ ② 工具条 ───────────────────────────────────────────────────────┐  │
│ │ 共 50 行 · 2026-08-02 评分        [TOP 50|30 Segmented] [刷新  ⟳] │  │
│ └──────────────────────────────────────────────────────────────────┘  │
│ ┌─ ③ 排名表格（可滚动，~2/3 宽）───┬─ ④ 评分详情（固定 ~300px）───┐    │
│ │ 排名│代码│名称│总分│趋势│题材│... │ 600519 贵州茅台      #1      │    │
│ │  1 │600519│茅台│88.5│27 │21 │... │ 总分 88.5                    │    │
│ │  2 │300750│宁德│85.0│25 │20 │... │ ── 趋势 27.0/30  ▓▓▓▓▓▓▓░░  │    │
│ │  ⋮                              │   VCP质量分  9.2/10          │    │
│ │                                  │   突破确认分  8.8/10          │    │
│ └──────────────────────────────────┴──────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────┘
```

面板内部垂直排布（自上而下）：① 温度计条 → ② 工具条 → ③④ 表格+详情水平分栏。

### 2. 表格列（12 列，默认全显，全部可排序）

| # | 列头 | 单元格类型 | 说明 |
|---|---|---|---|
| 0 | 排名 | `Rank` | 后端给出的官方排名（随行携带，不随排序变位）；默认排序列（升序 = 官方顺序）；1–3 名 warning 强调 |
| 1 | 代码 | `Text` | mono，裸 6 位代码 |
| 2 | 名称 | `Text` | 中文名 |
| 3 | 总分 | `Score{value: total, max: 100}` | 色阶着色，mono |
| 4 | 趋势 | `Score{value, max: 30}` | 权重 30% |
| 5 | 题材 | `Score{value, max: 25}` | 权重 25% |
| 6 | 资金 | `Score{value, max: 20}` | 权重 20% |
| 7 | 形态 | `Score{value, max: 20}` | 权重 20% |
| 8 | 风险 | `Score{inverted: true}` | 显示带符号扣分 `-x.x`，按 `1-|v|/max` 反向色阶（0 扣分绿 → 满扣分红） |
| 9 | 行业 | `Text` | 行业；后端提供题材时拼 `行业 · 题材1 · 题材2`（最多 2 个），题材在前缀之后排序自然正确 |
| 10 | 最新价 | `Price{value, change: None}` | mono，flat 色 |
| 11 | 涨跌幅 | `Price{value: change, change: Some}` | 当日涨跌幅，A 股红涨绿跌 |

默认排序：`set_sort(0, false)`（排名升序）+ `set_descending_default` 对总分/五模块列设为降序
（切换至这些列时自动降序，与 screener 市值列同款业务偏好）。

排序语义沿用 `sort_rows`（`data_table.rs:227`）：Text 字典序、数值列数值序、并列按第 0 列升序。

### 3. 评分详情：右侧详情面板（点击行展开）

**方案：右侧固定宽度详情面板（~300px），点击行即时刷新内容 + 表格行高亮。**
不用「行内展开」：`DataTable` 基于 `TableBuilder`，组件内部不支持展开行；行内展开需重构组件且
50 行下展开条会被滚动挤出视野。右侧面板用最少组件改动达成同一目标，且是股票排名工具的通行
形态（同花顺/通达信式 列表+详情）。

行点击同时触发两件事（与 screener 单次点击一致）：
1. `dispatch_row_fetch` → 图表联动（详见 §5.3）
2. 详情面板切换到该行内容 + 该行高亮

详情面板内容（自上而下，可滚动）：

```
600519 贵州茅台                    [Tag: #1]      ← 头部：名称 + 排名 Tag
总分 88.5                                          ← display 字号 mono，色阶色
─────────────────────────────────
趋势 27.0/30    ▓▓▓▓▓▓▓░░░  90%                    ← 模块行：标签 + 分数 + 进度条
  · VCP 质量分     9.2/10    +1.2亿                ← 子项：caption 标签 + mono 分数 + 备注
  · 突破确认分     8.8/10
  · RS 排名        28/1000（前 3%）
题材 21.5/25    ▓▓▓▓▓▓▓░░░  86%
  · 主线契合度     8.0/10
  · 题材持续性     6.5/10
资金 17.2/20    ▓▓▓▓▓▓▓▓░░  86%
  · 主力净流入     8.6/10   +1.2 亿                ← 备注显示原始值（text_secondary）
形态 16.4/20    ▓▓▓▓▓▓▓▓░░  82%
  · 平台突破质量   7.9/10
风险 0.0/-5     ▓░░░░░░░░░  0%                      ← 风险模块：0 分 = 无扣分
  · 高位放量       0.0/2   （无）                    ← 无风险时显示弱化文字「无」
  · 破位风险       0.0/3
─────────────────────────────────
题材: [白酒] [茅指数]                                ← 题材 Tag 区（后端提供时）
```

要点：

- 模块行 = 模块名 + `得分/满分`（mono，色阶色）+ egui 原生 `ProgressBar`（fill 色 = 色阶色，
  显示百分比）
- 子项 = 通用 `SepaFactor { label, score, max, note }` 列表，**GUI 不做任何业务解析**，
  按后端返回原样渲染；note（如主力净流入原始值）以 caption/text_secondary 显示
- 风险模块子项含义特殊（扣分项），分数按色阶反向着色（扣分多 = 红）
- 题材 Tags 复用 `Tag` 组件（`TagVariant::Custom` + `score_color` 着色）
- 无选中行时显示占位：`EmptyState` 小号版「点击排名行查看评分详情」或弱化文字

### 4. 温度计：面板顶部横向条

**方案：顶部横向 Card 条（恒显示，不随选中行变化）。** 温度计是**市场级**信息，不是个股级，
放侧栏会与详情面板抢空间且被滚动挤出；放顶部条保证打开 SEPA 面板即见全局热度。

```
┌─ Card「市场温度计」───────────────────────────────────────────────┐
│ [THERMOMETER icon] 市场温度 72.0    [Tag: 半仓 50%]               │
│ ─────────────────────────────────────────────                    │
│ [上涨占比 62% ▲] [涨停家数 45 ▲] [连板高度 4板 ▲]                │
│ [两市成交 1.24万亿 ▼] [北向净流入 +18亿 ▲]    ← 5 个指标 chip      │
└──────────────────────────────────────────────────────────────────┘
```

- 左段：温度计图标（egui-phosphor `THERMOMETER`）+ 「市场温度」caption 标签 + 分数
  （display 20px mono，`score_color(score/100)`）
- 中段：仓位建议 `Tag`（文字如「空仓/轻仓/半仓/重仓」+ 仓位百分比），Tag 底色 = 色阶色
  （仓位 100% → 绿，0% → 红）
- 右段：5 个指标 chip，每个 = `label(caption, secondary) + value(mono) + delta(▲/▼)`；
  chip 底色 tint = `score_color(heat)`（热度高绿/低红），delta 箭头按 A 股红涨绿跌着色
  （▲ 红 / ▼ 绿）——**两套色语义并存且职责清晰：色阶 = 热度水平，涨跌色 = 环比方向**
- 指标具体名称/口径由数据侧决定（见「待确认」），GUI 按通用 `SepaIndicator` 渲染，零硬编码

### 5. 交互

#### 5.1 刷新按钮

- 工具条右侧：`Button(Primary)` 图标 `ARROW_CLOCKWISE` + 文字「刷新」
- 点击：`sepa_loading.set(true)` → `RunSepaRequest` 经新通道发出（与筛选按钮同款逻辑，
  `screener.rs:246-262`）
- loading 期间：按钮禁用 + 内嵌 spinner + 文案「计算中…」；完成后恢复
- 成功：Success toast「SEPA 评分已更新 · 50 只」（沿用 `main.rs` 的 toast 转换模式）；
  失败：Error toast（`sepa_error` None→Some 转换推送，复用 `main.rs:536-542` 模式）

#### 5.2 TOP N 切换（50 / 30）

- `Segmented::new(&tokens, ["TOP 50", "TOP 30"])`，默认 50
- **纯 GUI 截断，不触发后端重算**：后端始终返回完整 TOP50，面板按 `top_n` 截断渲染
  （`rows.truncate(top_n)`），切换瞬时完成；计数标签同步「共 30 行」
- 理由：全市场五模块评分计算代价高（秒级），切 30↔50 反复触发重算是浪费；
  50 行数据量本地截断成本可忽略

#### 5.3 行点击 → 图表联动

完全复用 `dispatch_row_fetch`（`screener.rs:470-487`）：
置 `shared_state.symbol` → 取当前 timeframe → `dispatcher::handle(AppMessage::FetchBars, …)`。
建议把该函数从 screener 模块提升为共享函数（如 `dispatcher.rs` 或 `citizens/mod.rs`），
选股器与 SEPA 共用一份实现。

#### 5.4 首次打开：纯手动（已确认）

- **不做**首次打开自动计算。SEPA 标签页打开后显示空态（EmptyState「暂无 SEPA 评分数据 / 点击刷新计算全市场 TOP50 评分」），用户点击「刷新」才触发计算
- 理由：全市场五模块评分为秒级计算，避免打开标签页时的意外计算耗时；行为与选股器一致（纯手动触发）

#### 5.5 快捷键

本期不加（与选股器一致，保持「按钮 + 鼠标」最小面）。`R` 刷新与输入框聚焦有冲突风险，
不做。后续如需可加 `Ctrl+R` 刷新 SEPA（Ctrl 组合不受文本输入焦点守卫影响）。

### 6. 视觉与状态

#### 6.1 分数色阶（高分绿 / 低分红）

新增纯函数（放 compass-ui，供表格/详情/温度计共用）：

```
fn score_color(tokens: &ThemeTokens, norm: f32) -> Color32   // norm ∈ [0,1]
  norm ≥ 0.8  → success（绿 #34C77B dark / #188A51 light）
  0.5–0.8     → Color32::lerp(warning, success, (norm-0.5)/0.3)
  0.25–0.5    → Color32::lerp(error, warning, (norm-0.25)/0.25)
  < 0.25      → error（红 #EF5350 dark / #D93025 light）
```

归一化规则：总分 `norm = total/100`；模块分 `norm = score/权重`（趋势/30、题材/25、资金/20、
形态/20）；**风险列 `norm = 1 - |risk|/3.75`**（risk ∈ [-3.75, 0]——扣分合计上限 75×0.05=3.75，与引擎侧"风险贡献 = −扣分合计×0.05"契约对齐，审查修订；扣分越多越红，0 扣分 = 绿）。
排名 1–3 的「排名」单元格文字用 `warning`（琥珀）强调。

色阶为语义色（success/warning/error），不占用 A 股涨跌色语义——涨跌幅列仍用 `up`/`down`。

#### 6.2 字体与间距

- 所有数值（分数、价格、涨跌幅、排名）mono（JetBrains Mono，`TypeTokens.mono` 12px）
- 名称/标签 body；模块子项标签 caption
- 面板内间距：温度计条内 `padding md`（Card 默认）、工具条与表格间 `spacing.sm/md`、
  详情面板内模块间距 `spacing.md`——全部走 `SpacingTokens`，不硬编码

#### 6.3 状态

| 状态 | 表现 | 位置 |
|---|---|---|
| Loading | `ui.spinner()` + 「SEPA 评分计算中…（全市场）」 | 表格区（详情面板置灰/保留上次内容） |
| Error | `colored_label(error_fg_color, msg)` + Error toast | 表格区 + 右上角 toast |
| 空态（从未计算/数据缺失） | `EmptyState`：「暂无 SEPA 评分数据」+ 描述 +「刷新」action 按钮 | 表格区 |
| 无选中行 | 详情面板显示弱化占位「点击排名行查看评分详情」 | 详情面板 |

空态文案：「暂无 SEPA 评分数据 / 点击刷新计算全市场 TOP50 评分」，icon 建议
egui-phosphor `CHART_SCATTER`（具体字形以实现时确认）。

### 7. 边界类型与接线（GUI 侧契约）

以下为 GUI 依赖的数据契约；具体字段口径由数据/策略侧实现确认（见「待确认」）。

**compass-types（边界 crate，GUI ↔ strategy 共用）**：

```rust
pub struct SepaFactor { pub label: String, pub score: f64, pub max: f64,
                        pub note: Option<String> }      // 子项；note = 原始值展示

pub struct SepaDetails { pub trend: Vec<SepaFactor>, pub theme: Vec<SepaFactor>,
                         pub capital: Vec<SepaFactor>, pub pattern: Vec<SepaFactor>,
                         pub risk: Vec<SepaFactor> }

pub struct SepaRow {
    pub symbol: String,  pub name: String,  pub rank: usize,
    pub total_score: f64,                    // 0..100
    pub trend: f64, pub theme: f64, pub capital: f64, pub pattern: f64,
    pub risk: f64,                           // -3.75..0（扣分贡献，上限 75×0.05）
    pub industry: String, pub themes: Vec<String>,   // 题材可能为空
    pub latest_price: f64, pub change_pct: f64,      // 当日涨跌幅 %
    pub details: SepaDetails,
}

pub struct SepaIndicator { pub label: String, pub value_text: String,
                           pub delta_pct: Option<f64>,   // 较昨日，A 股色
                           pub heat: f64 }               // 0..1，色阶 tint

pub struct MarketThermometer { pub score: f64, pub position: String,
                               pub position_pct: f64,    // 0..100
                               pub indicators: Vec<SepaIndicator> }  // 5 项

pub struct SepaData { pub rows: Vec<SepaRow>,           // 完整 TOP50，官方排序
                      pub thermometer: MarketThermometer,
                      pub date: String }                // 评分日期（如 2026-08-02）
```

**messages.rs**：`RunSepaRequest {}`（无参；top_n 为纯 GUI 态）+ `RunSepaResponse { data: SepaData, error: Option<String> }`

**state.rs**：新增 `sepa_data: Dynamic<Option<SepaData>>`、`sepa_loading: Dynamic<bool>`、
`sepa_error: Dynamic<Option<String>>`（单个 `Option<SepaData>` 字段，避免 rows/thermometer
分开造成半更新状态）

**backend.rs**：第三条 `AsyncDispatcher<RunSepaRequest, RunSepaResponse>` 通道，
`wire_backend` 返回值增加 `Signal<RunSepaRequest>`；result slot 写回上述字段 + 日志
（照抄 screener 通道，`backend.rs:114-168`）。

**策略侧契约（审查 M1 修订）**：后端 handler 调用 `compass_strategy::sepa::run_sepa(...)`（lib.rs 将加 `pub mod sepa;`，模块路径为 `compass_strategy::sepa::run_sepa`，拆分审查修订）——
该函数当前不存在，由策略侧子 issue 实现，签名假设为：

```rust
// 读 Parquet（stock_daily/stock_basic/concept_*/capital_*）→ 过滤 + 评分 → 当日快照
pub fn run_sepa(query: &SepaQuery, reader: &ParquetReader,
                now: NaiveDate) -> Result<SepaData, ScreenerError>
pub struct SepaQuery { pub top_n: usize }   // 后端截断上限（默认 50）
```

- GUI 集成测试依赖 `run_sepa` 可运行：测试顺序排在策略侧子 issue 之后，或用 stub
  实现（返回构造的 `SepaData` fixture）先行落地通道测试
- **wire_backend 返回值从 2-tuple 变 3-tuple**，波及 main.rs:73、backend.rs 测试 4 处
  （`:282,:327,:364,:473`）、main.rs 测试 1 处（`:1044`）——全部同步改解构
- **TOP N 截断只作用于面板本地渲染副本，绝不回写 shared_state**（写回会污染数据，
  切回 50 时行已丢）；刷新成功后重置 `selected`（索引指向旧数据）

**接线链**：`main.rs` 持有 sepa signal → `tabs.rs TabKind::Sepa` 新增 variant + `dispatcher.rs`
注册 citizen → `TabViewer` 传 signal 给 `SepaPanel::show` → 主题切换调用 `sepa.set_tokens`。

**compass-ui 最小扩展**（自建组件库，非新依赖）：

1. `DataCell::Score { value: f32, max: f32, inverted: bool }` 变体——mono 着色数值，数值排序（按 value）。
   `inverted = true`（风险列专用）：显示带符号值 `-3.2`，色阶 norm = `1 - |value|/max`（0 扣分绿 → 满分扣分红）；
   `inverted = false`（总分/模块列）：norm = `value/max`。渲染格式统一 `{:.1}`。
2. `DataCell::Rank(usize)` 变体——排名列专用：数值排序，rank 1–3 渲染 `warning` 强调，其余 `text_primary`
3. `score_color(tokens, norm)` 公开纯函数（§6.1），放置 `crates/compass-ui/src/widgets/`（新 `score.rs` 或并入 data_table.rs 同模块）
4. （推荐）`DataTable::set_selected(Option<usize>)` 行高亮——`selection_bg` 底色，
   详情面板联动选中态；可选，不做也成立（仅详情面板切换）

**GUI 面板**：`crates/compass/src/citizens/sepa.rs` → `SepaPanel`（字段：citizen_id/
citizen_state/tokens/table/top_n/selected/on_auto_run 标记），结构镜像 `ScreenerPanel`。

## 交互效果

| 触发 | 表现 | 时长/缓动 | 目标状态 |
|---|---|---|---|
| 行 hover | 行底色 `bg_hover` | 即时 | 与 DataTable 现有 hover 一致 |
| 行点击 | 详情面板内容切换 + 行高亮 `selection_bg` + 图表联动 | 即时（不引入动画；可选 100ms fade-in，按 MotionTokens） | 选中行高亮、详情更新、图表切到该股 |
| 刷新点击 | 按钮禁用 + spinner + 「计算中…」；表格区 spinner | 即时 | `sepa_loading = true` |
| 计算完成 | 表格填充 + 温度计更新 + Success toast | 即时 | `sepa_loading = false`；toast 3s |
| 计算失败 | 表格区错误文案 + Error toast | 即时 | toast 8s |
| TOP N 切换 | 表格行数即时变化 + 计数标签更新 | 即时（无重算） | 「共 30 行」 |
| 首次打开标签页 | 空态 + EmptyState「刷新」按钮（不自动计算） | — | 见 §5.4 已确认 |
| 主题切换 | `set_tokens` 刷新面板全部组件 | 即时 | 与新主题一致 |

面板整体不引入装饰性动画——信息密度优先，符合金融终端风格。

## 实现期验证项（审查 M4）

dock_style 的「每 leaf 单 tab」假设（`dock_style.rs:35-39` 注释）——审查已确认 egui_dock
0.20.1 的 tab 状态判定是 per-tab 的（focused > active > inactive），叠入 Chart leaf 后双 tab
视觉行为正确、dock_style 无需修改。但为客观验证（禁止目测 debug），实现期补一个**双 tab
leaf 的 egui_kittest 视觉断言**（参照 `dock_style.rs:176` 附近的形状测试先例）：断言激活 tab
与未激活 tab 的样式形状差异。

## 待确认（已确认 2026-08-02）

1. ~~**题材数据可用性**~~ → **已确认**：概念板块采集就绪后，行业列拼接 `行业 · 题材1 · 题材2` + 详情面板 Tag 区；采集未就绪期间行业列纯文本、详情面板省略题材区
2. **温度计五项指标口径**：具体指标名称与数值口径由数据侧决定（GUI 按通用渲染，零硬编码）
3. ~~**首次打开自动计算**~~ → **已确认**：不做自动计算，纯手动刷新（§5.4）
4. **资金/题材模块数据源**：资金流数据（主力净流入）与概念数据是否已就绪？若本期未就绪，相关模块按后端返回空子项处理（GUI 无感）
5. ~~**`[sepa]` 配置节**~~ → **已确认**：本期不做 top_n 持久化，重启默认 50
6. **TOP50 上限**：本期固定 50；如需 TOP100 仅改后端返回上限，GUI 无需变更

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 面板形态 | 独立 tab / 并入 screener | 独立 tab「东方SEPA」（`TabKind::Sepa`） | 查询型（条件选股）与报告型（每日排名）心智不同；合并使 screener 三层叠加，单一职责被破坏 | 并入会撑爆 screener 布局且混两种交互模型 |
| 初始 dock 位置 | Chart leaf 叠 tab / Screener leaf / 新建 leaf | 叠入 Chart leaf（顶部全宽） | 12 列表格 + 详情需 ~900px 宽；底部 leaf 仅 ~600px 宽 × 25% 高；egui_dock 同 leaf 多 tab 原生支持，用户可拖出；经审查验证 dock_style 7 态样式在双 tab leaf 下语义正确（per-tab 状态判定），无需修改 | Screener leaf 太窄；新建 leaf 改动 DockState 初始布局更多（约束要求尽量不动 DockArea 布局） |
| 详情展示 | 右侧详情面板 / 行内展开 / Modal | 右侧固定 ~300px 面板 | 不重构 DataTable 即达「点击行看明细」；50 行下展开条会被滚动挤出；股票排名工具通行形态 | 行内展开需给 DataTable 加大改（TableBuilder 无原生支持）；Modal 遮住表格上下文、反复开关成本高 |
| 温度计位置 | 面板顶部条 / 侧栏 | 顶部横向 Card 条 | 市场级信息恒显示；打开面板即见全局热度；不与详情面板抢横向空间 | 侧栏在滚动中会消失、与详情面板空间冲突 |
| TOP N 切换 | GUI 截断 / 后端重算 | GUI 截断（后端始终返回 TOP50，只作用于本地副本不回写） | 全市场评分秒级代价，切 30↔50 重算是浪费；50 行本地截断成本可忽略 | 后端重算每次切换延迟 + 后端无谓压力 |
| 分数色阶实现 | 扩展 `DataCell::Score{value,max,inverted}` + `Rank` 变体 + `score_color` / 复用 `DataCell::Price` / 纯文本 | 新增 Score（含 inverted）+ Rank 变体 + 纯函数 | Price 按符号走红涨绿跌，语义不符（高分应绿）；风险列需反向归一（0 扣分绿）需 inverted 标志；排名 1–3 强调需 Rank 变体（Count 无着色通道）；score_color 供表格/详情/温度计共用 | Price 语义错位；纯文本丢失色阶与数值排序；Count 无法着色 |
| 风险列显示 | 带符号扣分 `-x.x` 按幅度着色 / 正分反转 / 隐藏 | 带符号扣分 + `inverted` 反向色阶 | 扣分语义直白（「-3.2」即被扣 3.2 分）；0 扣分绿 → 5 扣分红的渐变直观 | 正分反转（显示 1.7）需脑内换算，反直觉 |
| 题材标签位置 | 行业列拼接 + 详情 Tag / 独立题材列 / 仅详情 | 行业列拼接（数据可用时）+ 详情面板 Tag | 12 列已达宽度上限，独立列挤爆；拼接用 `行业 · 题材1 · 题材2` 纯文本，零组件改动且排序正确（前缀优先） | 独立列过宽且题材数量不定；仅详情则排名列表缺题材线索 |
| 刷新/联动机制 | 复用现有通道与 `dispatch_row_fetch` / 新机制 | 复用（第三条 AsyncDispatcher 通道 + 提取 symbol 级核心函数） | 与 screener 通道零差异，实现面最小；联动行为与选股器一致（单击切图）；提取 `dispatch_symbol_fetch(state, signal, symbol)` 核心函数，screener 薄封装、SEPA 直调，避免泛型化重构 | 新机制增加接线与测试面，无收益；泛型化绑定 ScreenerRow 成本高 |
| 数据状态字段 | 单个 `sepa_data: Dynamic<Option<SepaData>>` / 分多字段 | 单字段 Option | rows 与 thermometer 同次返回，单字段避免半更新不一致；与 screener 分字段模式差异可接受 | 分字段需处理 thermometer 与 rows 不同步的中间态 |
| 策略侧契约 | 后端直接调用 `run_sepa` / GUI 自含计算 | 后端 handler 调用 `compass_strategy::run_sepa(query, reader, now) -> SepaData` | 计算逻辑属策略层，GUI 只消费结果；契约经 compass-types 闭合 | GUI 自含计算违反分层，与 14 子 issue 分解冲突 |
