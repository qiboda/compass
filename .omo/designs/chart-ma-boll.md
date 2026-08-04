# MA 均线 + BOLL 布林带叠加层 + K 线前复权显示 — 视觉设计方案

> 归档：`.omo/designs/chart-ma-boll.md`（过程归档，非权威）
> 权威文档：`kb/design/ui.md`（经用户确认后由主 agent 同步最终要点）
> 日期：2026-08-04

## 目标

1. 在暗色（默认，TradingView 风格）/ 亮色两套主题下，为 K 线图叠加
   **MA(5/10/60/120/250) 五条均线 + BOLL(20, 2.0) 三线**，清晰可辨、符合
   A 股用户心智（同花顺/东方财富均线色系）、与现有 design token 系统一致。
2. 五条 MA 相邻周期颜色在 2px 实际线宽下肉眼可分，且抗 ~5px 重叠/粗线
   渲染仍可辨；暗色与亮色背景同时成立。
3. MA60/MA120/MA250 长周期线避免与涨跌红绿（up `#EF5350`/`#D93025`、
   down `#26A69A`/`#0E8F6E`）及现有主色 accent 蓝（`#2962FF`）冲突。
4. K 线价格按**前复权**显示（fetch 层缩放后写入 `Bar`，渲染层无感知），
   并提供明确的「前复权」视觉提示。
5. 保持简洁：本迭代无指标参数调节控件、无 MACD、无指标面板切换。

## 现状

### 渲染链路（vendored egui-charts，compass 分支）

- `~/.cargo/git/checkouts/egui-charts-a14ffbf1d5a8ad83/2b18acd/`：
  - `Indicator` trait（`src/studies/indicator_trait.rs`）：`calculate/values/colors/
    set_colors/is_overlay/line_cnt/line_names`，`IndicatorValue::{Single, Multiple, None}`。
  - `SMA`（`src/studies/builtin/sma.rs`）：`SMA::new(period)` + `with_color(color)`；
    默认色 `DESIGN_TOKENS.semantic.indicators.ma`（orange_500）。
  - `BollingerBands`（`src/studies/builtin/bollinger_bands.rs`）：`new(20, 2.0)`，
    输出 `Multiple([upper, mid, lower])`，无 builder 色 API —— 需 `set_colors`
    （传入 <3 色时三线同色）；默认 `bb_upper/bb_middle/bb_lower`（purple_500/
    gray_600/purple_500）。
  - `IndicatorRegistry`（`src/studies/mod.rs`）：`add/calculate_all/indicators()`。
  - 渲染 `show_with_indicators(ui, drawing_manager, Some(&registry))`
    （`src/widget/mod.rs:620`）：overlay 指标由 `IndicatorRenderer`
    （`src/chart/renderers/indicator.rs`）画在**蜡烛之上、OHLC legend 与十字
    准线之下**；线宽固定 `DESIGN_TOKENS.stroke.thick = 2.0px`，无
    per-indicator 线宽；**无 BOLL 通道填充渲染路径**（`bb_fill` token 存在但
    renderer 未使用）。
  - `show(ui)` = `show_internal(ui, None, None)`（`widget/mod.rs:892`）；
    全 overlay 指标时 pane 高度预留为 0，`show_with_indicators` 与 `show`
    布局完全一致。
  - OHLC legend（`src/chart/renderers/labels.rs:293 render_legend`）：左上角
    `symbol • timeframe O H L C ±涨跌`，锚定 `rect.min + (PADDING=40, y=12)`，
    **不包含任何指标值**；悬停 tooltip（`render_tooltip_with_options`）跟随光标
    绘制 OHLCV 详情。
  - `Chart.state`（`ChartState`）公开，`visible_range()` 可读可见窗口
    （`src/model/chartstate.rs:150`）。

### compass 侧

- `crates/compass/src/citizens/chart.rs`：`ChartCitizen::show` 每帧
  `app_theme.apply_to_chart(&mut chart)` → `chart.update_data(...)` →
  `chart.show(ui)`；空 bars 显示 EmptyState。
- `crates/compass-ui/src/theme.rs` `apply_to_chart`：只映射
  `ChartSemanticTokens`（蜡烛/网格/十字准线等），**不触碰 egui-charts 的
  `semantic.indicators`**——指标色必须由 compass 侧逐指标
  `with_color/set_colors` 覆盖。
