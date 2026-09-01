# K 线图复权方式切换（前复权 / 后复权 / 不复权）

**状态**：过程归档（ui-designer 产出；用户已确认全部 4 项待确认问题，见 §待确认 → §已确认决策）
**日期**：2026-09-01
**关联**：Issue #345；worktree `adjust-mode`（分支 `feat/adjust-mode`）

---

## 目标

把 K 线主图表工具栏 Group B（周期 `1d | 1w | 1M` 旁）的**静态「前复权」Tag** 升级为
**三档 Dropdown**（前复权 / 后复权 / 不复权），满足：

1. 用户可在运行中切换复权方式，切换立即触发图表重载，与周期切换行为一致
2. 指数/板块（is_index）图表无复权概念：控件隐藏，指数永远 factor=1.0
3. 切换只影响 K 线显示；SEPA 面板「最新价」保持数据管线 close（不复权），不变
4. 复用现有 `Dropdown` 组件，不造新控件；全部视觉值来自 design token
5. `config` 新增 `default_adjust`（默认 `qfq`）；运行中切换不持久化（与 timeframe 行为一致）

---

## 现状（代码依据）

### 1) 工具栏 Group B：周期 Segmented + 静态前复权 Tag

`crates/compass/src/main.rs` `render_toolbar`（1381-1501），Group B 在 **1392-1411 行**：

```rust
// Group B — 周期: segmented 1d/1w/1M + 前复权 tag. The adjust tag
// is hidden when the current symbol is an index/board (指数不
// 复权, fqt=0 — plan T7); stocks keep it.
tb.group(ui, |ui| {
    if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
        .selected(self.timeframe_index)
        .show(ui)
    {
        self.set_timeframe(idx);
    }
    let current_symbol = self.shared_state.symbol.get();
    let is_index = self.is_index_or_board(&current_symbol);
    if !is_index {
        let adjust = t!("toolbar.adjust");
        Tag::new(&tokens, &adjust)
            .variant(TagVariant::Custom)
            .color(tokens.color.info)
            .show(ui);
    }
});
```

现状要点：

- **Tag**：`TagVariant::Custom` + `tokens.color.info`（#2962FF），
  `Tag::show`（`compass-ui/src/widgets/tag.rs:73-106`）按 caption 11px 文本 + 12×6 padding
  度量，非交互（`Sense::hover()`），约 20px 高、10px 圆角——是**状态徽标**而非控件。
- **隐藏守卫**：`is_index_or_board`（`main.rs:1195-1203`）——BK 前缀（`parse_explicit_prefix(symbol).0 == "BK"`）
  或列于 `index_list` 且 `index_type` 非空 ⇒ 视为指数/板块 ⇒ 隐藏 Tag。
- **组内间距**：`Toolbar::show` 设置 `item_spacing.x = tokens.spacing.sm`（8px）；
  组间自动插 strong 竖 Divider + 双侧 `spacing.lg`（16px）（`compass-ui/src/widgets/toolbar.rs:40-65`）。
- **周期切换范式**（复权切换的对齐目标）：`set_timeframe`（`main.rs:1105-1118`）——
  同步 `self.shared_state.timeframe.set(...)` 后**无条件** `fetch_bars()`（loading 守卫不拦截，
  注释：`a fetch already in flight belongs to the old timeframe`，最后请求胜出）。
- **启动派生**：`timeframe_index: timeframe_index_from_value(&config.app.app.default_timeframe)`
  （`main.rs:206`），与 `timeframe_value` / `timeframe_label` 双向同步。

### 2) Dropdown 组件（复用目标）

`crates/compass-ui/src/widgets/dropdown.rs`：

- **API**：`Dropdown::new(tokens, options: impl IntoIterator<Item: Into<String>>)`；
  `.selected(usize /* 初始索引，默认 0 */)`；`.width(f32 /* 默认 160 */)`；
  `.searchable(bool)`；`.id_salt(&str)`；`.show(ui) -> Option<usize>`（**仅选中变化时**返回
  新索引；弹层开合存 egui memory，组件无状态——调用方自持当前选中值）。
- **Trigger**（83-92 行）：`egui::Button`，文本 `"{label} {CARET_DOWN}"`（Phosphor 图标），
  `color(c.text_primary)`、`.size(tokens.typography.body)`（12.5px）、
  `.fill(c.bg_panel_alt)`、`.stroke(1.0, c.border)`、`.corner_radius(tokens.radius.sm)`（4px）、
  `min_size((width, tokens.spacing.control_md))`（**32px 高**——与 Group B Segmented 同高）。