- design token（`crates/compass-ui/src/tokens/color.rs`）：`ColorTokens`
  含 `chart: ChartTokens`（grid/crosshair/candle/volume），**无指标色 token**。
  涨跌色：dark up `#EF5350` / down `#26A69A`；light up `#D93025` / down
  `#0E8F6E`；accent `#2962FF`。
- 工具栏（`crates/compass/src/main.rs:884`）：Group A 标的 / Group B 周期
  Segmented(1d|1w|1M) / Group C Fetch / Group D 显示；`compass-ui::Tag`
  组件现成（`widgets/tag.rs`，Custom variant 支持自定义 tint 色）。
- 数据层：`stock_daily.parquet` 含 `adjclose`（前复权收盘，最新日
  adjclose == close），`fetch_bars_blocking`（`parquet.rs:112`）当前
  **不读取 adjclose**，K 线走原始 OHLC。
- 排版 token：caption 11px / mono 12px（JetBrains Mono）/ body 12.5px；
  间距 xs4 sm8 md12 lg16。

## 设计方案

### 1. 新增 `IndicatorTokens`（compass-ui `tokens/color.rs`）

在 `ColorTokens` 下新增子结构 `IndicatorTokens`（与 `ChartTokens` 并列），
两套主题各 8 个色值。**全部色值不硬编码到 UI 代码**，经
`app_theme.tokens().color.indicator.*` 取用。

```rust
pub struct IndicatorTokens {
    pub ma5: Color32,
    pub ma10: Color32,
    pub ma60: Color32,
    pub ma120: Color32,
    pub ma250: Color32,
    pub bb_upper: Color32,
    pub bb_middle: Color32,
    pub bb_lower: Color32,
}
```

#### MA 五线配色表

| token | 暗色值 | 亮色值 | 惯例依据 | 区分度论证 |
|---|---|---|---|---|
| `ma5` | `#D1D4DC`（= `text_primary`） | `#1B2430`（= `text_primary`） | A 股惯例 MA5 白线 | 中性极色，与一切彩色线对比最强 |
| `ma10` | `#F5A623`（= `warning`） | `#B57A00`（= `warning`） | A 股惯例 MA10 黄线 | 金黄连，H≈41° 高饱和高亮 |
| `ma60` | `#BA68C8`（Purple 300） | `#7B1FA2`（Purple 700） | 承接通达信 MA20 紫色槽位 | H≈291° 品红紫，与蜡烛红、accent 蓝均异色相 |
| `ma120` | `#00BCD4`（Cyan 500） | `#00838F`（Cyan 800） | 通达信 MA120 蓝青槽位 | H≈187° 纯青（B>G），与 down 绿（H≈172° G>B）异色 |
| `ma250` | `#A1887F`（Brown 300） | `#6D4C41`（Brown 700） | 年线惯例棕/灰 | 低饱和暖棕，与 MA10 金黄以色相+饱和度+明度三重区分 |

**相邻周期两两检查**（按图例顺序 MA5→MA10→MA60→MA120→MA250，2px 线宽、
抗 5px 重叠）：

- 白↔黄：饱和度差（0% vs 86%）、色相 0°↔41° —— 显著。
- 黄↔紫：色相 41°↔291°（互补）—— 显著。
- 紫↔青：色相 291°↔187°（互补）—— 显著。
- 青↔棕：色相 187°↔22° + 饱和度 100%↔33% —— 显著。
- 黄↔棕（跨行相邻，MA10 与 MA250）：19° 色相差 + 86%↔33% 饱和度差 +
  明度差（暗色 218↔142 luma）—— 实际可辨（金黄亮而棕褐暗且浊）。

**冲突检查**：五线均避开 up 红（`#EF5350`/`#D93025`）、down 绿
（`#26A69A`/`#0E8F6E`）、accent 蓝（`#2962FF`）——紫偏品红、青纯 cyan
（非绿非 accent 蓝）、棕低饱和；MA5/MA10 直接复用既有 `text_primary`/
`warning` token 值（与现有「error==up、info==accent」语义别名惯例一致，
不新增冗余色值）。

#### BOLL 三线配色表

| token | 暗色值 | 亮色值 | 说明 |
|---|---|---|---|
| `bb_upper` | `#90A4AE`（Blue Gray 300） | `#546E7A`（Blue Gray 600） | 上轨 |
| `bb_middle` | `#90A4AE` | `#546E7A` | 中轨（= MA20，**同色**） |
| `bb_lower` | `#90A4AE` | `#546E7A` | 下轨 |

**为什么不用 egui-charts 默认紫色**：默认 `bb_upper/bb_lower = purple_500
(#9C27B0)`，与 MA60 品红紫同族——两套线重叠时无法区分。改用**灰蓝
(slate)** 后：与五条 MA 全部异色相/异饱和度；与网格线
（暗 `#2D323C`/亮 `#E4E7EC`）明度差大（暗色 luma 159 vs 47）不混淆；
与十字准线（暗 `#64A0A0`/亮 `#3D7A7A` 青灰）以色相区分（slate 蓝灰 vs
青灰），且十字准线仅悬停时出现、线宽 1px，冲突风险可忽略。

**为什么三线同色**：用户心智中 BOLL 是一个整体指标（同花顺/东财均三线同
色），同色强化「一个指标、一条通道」的认知；中轨即 MA20 的语义由图例行
的 `BOLL 上/中/下` 标签承载，无需额外颜色。

### 2. 线宽与透明度

- **全部指标线：实线、2.0px、alpha 255（不透明）**。
  - 依据：vendored `IndicatorRenderer` 固定 `Stroke::new(stroke.thick=2.0,
    color)`，无 per-indicator 线宽；本迭代**不 patch fork**（保持简洁）。
  - 2px 实线 + 上表色相/饱和度/明度三重区分，保证「5px 线宽/重叠仍可辨」
    的目标成立。
  - 不做半透明：暗色背景下半透明彩色线发灰发浊，牺牲可读性；实线最稳。
- **BOLL 通道不填充**（决策详见 §2 决策记录）：
  - vendored overlay renderer **无填充渲染路径**（只画三根线，`bb_fill`
    token 存在但未被 `indicator.rs` 使用），填充需 fork 补丁。
  - 填充色块横跨价格区间会**遮挡 K 线主体**（OHLC 是本应用的一级数据）；
  - A 股主流软件（同花顺/东财）BOLL 默认三线无填充。
  - 结论：本迭代**线条方案**，三线之间留白；未来若做填充，建议
    `bb_fill` = slate 色 @ ~8-10% alpha（约 `(144,164,174,25)` 暗色 /
    `(84,110,122,20)` 亮色），并 patch vendored renderer 画上下轨间闭合
    多边形——记为后续工作。

### 3. 渲染接入（ChartCitizen）

`ChartCitizen` 增加字段：

```rust
registry: IndicatorRegistry,        // 惰性构建一次
cached_bars_key: Option<(usize, i64)>, // (bars.len(), 末根 time 秒) 缓存键
```

`show()` 流程（替换 `chart.show(ui)`）：

1. 从 `app_theme.tokens().color.indicator` 读取 8 色。
2. **仅当 bars 数据变化**（缓存键 = `bars.len()` + 末根 `time` 秒值；也可用
   egui-mobius `Dynamic` 订阅）才重建/重算：
   - `SMA::new(5).with_color(ma5)`、`SMA::new(10).with_color(ma10)`、
     `SMA::new(60).with_color(ma60)`、`SMA::new(120).with_color(ma120)`、
     `SMA::new(250).with_color(ma250)`；
   - `let mut bb = BollingerBands::new(20, 2.0); bb.set_colors(vec![bb_upper, bb_middle, bb_lower]);`
     （`set_colors` 需恰好 3 色，否则三线同色回落）。
   - `registry.calculate_all(&bars)` —— 实时计算、不存储（与项目「指标不
     持久化」约定一致）。
3. `self.chart.show_with_indicators(ui, None, Some(&registry))` —— 全 overlay
   时布局与 `show()` 完全一致（已验证 pane 高度预留为 0）。
4. bars 为空时跳过计算与图例（保留现有 EmptyState）。

主题切换：`app_theme` 每帧传入，色值天然随 `CompassTheme` 即时重映射（与
现有蜡烛/网格同机制），**无需额外接线**。

### 4. 图例方案（ChartCitizen 自绘 overlay）

- **位置**：图表左上**第二行**（vendored OHLC legend 正下方），同花顺风格：
  - x = `resp.rect.min.x + 40`（与 vendored legend 左缘 `PADDING=40` 对齐）；
  - y = `resp.rect.min.y + 30`（第一行 `+12` + 行高 ~16 + 行距 2）。