- **Popup**（99-141 行）：`egui::Area` + `Order::Foreground`，`fixed_pos(trigger.left_bottom())`，
  **`constrain(true)`**（自动限制在视口内——无溢出问题）；Frame = `bg_panel` 填充 +
  1px `border` 描边 + `radius.md`（6px）+ `shadow.popup` + 4px 内边距；`set_min_width(self.width)`。
- **选项行**（152-203 行）：28px 高，`min_size((width-8, 28))`；hover 时组件内显式
  `style.visuals.widgets.hovered.weak_bg_fill = c.bg_hover` + `radius.sm`；选中行文本 `accent`
  色（#2962FF），未选中 `text_primary`；点击返回 index。searchable 时才渲染搜索框，
  空过滤显示 `common.no_matches`（caption / `text_weak`）。
- **无紧凑/自适应模式**：宽度仅由 `.width()` 手动指定（固定值，文本超宽时按钮按文本
  撑宽，不会截断）。
- **id_salt 是硬性要求**（规范见 `kb/design/ui-widgets.md` Dropdown 条目与 `dropdown.rs:17-19`）：
  同一 `Ui` 渲染多个 Dropdown 必须显式盐（popup_id = `ui.id() + "compass_dropdown_popup:{salt}"`，
  缺省会 Area id 冲突互相覆盖）。Group D 的 theme/lang 下拉分别用 `"theme"` / `"language"`。

### 3) 主题 token 实际值（`crates/compass-ui/src/tokens/`）

| token | 值（dark / light） | 用途 |
|---|---|---|
| `spacing.sm` | 8 / 8 | 组内间距（Toolbar 已设） |
| `spacing.md` | 12 / 12 | 组外间距；Button padding.x |
| `spacing.control_md` | 32 / 32 | Dropdown trigger 高（与 Segmented 同高） |
| `radius.sm` | 4 / 4 | trigger 圆角 |
| `radius.md` | 6 / 6 | popup 圆角 |
| `typography.body` | 12.5 / 12.5 | trigger 文本 |
| `color.bg_panel_alt` | #2A2E39 / #EDEFF2 | trigger 填充 |
| `color.bg_panel` | #1E222D / #FFFFFF | popup 填充 |
| `color.border` | #2A2E39 / #D6DAE0 | trigger/popup 描边 |
| `color.bg_hover` | #2A2E39 / #E8EBEF | 选项 hover |
| `color.text_primary` | #D1D4DC / #1B2430 | trigger 文本 / 未选中选项 |
| `color.accent` | #2962FF / #2962FF | 选中选项文本 |
| `color.info` | #2962FF / #2962FF | 现 Tag 色（将不再用于触发器） |
| `shadow.popup` | offset(0,4) blur 12 black 35% / 15% | 弹层阴影 |
| `motion` | fast 100ms / base 150ms / slow 300ms | 现有 Dropdown 无弹出动画（即时显示） |

### 4) i18n 现状

- `crates/compass-i18n/locales/zh.yml`：`toolbar.adjust: 前复权`；`en.yml`：`toolbar.adjust: Adj.`
- 键常量：`compass-i18n/src/lib.rs:45-46` `pub const KEY_TOOLBAR_ADJUST: &str = "toolbar.adjust";`
- 键完整性测试：`lib.rs` 底部 `ALL_KEYS` 列表（~L359 起）+ key-completeness 测试；
  `main.rs:3934` 有 zh/en 键值对照表测试（`("toolbar.adjust", "前复权", "Adj.")`）。
- 复权值本身（"qfq"/"hfq"/"none"）为内部枚举值，不本地化；仅显示文案本地化。

### 5) 数据流（切换生效路径）

- `FetchRequest`（`crates/compass/src/messages.rs:16-21`）：`symbol / timeframe / range_start / range_end`
  ——**尚无 adjust 字段**，实现需新增。
- `backend.rs:104`：`provider.fetch_bars(&req.symbol, &req.timeframe, req.range_start, req.range_end)`。
- `DataProvider::fetch_bars`（`crates/compass-core/src/data/provider.rs:58-66`）：当前固定
  **前复权**（`factor_i = adjclose_i / close_i`，文档注明"Bars are **forward-adjusted**"）。
- `dispatch_symbol_fetch`（`crates/compass/src/dispatcher.rs:98-107`）：SEPA/screener 行点击
  联动——`shared_state.symbol.set(...)` + 用**当前** `shared_state.timeframe` 发 FetchRequest，
  主图切换显示该股（不切 tab）。

### 6) SEPA 相关（关键事实，与预期不符处如实报告）

- **仓库中不存在独立的「SEPA 弹出图表」窗口**：全仓唯一 K 线渲染点是
  `ChartCitizen`（`crates/compass/src/citizens/chart.rs:20-139`，main.rs:121 创建，CHART_ID）。
- SEPA 面板（`citizens/sepa.rs`）是**表格 + 详情面板**（无图表、无弹出窗口）；
  表格「最新价」列来自 `SepaRow.latest_price`（`sepa.rs:458` `row_cells` → `row.latest_price`，
  表头键 `sepa.table.latest`）——**SEPA 评分数据管线的字段，与 K 线 fetch 无关**，
  天然保持 close 不复权，无需联动。
- SEPA 行点击 → `dispatch_symbol_fetch` → 主 Chart tab 显示该股 K 线。此联动图与主图
  是**同一 ChartCitizen、同一 shared_state.bars、同一次 fetch**——因此工具栏上的复权
  控件（全局状态）对该联动图**天然同时生效**，无需第二个控件。
- 结论：任务描述中「SEPA 弹出图表」应指 **SEPA 行点击联动的主图表**（见 §设计方案的
  联动机制说明；并列入「待确认」请主 agent 核实）。

### 7) 组件规范依据

- `kb/design/ui-widgets.md` Dropdown 条目：适用「选项固定且较少（≤10）的单选」；
  反模式第三条——**「不要依赖 `show()` 返回值判断当前选中值」**（只报变化，调用方自持）。
  Toolbar 条目：组内禁止 `spacing.lg` 级间距（组内是 `sm`）。
- `kb/design/ui.md`（权威文档，待本方案确认后同步）：「前复权 Tag（工具栏周期组内）：
  非交互 Tag（Custom + info 色）——本迭代无模式切换开关」→ 将被本方案取代。

---

## 设计方案

### 1. 控件形态：Dropdown 三档（复用现有组件，零改组件）

- 组件：`Dropdown::new(&tokens, [t!("toolbar.adjust.qfq"), t!("toolbar.adjust.hfq"), t!("toolbar.adjust.none")])`
  ——三档文案为运行时本地化字符串（与 theme dropdown 相同模式），**不用 searchable**（三选一无需过滤）。
- `id_salt("adjust")`：**必填**（Group D 已有 theme/lang 两个下拉，同 Ui 渲染必须盐化）。
- `.selected(self.adjust_index)`：App 自持有 `adjust_index: usize`（与 `timeframe_index` 对称，
  组件无状态，当前值由调用方维护——符合组件规范）。
- `.width(96.0)`（见 §3 宽度策略）。

**为什么 Dropdown 而非 Segmented**：需求锁定复用 Dropdown（决策 2）；且 Segmented 适合
「互斥短选项常显」，复权三档是属性设置类，与 Group D 的 theme/lang（同为 Dropdown）
在工具栏上形成「设置类控件用下拉」的语义一致性。若未来取消该约束，Segmented
（三档均宽）是视觉等价替代，但会与周期 Segmented 相邻造成两段式混淆——维持 Dropdown。

### 2. 放置位置与排列（Group B 内，零布局重构）

```
Group B: [ Segmented 1d|1w|1M ]  ← sm(8px) →  [ Dropdown 前复权 ▾ ]
```

- **紧跟周期 Segmented 之后**，位置与现 static Tag 完全一致（`main.rs:1395-1411` 段内原位替换
  `if !is_index { ... }` 分支内的 Tag 渲染）。
- **不换行**：Toolbar 是 `Layout::left_to_right` + 40px 高，Group B 总宽由
  ≈「(32×3+padding)+8+Tag≈60px」≈188px 变为 ≈224px（dropdown 96px），增量 +36px；窗口默认
  1440px（`WINDOW_INNER_SIZE`，main.rs:53），富余充足。
- **组内间距用默认 `item_spacing.x = 8px`（sm）**：与现状 Tag 间距一致，不手动加 spacing.lg
  （ui-widgets.md 明确禁止在组内用 lg）。