- **内容**（单行，左→右）：
  ```
  MA5 10.25   MA10 10.18   MA60 9.87   MA120 9.52   MA250 9.10   │   BOLL 10.44 / 10.25 / 10.06
  ```
  - 每个 MA 项 = 标签（caption 11px，`text_secondary`）+ 值（mono 12px
    JetBrains Mono，**该线色着色**）；
  - BOLL 项 = 标签 `BOLL` + 三值 `上 / 中 / 下`（同 slate 色，值着色；
    中轨值即 MA20）；
  - 项间距 `spacing.sm`（8px），MA 组与 BOLL 组之间以 1px `border_strong`
    竖分隔线 + `spacing.md` 间隔；
  - 数值格式复用 vendored `format_price` 规则：`≥100 → 2 位小数，≥1 → 4 位，
    <1 → 6 位`（与 OHLC legend 一致）；
  - 暖机期无值（如数据不足 250 根时 MA250）：显示 `—`（`text_weak`），
    渲染层由 renderer 自动断线。
- **数据源**：取**可见窗口最后一根 bar** 的指标值
  （`let (_, end) = self.chart.state.visible_range();` → 读
  `registry.indicators()[i].values()[end-1]`），与 vendored OHLC legend 显示
  last visible bar 的语义一致；滚动/缩放时随可见窗口联动。
- **样式**：整行一个圆角 chip 背景 —— `bg_panel_alt` @ 85% alpha +
  1px `border_strong` 描边 + `radius.sm` 圆角 + `padding` 4/6px。半透明背景
  保证压住下方蜡烛/指标线时文字可读，又不至于盖死图面（TradingView 图例
  同款遮罩思路）。
- **交互**：**纯静态展示**，不拦截鼠标（不消费点击/拖拽/滚轮，平移缩放
  十字准线不受影响）；数值**不跟随**悬停 bar（本迭代，见 §待确认）。

### 5. 叠放层级（自底向上）

| 层 | 内容 | 来源 |
|---|---|---|
| 1 | 网格/背景 | vendored |
| 2 | K 线蜡烛 + 成交量 | vendored `render_chart_type` |
| 3 | **MA/BOLL 指标线**（2px，画于蜡烛之上——标准行为） | vendored `IndicatorRenderer` |
| 4 | OHLC legend（左上第一行） | vendored `render_legend` |
| 5 | 十字准线 + 悬停 OHLCV tooltip（跟随光标） | vendored |
| 6 | **MA/BOLL 图例行（最后绘制，chip 背景遮罩经过的十字准线）** | ChartCitizen overlay |

- 十字准线/tooltip 画在指标线之上 → 悬停读数永不被 MA 线遮挡 ✓。
- 图例行带 chip 背景、画在最顶层 → 十字准线经过图例区时被遮罩（TV 同款
  行为），图例文字始终可读 ✓；tooltip 跟随光标，不落入左上静态图例区
  （光标贴左上边缘的极端情况除外，可接受）。
- 平移/缩放/点击命中测试不受影响：图例不消费输入事件。

### 6. 前复权显示与视觉提示

- **数据路径**（fetch 层，渲染层无感知）：`fetch_bars_blocking` 的 SQL 增加
  `adjclose` 列；取全量序列后按前复权锚定缩放：
  `scale_i = adjclose_latest / adjclose_i`（`stock_adj_factor` 因子同理），
  `open/high/low/close × scale_i` 后写入 `Bar`。最新日 scale=1，价格与现价
  一致。MA/BOLL 在缩放后的 adjusted 序列上实时计算（即基于前复权价）。
  （注：本迭代只存在前复权一种模式，无切换开关。）
- **视觉提示**：工具栏 **Group B（周期）** 内、Segmented 之后追加一个
  **非交互 `Tag` 组件**，标签「前复权」：
  - 用 `compass-ui::Tag`（`TagVariant::Custom`）+ `info` 色（accent tint，
    自动适配暗/亮）——现成组件，零新依赖；
  - 位置理由：复权状态属于「数据/周期模式」语义组，同花顺/东财均把复权
    状态放在图表工具栏；40px 工具栏容纳 24px Tag 无压力；
  - 与现有 tag 用法（SEPA 面板「仓位建议 Tag」）一致。