- Dropdown 32px 高与 Segmented 32px 同高 ⇒ **水平基线对齐**（现 Tag 20px 是「悬浮徽标」，
  替换成控件后整体保持同一高度带，视觉更整齐）。

### 3. 宽度 / 截断策略

- `.width(96.0)`：zh「前复权」= 3×12.5 = 37.5px 文本 + CARET_DOWN（~14px）+ Button
  padding.x（`spacing.md` 12×2=24）≈ 76px；96px 留 20px 余量。
- 固定宽度（而非内容自适应）理由：三档文案宽度不同（前复权=后复权=3字、不复权=3字；
  en 下 QFQ/HFQ/None 更短），固定值保证**切换不动布局**（Segmented 不因 dropdown 伸缩移位）。
- 不截断：egui Button 在文本超出 min_size 时按文本撑宽（`min_size` 是下限）——极端放大字体
  时按钮变宽而非截断，Group B 随后胀宽（可接受；egui 缩放是全局的，Group D 同样变宽）。
- popup 宽度 = trigger 宽度（`set_min_width(self.width)`），选项行 28px 高放 3 字中文
  （88px 宽）无压力。

### 4. 指数/板块场景（控件隐藏）

- 保留现有守卫：`if !is_index { Dropdown... }`（`is_index_or_board`，main.rs:1195-1203，
  与现状逻辑逐字不变；`crates/compass/tests/requirement_index_market.rs:95-103`
  `toolbar_adjust_tag_has_index_hide_guard` 测试保留并更新断言为「dropdown 分支仍有隐藏守卫」）。
- 指数永远 factor=1.0（数据层，本设计不涉及）；切换标的 index↔stock 时控件随
  `is_index` 每帧判断即时出现/消失（无动画，与现状一致）；用户此前在股票上选的复权档位
  在切回股票时**保持**（adjust_index 是会话内状态，不因切换标的重置——与 timeframe 一致）。

### 5. 交互：选择后立即生效

对齐 `set_timeframe`（main.rs:1105-1118）的新方法（实现层）：

```rust
fn set_adjust(&mut self, idx: usize) {
    self.adjust_index = idx;
    self.shared_state.adjust.set(adjust_value(idx));   // "qfq" / "hfq" / "none"
    self.fetch_bars();                                 // 无条件重载，最后请求胜出
}
```

- `shared_state.adjust`：`SharedState` 新增 `Dynamic<String>`（默认 `"qfq"`，
  见 `crates/compass/src/state.rs:14-15` timeframe 同款）。
- **无条件重载**（loading 守卫不拦截）：旧复权数据与下拉标签不一致时宁可重拉——与周期
  切换的注释与行为完全一致（main.rs:1108-1111）。
- **运行中不持久化**：与 timeframe 一致，无 `save_adjust_config`；重启回 config 默认。
- **加载反馈**：复用现有——Fetch 按钮 spinner（Group C）+ StatusBar 脉冲点；
  **不加 toast**（theme/lang 切换有 toast 因是全局设置；复权是图表视图属性，重载即反馈，
  与周期切换一致）。
- **SEPA/选股器行点击联动**：`dispatch_symbol_fetch`（dispatcher.rs:98-107）构造
  FetchRequest 时除 `timeframe` 外同时取出 `shared_state.adjust` 携带——
  保证「SEPA 行点击 → 主图显示」的图表使用**当前复权档位**（与主图同款同源）。

### 6. i18n 键设计

| 键 | zh | en | 建议常量 |
|---|---|---|---|
| `toolbar.adjust.qfq` | 前复权 | QFQ | `KEY_TOOLBAR_ADJUST_QFQ` |
| `toolbar.adjust.hfq` | 后复权 | HFQ | `KEY_TOOLBAR_ADJUST_HFQ` |
| `toolbar.adjust.none` | 不复权 | None | `KEY_TOOLBAR_ADJUST_NONE` |

- 复用 `toolbar.adjust` 前缀段（现键 `toolbar.adjust: 前复权` 改造为三段子键；
  旧键删除——**无外部引用**，全仓仅 main.rs:1405 与两处测试引用，测试同步更新）。
- en 用 **QFQ / HFQ / None**（A 股惯例缩写，东财同款）：显示带宽紧凑，
  且 en trigger「QFQ ▾」总宽 ≈60px，给 96px 宽度更大余量。
- 内部值域 `"qfq"/"hfq"/"none"` 不本地化；`adjust_value(idx)` / `adjust_index_from_value(s)`
  双向映射（仿 `timeframe_value` / `timeframe_index_from_value`，
  main.rs:206 附近），未知值（config 手写错）回退 index 0（qfq）。
- 键完整性：`compass-i18n/src/lib.rs` ALL_KEYS 增 3 常量；`main.rs:3934` 键值对照表同步。

---

## 视觉细节（全部 token 驱动）

| 元素 | 规格（dark 主题值；light 对应用括号） |
|---|---|
| **trigger（收起态）** | 高 32px（`spacing.control_md`）；fill `bg_panel_alt` #2A2E39（#EDEFF2）；stroke 1px `border` #2A2E39（#D6DAE0）；圆角 `radius.sm` 4px；文本 `{label} ⌄`（Phosphor `CARET_DOWN`），12.5px `typography.body`，`text_primary` #D1D4DC（#1B2430）；左对齐（egui Button 默认）；与 Segmented 同高、与 theme/lang 下拉同外观 → **同一层级控件的一致性** |
| **trigger hover** | 与既有 theme/lang 下拉**逐像素一致**（同一组件同一代码路径）。组件现状：trigger 显式 `fill`（egui 0.35 Button 静态 fill，hover 不换背景色）；hover 反馈 = egui 默认光标变化。**不做额外定制**（保持组件零改动）；如用户反馈需要 hover 底色，另开组件级 issue（`Dropdown` 加 hover 态），不属本 issue |
| **popup 展开态** | `Area` + `Order::Foreground`；位置 trigger 左下贴齐（`fixed_pos(rect.left_bottom())`）；`constrain(true)` 贴边自动收；面板 fill `bg_panel` #1E222D（#FFFFFF）；stroke 1px `border`；圆角 `radius.md` 6px；阴影 `shadow.popup`（offset (0,4) blur 12 black 35% / light 15%）；内边距 4px；宽度 = 96px |
| **选项行** | 28px 高 × 88px 宽；文本 12.5px body |
| **选项 hover** | 弱填充 `bg_hover` #2A2E39（#E8EBEF）+ 圆角 `radius.sm`（组件 `render_options` 已实现，dropdown.rs:165-166） |
| **选项选中态** | 文本 `accent` #2962FF（光/暗同值）；未选中 `text_primary`；无背景色区分（组件现状——选中靠文本色；与 theme dropdown 一致） |
| **动画** | **无**：弹出/收起即时（现有 Dropdown 无过渡动画，egui Area 直接显示）。不新增动画（保持组件零改动；如需未来统一加，属组件级 issue） |
| **关闭** | 点击组件外任意处（`pointer.any_click()` 且不在 popup/trigger 上）关闭；选中后自动关闭（dropdown.rs:145-147） |
| **无障碍色** | trigger 文本 #D1D4DC on #2A2E39：对比度约 7.0:1（WCAG AA/AAA 通过）；选中选项 #2962FF on #1E222D ≈ 4.3:1（AA 通过）——全走既有 tokens，无新增对比度风险 |

---

## 无障碍 / 键盘 / 边界情况

**键盘**（与现有 Dropdown 一致的现状，不扩范围）：
- 无自定义快捷键分配：`1/2/3` 已被周期占用、`/` 聚焦标的输入框——**不给复权分配数字键**；
  复权切换对重度用户是一步鼠标操作（trigger 点开 + 选项点选），与 theme/lang 一致。
- trigger/选项均为 egui `Button`：默认参与 Tab 焦点序 + Enter/Space 激活（egui 内建）；
  popup 打开后焦点不移入（组件现状，theme/lang 同——记为 Dropdown 组件级已知限制，
  统一键盘导航（↑↓/Enter/Esc）需组件级改造，**不在本 issue**）。
- AccessKit：Button 自带 labeled widget info（text 本地化字符串），屏幕阅读器可达。

**窗口窄**：Toolbar `left_to_right` 不换行（现状），Group B 增量 +36px 不构成新溢出源；
与 Group D 一样在极窄窗口下右侧被裁（应用级现状，非本 issue）。

**数据重载竞态**：与周期切换完全相同——无条件重载、最后请求胜出（dispatcher 置
loading=true 同步执行；无新增竞态面）。