- 备选（见 §待确认 4）：状态栏右段「本地数据源 · N 只」后追加「· 前复权」。

### 7. 无障碍

- 对比度：全部 8 色在暗/亮背景上明度差 ≥ 60（BT.601 luma），文字/线条
  可读（§1 表内已附论证）。
- 键盘：无新增快捷键；图例与 Tag 均为非交互元素。
- 文字缩放：图例用 `caption`/`mono` token，随 egui 全局字体缩放联动。
- 色盲考量：MA 五线除色相外还有明度/饱和度差（白最亮、棕最暗），非全
  靠色相区分；BOLL slate 与 MA 无依赖关系。

## 交互效果

| 触发 | 表现 | 时长/缓动 | 目标态 |
|---|---|---|---|
| 主题切换（工具栏下拉） | 指标线色、图例文字色、前复权 Tag 随 `CompassTheme` 下帧即时重映射 | 无动画（与现有主题切换一致） | 新主题色 |
| 切换标的 / Fetch 加载完成 | bars 更新 → registry 重算 → 指标线与图例行随数据帧刷新 | 无动画（避免过度设计） | 新序列的 MA/BOLL |
| 平移 / 缩放 | 指标线随坐标联动（vendored 机制），图例值随可见窗口更新 | 即时 | 跟随可见窗口 |
| 悬停 K 线 | 十字准线 + OHLCV tooltip 画于指标线之上（vendored），图例不受影响 | 即时 | — |
| 图例行 hover（可选） | chip 背景由 `bg_panel_alt` 微提亮至 `bg_hover`，作为可感知反馈 | 120ms / ease-out | hover 态 |

> 本迭代刻意无动画：指标线跟随数据即时刷新即「正确性」的体现，过渡动画
> 属过度设计（对齐项目「保持简洁」约束）。

## 待确认

1. **MA 五线配色**「白/黄/紫/青/棕」是否接受？特别确认 MA250 用**棕褐**
   而非橙——橙（#FF9800 系）与 MA10 金黄（#F5A623）色相仅差 ~5°，2px
   线宽下几乎无法区分，故排除。
2. **BOLL 三线灰蓝（slate）** 是否接受？备选：统一一种彩色（如青色系，
   但与 MA120 青、down 绿竞争）；或保持 egui-charts 默认紫（会与 MA60
   品红紫冲突，不推荐）。
3. **图例行位置**：左上第二行（同花顺风格，推荐）vs 右上角（TradingView
   风格）？右上角与 OHLC legend 无语义关联，且靠近坐标轴标签。
4. **前复权提示**：工具栏 Tag（推荐）vs 状态栏文本？
5. **图例行背景 chip** 是否需要（推荐带 85% alpha chip）？纯文字无背景更
   简洁（TV 风格），但价格触及顶部时文字会压蜡烛且无遮罩。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| MA 五线配色序列 | A 股惯例 白/黄/紫/青/棕 / TradingView 蓝橙系 / 全灰阶单色 | 白/黄/紫/青/棕（暗亮两套按背景调明度） | 贴合 A 股用户心智（同花顺/东财均线色系）；五色相环均匀分布（含互补对），2px 下两两可分、抗 5px 重叠；全部避开涨跌红绿与 accent 蓝 | TV 蓝橙系与 accent 蓝冲突、违背 A 股直觉；全灰阶丧失周期区分度 |