**config 非法值**：`default_adjust` 手写错误值 → `adjust_index_from_value` 回退 0（qfq），
与 `timeframe_index_from_value` 行为对齐。

**SEPA 面板「最新价」**：不动（数据管线 close 值，与复权无关）。

**浮点数精度**：切换复权后重新 fetch，bars 全量重算——无「同一窗口内部分修正」问题
（ChartCitizen 缓存指纹 `(symbol, len, 首末 time, 末根 close)`，chart.rs:104-114，
close 位模式变化会触发指标重算，不会出现陈旧 MA/BOLL）。

---

## 预期的文件变更清单（实现层规划，供主 agent 参考）

| 文件 | 变更 |
|---|---|
| `crates/compass-core/src/model.rs` | `AppSection` 加 `#[serde(default = "default_adjust")] pub default_adjust: String`（355-371 区域）+ `default_adjust() -> "qfq"` |
| `crates/compass/src/state.rs` | `SharedState` 加 `adjust: Dynamic<String>`（默认 `"qfq"`；仿 timeframe，14-15/71-74 行） |
| `crates/compass/src/messages.rs` | `FetchRequest` 加 `adjust: String`（16-21 行） |
| `crates/compass/src/backend.rs` | handler 持 `req.adjust` 传入 `provider.fetch_bars`（~104 行） |
| `crates/compass-core/src/data/provider.rs` | `DataProvider::fetch_bars` 增加 adjust 参数（traits 58-66 行）；接口变更波及 `duckdb.rs` / `parquet.rs` / `synthetic.rs` 实现与调用点 |
| `crates/compass-core/src/data/duckdb.rs` | 三态复权：qfq=现行为（factor=adjclose/close）；hfq=反向累积因子；none=factor 1.0（数据层实现细节，UI 不感知） |
| `crates/compass/src/dispatcher.rs` | `dispatch_symbol_fetch` 构造 FetchRequest 时带 `shared_state.adjust`（98-107 行） |
| `crates/compass/src/main.rs` | Group B Tag→Dropdown（1395-1411）；新增 `set_adjust`（仿 set_timeframe 1105-1118）；`adjust_index_from_value`/`adjust_value` 辅助；App 字段 `adjust_index` 初始化自 config（仿 206 行）；i18n 对照表 3934 行；测试 `render_toolbar_renders_adjusted_price_tag`（2035-2046）改断言三档下拉 + 新增切换重载测试（仿 2048+ timeframe 切换测试） |
| `crates/compass-i18n/src/lib.rs` | `KEY_TOOLBAR_ADJUST_QFQ/HFQ/NONE`（45-46 区域）+ ALL_KEYS 完整性表（~359） |
| `crates/compass-i18n/locales/zh.yml` / `en.yml` | `toolbar.adjust` → `toolbar.adjust.qfq/hfq/none` 三段 |
| `crates/compass/tests/requirement_index_market.rs` | `toolbar_adjust_tag_has_index_hide_guard`（95-103）断言随 Tag→Dropdown 更新、隐藏守卫保留 |
| `.dsh/kb/design/ui.md` | 权威文档「前复权 Tag」两处表述（181-182、240-242 行）改「复权 Dropdown」+ 交互规范工具栏章节补一条 |
| `.dsh/kb/user/config.md` | 新增 `default_adjust` 配置项说明 |

---

## 待确认 → 已确认决策（2026-09-01 用户答复，全部选推荐项）

1. **「SEPA 弹出图表」= SEPA 行点击联动的主 Chart 图**（仓库核实无独立弹窗；同一
   ChartCitizen/同一 fetch，控件天然同时生效；SEPA 面板最新价保持 close）——已确认。
2. **en 文案选 QFQ / HFQ / None**（东财惯例缩写）——已确认。
3. **宽度固定 96px**（zh 三档 3 字均容纳，字体缩放留余量）——已确认。
4. **切换立即重载**（与周期切换一致，last request wins）——已确认。

（以下为原始待确认记录，保留过程痕迹）

## 待确认

1. **「SEPA 弹出图表」指什么？** 仓库勘察结论：不存在独立的 SEPA 弹出图表窗口；唯一
   K 线渲染点是主 Chart tab 的 `ChartCitizen`，SEPA 行点击经 `dispatch_symbol_fetch`
   联动主图。本方案按「SEPA 联动图 = 主图，控件天然同时生效」设计——请主 agent 核实
   该表述与 Issue #345 原始意图一致；若用户确指某个独立弹出窗口（本仓库未见），需
   补充该窗口的代码位置我再细化。
2. **en 文案**：推荐 `QFQ / HFQ / None`（东财惯例缩写，紧凑）；备选 `Adj. Fwd / Adj. Bwd / None`
   （更自解释但 trigger 变宽至 ~110px）。默认按推荐执行。
3. **默认宽度 96px**：zh 三档 3 字、en 缩写下均富余；若用户偏好更紧凑（80px），
   中文 3 字文本仍可容纳（≈76px 需求），选 96 是为缩放字体留余量。
4. **切换后立即重载 vs 仅标记下次 fetch**：本方案选**立即重载**（与周期一致）。
   备选「延迟到下次 Fetch」省流量但会造成图表标签与数据不一致——不推荐。

---

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 控件形态 | Segmented 三档 / Dropdown 三档 / 保持 Tag + 另设开关 | **Dropdown 三档（复用现有组件）** | 需求锁定复用 Dropdown（决策 2）；组件支持固定宽度 96px、32px 高与 Segmented 同高；与工具栏 theme/lang 下拉形成「设置类控件」一致性 | Segmented 与周期 Segmented 相邻易混（两段式）；Tag 非交互无法表达三态；另造控件违反约束 |
| 视觉样式 | trigger 用 info 蓝 tint（延续现 Tag） / 组件默认中性 | **组件默认中性（bg_panel_alt + border + text_primary）** | Dropdown 现无 trigger 颜色定制 API，改组件=扩大 API 面+违背零改动；下拉的展开柄已传递「可点击」语义，选中态由 popup 内 accent 文本表达 | 蓝色 trigger 会与 Button/Danger 等语义色混淆；「前复权」不再是状态徽标而是视图属性控件 |
| 放置位置 | 原位（Segmented 后）/ Group D 内 / 独立小组 | **原位（Group B 内 Segmented 之后，8px 间距）** | 与周期同为图表视图属性；现 Tag 同位置语义迁移最低成本；Toolbar 组结构零重构 | Group D 是「显示/全局设置」（主题语言），复权随行情数据，分离会割裂「图表属性」心智 |
| 隐藏策略 | 指数下置灰禁用 / 隐藏 | **隐藏（沿用现守卫，零改动）** | 指数无复权概念，禁用态仍需解释「为什么不可用」；隐藏即无误导 | 置灰需要禁用理由 tooltip + 组件禁用态，成本高且语义多余 |
| 切换行为 | 立即重载 / 仅标记下次 fetch | **立即重载（last request wins）** | 与 set_timeframe 完全一致（main.rs:1105-1118）；下拉标签与图表数据永不打架 | 延迟重载造成「选后复权但图还是前复权」的窗口期，且 SEPA 联动 fetch 会用到错误档位 |
| 持久化 | 写回 config / 不持久化 | **不持久化** | 与 timeframe 行为一致（锁定决策 5）；复权是会话内视图偏好，避免 config 膨胀与「重启后意外档位」 | 写回需 save_adjust_config + config 校验，与既有时长（timeframe）决策冲突 |
| 宽度 | 0（内容自适应）/ 96px / 更多 | **固定 96px** | 三档文案等宽（zh 3 字），固定宽度切换不动布局；缩放字体时按钮按文本撑宽不截断 | 内容自适应在 zh/en 间宽度跳动，波及 Segmented 位置；更大宽度与 Group B 密度不匹配 |
| 触发加载状态 | 加 Info toast / 复用 loading spinner | **复用 loading（无 toast）** | 与周期切换一致（无 toast）；重载即反馈 | toast 对高频视图属性切换是噪声 |
| adjust 状态存放 | App 字段独占 / SharedState Dynamic | **SharedState `Dynamic<String>`（默认 qfq）+ App `adjust_index`** | 与 timeframe 双轨一致（state.rs:14-15 取值 / App 持索引）；dispatcher 行点击联动需读值组 FetchRequest | 仅 App 持有则 SEPA/screener 联动无法带档位；仅 SharedState 则无法驱动 Dropdown 初始选中 |
| en 文案 | QFQ/HFQ/None / Adj. Fwd/Adj. Bwd/None | **QFQ / HFQ / None** | 东财惯例缩写，trigger ≈30px 宽；96px 宽度富余最大 | 自解释长文本使 trigger 超宽（~110px），Group B 占比失衡 |