| MA5/MA10 色值来源 | 复用 `text_primary`/`warning` token / 独立新色值 | 复用既有 token 值 | 与现有语义别名惯例一致（error==up、info==accent）；白色/黄色正是惯例所需，无需新造 | 独立色值制造冗余 token |
| MA250 颜色 | 棕褐 / 橙 / 灰 | 棕褐（`#A1887F`/`#6D4C41`） | 橙与 MA10 金黄 H 差仅 ~5° 不可分；棕与黄 19° 色相差 + 饱和度/明度三重区分，且年线惯例即棕/灰 | 橙与黄不可分；纯灰与网格/MA5 中性色易混 |
| MA60 紫色 vs BOLL 默认紫 | BOLL 改 slate 灰蓝 / MA60 改色保留 BOLL 默认紫 | BOLL 改 slate 灰蓝 | MA60 品红紫与 BOLL 默认 purple_500 同族不可分；灰蓝与五 MA 全部异色，且呼应「BOLL 是中性通道」语义 | MA60 改色会丢掉惯例紫色槽位；BOLL 保留紫则两指标打架 |
| BOLL 通道填充 | 不填充 / 半透明填充 / 填充+三线 | 不填充 | vendored overlay renderer 无填充路径（需 fork 补丁）；填充横跨价格区间遮挡 K 线主体；同花顺/东财默认无填充 | 半透明填充遮挡一级数据（OHLC）且增加 fork 维护面 |
| BOLL 三线配色 | 三线同色 slate / 上中下三色 | 三线同色 slate | 用户心智 BOLL 是一个整体指标（同花顺三线同色）；中轨=MA20 由图例标签承载 | 三色割裂「一个指标」认知，且需多占色相资源 |
| 指标线宽 | 保持 2.0px 统一 / per-indicator 线宽（patch fork） | 2.0px 统一 | vendored renderer 固定 `stroke.thick=2.0`；改线宽须 patch fork，超出本迭代范围；2px 实线配合配色已满足可辨目标 | patch fork 增加维护面与评审面，违反「保持简洁」 |
| 图例实现位置 | ChartCitizen 自绘 overlay / patch vendored `render_legend` | ChartCitizen 自绘 overlay | 零 vendored 变更、全部改动在 compass crate 内；不依赖图表库内部文本排版；图例数据源经公开 `chart.state.visible_range()` 可得 | patch vendored 触碰 fork 代码、与图表库 legend 布局强耦合 |
| 图例位置 | 左上第二行 / 右上角 | 左上第二行 | 同花顺/东财惯例（OHLC 行下方紧跟 MA 值行）；与 vendored OHLC legend 语义一致（均显示 last visible bar）；避开右上坐标轴标签/跳转按钮区 | 右上角与 OHLC legend 无关联、贴近坐标轴标签，且非 A 股惯例 |
| 图例数值跟随 | 静态 last visible bar / 跟随十字准线 bar | 静态 | 与 vendored OHLC legend 行为一致；跟随需读 chart 内部 hover 状态或 patch fork | 跟随 hover 引入图表内部耦合，本迭代不做（记为未来工作） |
| 前复权提示 | 工具栏 Tag / 状态栏文本 / 图表内标注 | 工具栏「周期」组 Tag | 复权属数据/周期模式语义组（同花顺/东财把复权状态放图表工具栏）；`Tag` 组件现成零依赖；非交互标签恰合「本迭代只有前复权一种模式」 | 状态栏距离图面远、弱提示；图表内标注需 patch vendored legend |
| 指标重算时机 | 每帧重算 / bars 变化时缓存重算 | bars 变化时缓存重算（键=len+末根 time） | 每帧 O(n) 分配浪费（千级 bars × 60fps）；数据不变则结果必然相同 | 每帧重算虽量级小但属工程浪费，缓存成本极低 |
| token 组织 | 新增 `IndicatorTokens` 子结构 / 扩展现有 `ChartTokens` | 新增 `IndicatorTokens` | 指标色与图表骨架色语义独立；`ChartTokens` 已被 `apply_to_chart` 逐字段消费，混入指标色职责不清 | 扩 ChartTokens 使 apply_to_chart 与指标色映射耦合 |

## 实现要点备忘（供实现 agent）

- `crates/compass-ui/src/tokens/color.rs`：新增 `IndicatorTokens` + `ColorTokens.indicator` 字段（dark/light），补测试断言两套 16 值。
- `crates/compass/src/citizens/chart.rs`：`ChartCitizen` 增 registry + 缓存键；
  `show()` 改调 `show_with_indicators`；图例行绘制在 `show_with_indicators`
  返回的 `response.rect` 上（`ui.painter()`，注意绘制顺序在最末）。
- `crates/compass-core`：fetch 层 `fetch_bars_blocking` 增 `adjclose` 读取 +
  前复权缩放（`scale_i = adjclose_latest / adjclose_i`）。
- `crates/compass/src/main.rs`：工具栏 Group B Segmented 后加
  `Tag::new(&tokens, "前复权").variant(TagVariant::Custom).color(tokens.color.info)`。
- 图例值取数用 `chart.state.visible_range()`（公开 API）；`end` 为开区间需
  在实现时确认（`end.saturating_sub(1)`）。
