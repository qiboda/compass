# UI 组件使用规范（统一设计语言）

**本文件是 compass GUI 组件使用规范的最终权威文档**，累积式维护——每次组件
规范变更（ui-designer 产出 + 主 agent 审阅 + 用户确认）后，将最终要点同步至此。

> **归档与权威的区别**：`.omo/designs/ui-widgets.md` 是 ui-designer 产出的
> **过程归档**（原始方案）；本文件才是组件使用规范的**最终版本**，与代码保持
> 同步。归档文件不删不改，但一切组件使用规范决策以本文件为准。

> **与 `kb/design/ui.md` 的分工**：`kb/design/ui.md` 覆盖设计 token、布局结构
> 与交互规范；本文件在**组件粒度**上补充使用规范（何时用、用哪个变体、怎么
> 组合、千万别怎么用），不改变任何既有设计系统决策。

> **同步记录（2026-08-09）**：仓库 pull 引入 ui-fixes PR（ref #217-#221），实质
> 修改 Button / PriceText / Tag / DataTable / MultiSelect 五个组件；本文已基于
> **pull 后代码**与 `kb/design/ui.md` 新增决策记录（ref #217-#221，含用户验收
> 6 项）同步更新受影响条目：Button 文字色统一 `text_primary` + loading 保留变体色、
> PriceText `percent_only()`、Tag 换行渲染 + `Sense::hover()`、DataTable numeric
> 右对齐 + 横向滚动 + percent_only 识别、MultiSelect id_salt（screener 三实例）。
> 之前的 6 条 ⚠️ 偏差标注不受影响，仍有效（其中偏差 #4/#5/#6 已转 issue
> #226/#227/#228 并于 2026-08-09 修复关闭，对应 ⚠️ 标注已从正文移除，历史记录
> 见文末「偏差跟踪」）。

---

## 目标

compass 需要一个**逐组件的使用规范**：24 个 compass-ui 组件各自
「什么时候用、用哪个变体、怎么组合、千万别怎么用」，让所有面板
（Chart / Logger / Screener / Sepa）与未来新增界面遵循同一套设计语言。

本文解决三个问题：

1. **选型**：同一需求有多个候选组件时（如 Badge↔Tag↔Label、Dropdown↔
   SearchableDropdown↔MultiSelect、Divider↔Card 边框），给出明确选择依据。
2. **一致性**：每个组件固定 8 字段模板（用途 / 适用场景 / 变体 / API 要点 /
   示例 / 反模式 / 相关组件 / 测试锚点），杜绝「同一语义多种画法」。
3. **边界**：明确原子 → 复合 → 业务三层的依赖方向与状态所有权，新增组件
   时有清晰的落位规则（compass-ui 零业务依赖是硬边界）。

与 `kb/design/ui.md`（设计 token + 布局 + 交互权威文档）的关系：本文在其
之上补充**组件粒度**的使用规范，不改变任何既有设计系统决策（token 六类、
compass_dark/compass_light 预设、egui-phosphor 图标、内置 toast/modal、
DockArea 布局均保持原样）。

---

## 现状

### 现有基础（均已实现，引用文件）

- **design token 系统**：`crates/compass-ui/src/tokens/`，六类——颜色
  `ColorTokens`（含 `ChartTokens` / `IndicatorTokens` 子结构，暗/亮两套）、
  间距 `SpacingTokens`、字号 `TypeTokens`、圆角 `RadiusTokens`、
  阴影 `ShadowTokens`、动效 `MotionTokens`；聚合体 `ThemeTokens` 提供
  `dark()` / `light()`。UI 代码不硬编码颜色值（ref #123）。
- **主题**：`crates/compass-ui/src/theme.rs` 的 `CompassTheme` 由 token
  直构 `egui::Visuals`（`apply_theme`）+ chart 薄封装（`apply_to_chart`），
  `crates/compass/src/theme.rs` 仅为 re-export（ref #126）。
- **字体**：SourceHanSansCN（中文）+ JetBrains Mono（数字等宽）+ egui-phosphor
  Regular 图标字体，全内嵌（`crates/compass-ui/src/fonts.rs`）。
- **组件库**：`crates/compass-ui/src/widgets.rs` 声明 24 个组件模块，每个
  组件以 `ThemeTokens` 为首参（builder 风格），compass-ui 零业务依赖。
- **权威设计文档**：`kb/design/ui.md` 覆盖 token、布局结构（工具栏 / Sidebar /
  DockArea / StatusBar / 浮层）、交互规范（快捷键 / toast / modal）、21 条决策记录。
- **设计归档**：`.omo/designs/gui-upgrade.md` §5 定义了三级组件分类
  （原子 16 + 复合 8 + 业务 citizen），§5.4 划定了与 egui 原生 widget 的边界。

### 24 个组件清单（widgets.rs 全量）

| 层 | 组件（文件名） |
|---|---|
| 原子（16） | `badge` `button` `card` `checkbox` `divider` `dropdown` `empty_state` `icon_button` `input` `label` `price_text` `section_title` `segmented` `status_dot` `tag` `tooltip` |
| 复合（8） | `data_table` `modal` `multi_select` `searchable_dropdown` `sidebar` `status_bar` `toast` `toolbar` |
| 业务（4，在 compass crate） | `citizens/chart.rs`（ChartCitizen）`citizens/logger.rs`（LoggerPanel）`citizens/screener.rs`（ScreenerPanel）`citizens/sepa.rs`（SepaPanel） |

> 业务层 4 个 citizen 不是 compass-ui 组件库成员，但作为「业务组件」层纳入
> 本文（详见 §组件使用规范·业务组件 与 §层级组织原则）。

---

## 组件使用规范

> **统一 8 字段模板**：用途 / 适用场景 / 变体 / API 要点 / 示例 / 反模式 /
> 相关组件 / 测试锚点。示例均从代码实际用法提炼（简化变量），反模式以
> 代码注释、测试断言、设计决策为依据；无明确依据时标注「未见约束」。

### 原子组件（16）

#### 1. Badge（`widgets/badge.rs`）

**用途**：数字计数角标——16px 高、min-width 16px 的全圆角 pill，只显示一个 `usize` 计数。

**适用场景**：需要强调「数量」的附着式角标（如未读/命中数）。与 Tag/Label 的选型见「相关组件」。当前业务代码（main.rs / citizens/*）尚无使用处——属于**待接入**的组件，新面板出现计数需求时优先用它，不要用 Tag 拼数字。

**变体**：`BadgeTone` — `Neutral`（默认，bg_panel_alt 底 + text_secondary 字）/ `Accent`（accent 底 + 白字）/ `Error`（error 底 + 白字）。选择规则：普通计数用 Neutral；强调主流程数量（如选中数）用 Accent；错误/风险计数（如失败数）用 Error。

**API 要点**：`new(tokens, count: usize)`；`.tone(BadgeTone)`；`colors() -> (bg, fg)`（供测试/预览）；`show(ui) -> Response`。

**示例**：
```rust
Badge::new(&tokens, 3).tone(BadgeTone::Accent).show(ui);
```

**反模式**：
- 用 Badge 显示非数字文本（如交易所代码）——那是 Tag 的职责；
- 给 Badge 加交互（点击）——Badge 是**纯展示**：无交互语义（无 `Sense`、无点击 API）、响应仅用于查询/测试；任何「可点击角标」需求先与设计确认，不要自行给 Badge 挂 `on_hover_text`/点击。

**相关组件**：Badge（数字计数 pill）↔ Tag（短文本标签 pill，20px 高）↔ Label（行内文本层级）。规则：**数字 → Badge；枚举/分类文本 → Tag；正文/说明文字 → Label**。

**测试锚点**：`widgets/badge.rs` `#[cfg(test)] mod tests`（L72）——`tone_colors_follow_design` / `count_is_queryable` / `single_digit_badge_meets_min_width_spec` / `badge_is_pure_display_without_click_semantics`。

#### 2. Button（`widgets/button.rs`）

**用途**：统一按钮——变体、尺寸、前导图标、loading、disabled 五要素，hover/press 态由 egui widget-state visuals 驱动（scoped style 注入 variant 色）。

**适用场景**：一切可点击的操作入口。选型：带文字的操作 → Button；纯图标操作 → IconButton；分段互斥选择 → Segmented；弹层内确认 → Modal 内嵌 Button。

**变体**：`ButtonVariant` — `Default`（bg_panel_alt 底 + border）/ `Primary`（accent 底 + **`text_primary` 字**，页面主操作）/ `Danger`（error 底 + **`text_primary` 字**，破坏性操作）/ `Ghost`（透明底 + border，次级/弹层内操作）。**全部变体文字色统一 `text_primary`**（主题感知：dark 浅灰 / light 深色，不硬编码白字——ref #217 验收「fetch 按钮文字颜色跟随主题」）。`ButtonSize` — `Sm`（24px）/ `Md`（32px，默认）/ `Lg`（40px）。选择规则：每屏**一个 Primary**（工具栏 Fetch、SEPA 刷新、筛选）；删除/移除类一律 Danger；Modal 内 Cancel 用 Ghost、Confirm 用 Primary/Danger；常规次要操作 Default。

**API 要点**：`new(tokens, text)`；`.variant(ButtonVariant)`；`.size(ButtonSize)`；`.icon(&str)`（phosphor 字形，渲染为 label 前缀）；`.loading(bool)`（内嵌右缘 spinner + 变暗 + `Sense::hover()` 忽略点击；**loading 保留变体文字色，由 spinner + 遮罩表达状态，不转灰**——ref #217 验收）；`.disabled(bool)`（禁用色 + 忽略点击；`disabled || loading` 合并判断；**仅真 disabled 将文字降为 `text_disabled`**）；`height() -> f32`；`show(ui) -> Response`（用 `.clicked()` 取点击）。

**示例**（源自 main.rs 工具栏 Fetch）：
```rust
if Button::new(&tokens, if loading { "加载中…" } else { "Fetch" })
    .variant(ButtonVariant::Primary)
    .size(ButtonSize::Lg)
    .icon(egui_phosphor::regular::DOWNLOAD_SIMPLE)
    .loading(loading)
    .show(ui)
    .clicked()
{
    // 触发 fetch
}
```

**反模式**：
- **loading 与 disabled 同时手动设**——loading 已内含禁用语义（测试 `loading_button_ignores_clicks`）；
- **loading 时仍响应点击**——`loading_button_ignores_clicks` 断言禁止；
- **loading 时期望文字变灰（`text_disabled`）**——loading 语义由 spinner + 遮罩表达，文字保留变体色（测试 `loading_button_keeps_variant_text_color` 断言 loading 渲染 `text_primary` 而非 `text_disabled`）；只有真 `disabled` 才降级文字色；
- 一屏多个 Primary 并列（稀释主操作层级）；
- 用 Button 替代 IconButton（图标+文字是 Button 的形态，纯图标请用 IconButton + tooltip）；
- 硬编码颜色——variant 色全部来自 token（`variant_colors()`），禁止自行传 `Color32`。

**相关组件**：IconButton（纯图标）、Modal（Confirm/Cancel 内嵌）、EmptyState（action 参数接收 Button）、MultiSelect（「完成」用 Primary+Sm）、Segmented（互斥选择替代）。

**测试锚点**：`widgets/button.rs` `mod tests`（L197）——`variant_colors_follow_design`（含各变体文字色 = `text_primary`）/ `sizes_map_to_control_tokens` / `primary_button_click_fires` / `loading_button_ignores_clicks` / `disabled_button_ignores_clicks` / `loading_button_keeps_variant_text_color` / `disabled_button_dims_label` / `icon_is_rendered_in_label`。

#### 3. Card（`widgets/card.rs`）

**用途**：面板容器——bg_panel 底 + radius_md + 1px border，可选标题行与可折叠正文。

**适用场景**：把表单/数据分组成独立区块（Screener 基础面/技术面两张卡、SEPA 温度计卡）；内容区需要统一底色的面板级容器。卡片之间用间距分隔（`spacing.md`~`lg`），不必额外加 Divider。

**变体**：`CardPadding` — `Md`（12px）/ `Lg`（16px，默认）。布尔开关：`bordered`（默认 true，false 去掉 1px border）、`collapsible`（默认 false，true 时标题行变折叠开关 + CARET_UP/DOWN）。

**API 要点**：`new(tokens)`；`.title(&str)`；`.padding(CardPadding)`；`.bordered(bool)`；`.collapsible(bool)`；`show(ui, |ui| { ... }) -> Response`（闭包返回正文渲染）。

**示例**（源自 sepa.rs 温度计）：
```rust
Card::new(&tokens).padding(CardPadding::Md).show(ui, |ui| {
    ui.horizontal(|ui| { /* 图标 + 标题 + score + Tag */ });
});
```

**反模式**：
- 卡片内再套整页级容器/重复边框（卡片本身就是面板，嵌套会叠 border）；
- 用 `collapsible` 承载高频切换的状态（那是 Segmented/Checkbox 的职责）；折叠状态存 egui memory（`ui.id().with("open")`），跨帧自持，调用方不要自行管理。

**相关组件**：Divider（卡片间的轻分隔，卡片自带边框时不需再用）；SectionTitle（卡内标题行；若卡片无 title 而面板头需要计数/操作，用 SectionTitle）。

**测试锚点**：`widgets/card.rs` `mod tests`（L132）——`title_and_body_render` / `collapsible_card_toggles_body`。

#### 4. Checkbox（`widgets/checkbox.rs`）

**用途**：复选开关——accent 勾选色、disabled 态，直接绑定 `&mut bool`。

**适用场景**：表单中的布尔选项（Screener 的「排除退市 / 均线 / 突破新高 / 动量 / 量能」）；MultiSelect 弹层内的行多选。

**变体**：无 enum 变体；`disabled(bool)` 开关。勾选态 stroke：accent 2px（hover 时 accent_hover），未勾选 text_weak 1px。

**API 要点**：`new(tokens, checked: &mut bool, label)`；`.disabled(bool)`；`show(ui) -> Response`（用 `.changed()` 判断本轮是否切换）。

**示例**（源自 screener.rs）：
```rust
Checkbox::new(&tokens, &mut self.form.exclude_delisted, "排除退市").show(ui);
```

**反模式**：
- 用 Checkbox 表达「互斥单选」——那是 Segmented/Dropdown 的职责；
- disabled 的 checkbox 仍可被点击——`disabled_checkbox_does_not_toggle` 断言禁止；
- 自己管理勾选状态（组件直接写 `&mut bool`，不要在外部另存副本再同步）。

**相关组件**：MultiSelect（内部组合 Checkbox）；Segmented（互斥单选对照）。

**测试锚点**：`widgets/checkbox.rs` `mod tests`（L71）——`click_toggles_checked` / `disabled_checkbox_does_not_toggle`。

#### 5. Divider（`widgets/divider.rs`）

**用途**：1px 分隔线，水平/垂直两向，regular/strong 两档。

**适用场景**：组间强分隔（Toolbar 内部自动使用）；同面板内语义区块的轻分隔（regular）；需要强调的分区边界（strong）。卡片已有边框时通常不再需要 Divider。

**变体**：`vertical(bool)`（默认 false 水平，true 垂直并撑满父级高度）；`strong(bool)`（默认 false → `border` 色，true → `border_strong` 色）。

**API 要点**：`new(tokens)`；`.vertical(bool)`；`.strong(bool)`；`stroke() -> Stroke`；`show(ui) -> Response`（`Sense::hover()`，不消费点击）。

**示例**（Toolbar 组间分隔为内置行为，业务侧直接使用）：
```rust
Divider::new(&tokens).vertical(true).strong(true).show(ui);
```

**反模式**：用多个 Divider 模拟卡片边框（边框属 Card）；在已有 `border_strong` 分界的 Dock/面板间重复加分隔线（未见约束，但会造成双线）。

**相关组件**：Card（容器边框）、Toolbar（内部自动插 strong vertical divider）。

**测试锚点**：`widgets/divider.rs` `mod tests`（L65）——`stroke_follows_strength` / `both_orientations_render`。

#### 6. Dropdown（`widgets/dropdown.rs`）

**用途**：单选下拉——统一 trigger（bg_panel_alt + border + CARET_DOWN）+ popup（bg_panel + shadow_popup + radius_md，28px 行）。可选内置搜索框。

**适用场景**：选项固定且较少（≤10 左右）的单选；工具栏主题切换（main.rs `Dropdown::new(&tokens, CompassTheme::all_names().to_vec())`）。**选项多、需要输入过滤/键盘导航 → SearchableDropdown；多选 → MultiSelect；互斥少选项（≤5）且需常显 → Segmented。**

**变体**：无 enum 变体；`searchable(bool)` 开关（popup 顶部加搜索输入框——**复用 `Input` 组件**，统一外观与 focus 描边约定——过滤 + 空过滤显示「无匹配结果」）。

**API 要点**：`new(tokens, options: impl IntoIterator<Item: Into<String>>)`；`.selected(usize)`（初始索引）；`.width(f32)`（默认 160）；`.searchable(bool)`；`show(ui) -> Option<usize>`（**选中变化时**返回新索引；popup 开合状态存 egui memory，组件无状态）。

**示例**（源自 main.rs 主题切换）：
```rust
if let Some(idx) = Dropdown::new(&tokens, CompassTheme::all_names().to_vec())
    .selected(theme_idx)
    .width(140.0)
    .show(ui)
{
    // idx 变化 → 切换主题
}
```

**反模式**：
- 把搜索框常驻 UI 而非弹层内（那是 Input 的职责；Dropdown 的搜索是 popup 内临时过滤）；
- 用 Dropdown 承载大量标的搜索——查询规范化（`sh.`/`sz.` 前缀）、键盘导航、空态都只在 SearchableDropdown 实现；
- 依赖 `show()` 的返回值判断「当前选中值」——它只报告**变化**，当前值需调用方自持（组件不拥有 state）。

**相关组件**：SearchableDropdown（可输入+键盘导航的下拉）、MultiSelect（多选下拉）、Segmented（互斥短选项）。

**测试锚点**：`widgets/dropdown.rs` `mod tests`（L194）——`initial_selection_is_first_option` / `clicking_option_changes_selection` / `searchable_filters_and_shows_empty_hint` / `searchable_popup_has_text_input` / `search_box_has_no_hardcoded_hint` / `searchable_typing_filters_options` / `empty_state_renders_when_no_match`。

#### 7. EmptyState（`widgets/empty_state.rs`）

**用途**：面板空态占位——居中 48px 图标（text_weak）+ heading 标题 + caption 描述 + 可选 action 按钮。

**适用场景**：面板无数据/未初始化时的引导（Chart 未 Fetch 时「输入代码并点击 Fetch」、Sidebar 空自选、SEPA 无评分数据、DataTable 空行）。**空态是组件内部默认行为的一部分**（DataTable 空行自动显示「无符合条件」）。

**变体**：无 enum 变体；`description(&str)` 与 `action(Button)` 均为可选开关（不设则不渲染）。

**API 要点**：`new(tokens, icon: &str, title: &str)`；`.description(&str)`；`.action(Button)`；`show(ui) -> Option<Response>`（有 action 时返回其 Response，供 `.clicked()`）。

**示例**（源自 chart.rs 空态）：
```rust
EmptyState::new(&tokens, egui_phosphor::regular::CHART_LINE, "暂无图表数据")
    .description("输入代码并点击 Fetch")
    .show(ui);
```

> ⚠️ 设计意图 vs 代码现状：见偏差清单 #3（原稿示例将标题/描述互换并编造了描述文案「数据来自本地数据源」——该文案不存在于代码库）。

**反模式**：
- 在非空数据上渲染 EmptyState（它是「无数据」专用）；
- 用 EmptyState 代替 toast 表达错误（错误反馈走 StatusBar/Toast，见 kb/design/ui.md 反馈状态表）；
- 给 EmptyState 传交互复杂的内容（action 只接收 Button，不是任意 UI）。

**相关组件**：DataTable（空行内部用 EmptyState）；SearchableDropdown/Dropdown 的「无匹配结果」是**轻量 hint 不是 EmptyState**（过滤场景不需要 48px 图标）；Sidebar 空自选复用 EmptyState。

**测试锚点**：`widgets/empty_state.rs` `mod tests`（L81）——`title_and_description_are_queryable` / `action_button_is_clickable`。

#### 8. IconButton（`widgets/icon_button.rs`）

**用途**：方形纯图标按钮——默认 32×32（`small()` 24×24），hover bg_hover / press bg_active，可挂 tooltip。返回 `bool` 点击信号。

**适用场景**：空间受限、图标语义自明的操作（工具栏侧栏开关、Sidebar 添加/删除 ×、Logger 导出）。kb/design/ui.md 图标约定：**图标 + 文字并用，除非空间受限**——IconButton 正是「空间受限」场景，因此**必须带 tooltip**。

**变体**：尺寸——默认 32px / `small()` 24px / `size(f32)` 自定义；`tooltip(Option<&str>)` 开关。

**API 要点**：`new(tokens, icon: &str)`；`.tooltip(&str)`；`.small()`；`.size(f32)`；`show(ui) -> bool`（点击即 true）。

**示例**（源自 main.rs 侧栏开关 + logger 导出）：
```rust
if IconButton::new(&tokens, egui_phosphor::regular::SIDEBAR_SIMPLE)
    .tooltip("切换侧边栏")
    .show(ui)
{
    self.sidebar_visible = !self.sidebar_visible;
}
```

**反模式**：
- 纯图标按钮不带 tooltip（图标语义不可靠，kb/design/ui.md 约定）；
- 用 IconButton 承载带文字的操作（那是 Button 的职责）；
- 用 `.size()` 做非标准尺寸（优先默认或 `small()`——默认尺寸须对齐 `control_md` token、`small()` 对齐 `control_sm` token，不得硬编码尺寸字面量）。

**相关组件**：Button（带文字操作）、SectionTitle（action 参数接收 IconButton）、Sidebar（行内 hover 删除 ×）、Tooltip（IconButton 内部用 `on_hover_text`；富文本提示用 Tooltip 组件）。

**测试锚点**：`widgets/icon_button.rs` `mod tests`（L81）——`icon_button_click_fires` / `small_size_uses_control_sm_token` / `default_side_follows_control_md_token` / `size_override_wins_over_default` / `tooltip_does_not_block_clicks`。

#### 9. Input（`widgets/input.rs`）

**用途**：文本输入框——统一外观（bg_panel_alt + border + radius_sm，focus 时 accent 1.5px 描边），可选前后缀图标与等宽字体。

**适用场景**：一切自由文本输入（Sidebar 搜索、Dropdown/SearchableDropdown/MultiSelect 的弹层搜索框）；代码/价格等需要等宽对齐的输入用 `monospace(true)`。

**变体**：无 enum 变体；开关：`placeholder` / `prefix_icon` / `suffix_icon` / `monospace(bool)` / `width(f32)`（默认 220）。

**API 要点**：`new(tokens, value: &mut String)`；`.placeholder(&str)`；`.prefix_icon(&str)`；`.suffix_icon(&str)`；`.monospace(bool)`；`.width(f32)`；`show(ui) -> Response`（用 `.changed()` 监听输入变化；focus 态自动画 accent 边框）。

**示例**（源自 sidebar.rs 搜索行）：
```rust
let search_resp = Input::new(tokens, search)
    .placeholder("搜索自选")
    .prefix_icon(ICON_SEARCH)
    .width(tokens.spacing.sidebar_w - 40.0)
    .show(ui);
if search_resp.changed() { /* 触发过滤 */ }
```

**反模式**：
- 用原生 `egui::TextEdit` 替代 Input（样式不统一，focus 边框/图标约定会丢失）；
- `monospace` 用于非代码/价格文本（中文字符在等宽字体下观感差）；
- 多行文本需求强行走 Input（组件只支持单行 `TextEdit::singleline`）。

**相关组件**：SearchableDropdown（内部用 Input 作搜索/展示框）、MultiSelect（弹层搜索用 Input）、Dropdown（弹层搜索框，设计意图应复用本组件）、Sidebar（搜索行组合 Input+IconButton）。

**测试锚点**：`widgets/input.rs` `mod tests`（L124）——`placeholder_is_rendered`（accesskit placeholder）/ `typing_updates_bound_value`。

#### 10. Label（`widgets/label.rs`）

**用途**：token 驱动的文本层级——level（色）× size（字号）二维组合，杜绝散落的 `RichText::new(...).color(...).size(...)`。

**适用场景**：纯展示文本且需要统一层级（面板说明、辅助文字、弱化信息）。业务代码目前多用 `ui.label(RichText::...)` 直写——新代码优先收敛到 Label，存量直写**不强制迁移**（未见约束，但收敛有助于一致性）。

**变体**：`LabelLevel` — `Primary`（默认，text_primary）/ `Secondary`（text_secondary）/ `Weak`（text_weak）/ `Disabled`（text_disabled）；`LabelSize` — `Body`（默认 12.5px）/ `Caption`（11px）/ `Heading`（14px）。

**API 要点**：`new(tokens, text)`；`.level(LabelLevel)`；`.size(LabelSize)`；`color() -> Color32`；`font_size() -> f32`；`show(ui) -> Response`。

**示例**：
```rust
Label::new(&tokens, "说明文字").level(LabelLevel::Secondary).size(LabelSize::Caption).show(ui);
```

**反模式**：
- 用 Label 表达涨跌色（那是 PriceText 的职责——涨跌色有专门语义 token）；
- 用 Label 做可点击文本（Label 无交互，点击请用 Button/IconButton）；
- 越级组合（如 Weak + Heading）制造非规范样式（未见约束，但违背 token 层级意图）。

**相关组件**：PriceText（数字+涨跌色，内部 mono）、Tag/Badge（胶囊形态）、SectionTitle（标题行，内部 heading 文本）。

**测试锚点**：`widgets/label.rs` `mod tests`（L95）——`levels_map_to_color_tokens` / `sizes_map_to_typography_tokens` / `label_text_is_queryable`。

#### 11. PriceText（`widgets/price_text.rs`）

**用途**：等宽价格 + 可选涨跌幅 + A 股红涨绿跌着色——`12.34 +1.23%` 格式（`percent_only()` 模式仅渲染 `+1.23%`，供涨跌幅列使用），全应用价格展示统一口径。

**适用场景**：任何价格/涨跌幅展示（StatusBar 摘要、DataTable 的 Price 单元格内部、SEPA 最新价列）。**A 股惯例：正涨红 `up`、负跌绿 `down`、0/flat 用主文本色**（kb/design/ui.md 涨跌色节）。

**变体**：`Tone` — `Auto`（默认，依 change 正负推导）/ `Up` / `Down` / `Flat`（强制指定，如无 change 数据需强制平色时）。

**API 要点**：`new(tokens, price: f32)`；`.change(f32)`（渲染 `+1.23%`，绝对值 <0.005 归一为 `0.00%`）；`.percent_only()`（**值即百分比**——`text()` 在 percent_only 且 change 存在时直接渲染 `format_change(change)`，供涨跌幅列使用，避免「2.50 +2.50%」双值重复——ref #217 验收）；`.tone(Tone)`；`color() -> Color32`；`text() -> String`；`show(ui) -> Response`。辅助函数：`auto_tone(Option<f32>) -> Tone`、`format_change(f32) -> String`。

**示例**（源自 status_bar.rs 内部）：
```rust
let mut price_text = PriceText::new(tokens, price);
if let Some(change) = change {
    price_text = price_text.change(change);
}
price_text.show(ui);
```

**反模式**：
- 手工 `format!("{:.2}")` + 自选颜色渲染价格（格式与涨跌色会漂移）；
- 用 PriceText 展示非价格数字（Score/Rank 用 DataCell::Score/Rank 或普通 label）；
- 涨跌方向与 A 股惯例相反（绿涨红跌是 TradingView 惯例，compass 已锁定红涨绿跌）。

**相关组件**：DataTable（`DataCell::Price` 内部复用；`price_cell_color()` 导出同口径色）、StatusBar（左段摘要）。

**测试锚点**：`widgets/price_text.rs` `mod tests`（L110）——`format_change_matches_design` / `auto_tone_follows_sign` / `auto_colors_use_up_down_tokens` / `rendered_text_contains_price_and_change` / `percent_only_renders_single_signed_percent` / `percent_only_colors_by_change_sign`。

#### 12. SectionTitle（`widgets/section_title.rs`）

**用途**：面板/区块标题行——heading 标题 + 可选 secondary 计数 + 可选右对齐 action（IconButton）。

**适用场景**：面板头部（Logger「日志」+ 导出按钮、Sidebar 分组标题「自选 3」、Screener 表单分区「行业/交易所/板块/上市时长/市值」）。与 Card 的选型：**Card.title 用于卡片自带标题；SectionTitle 用于非卡片面板头或卡内分区头**。

**变体**：无 enum 变体；`count(usize)` 与 `action(IconButton)` 可选开关。

**API 要点**：`new(tokens, text: &str)`；`.count(usize)`；`.action(IconButton)`；`show(ui) -> Option<bool>`（`Some(true)` = action 被点击；无 action 时为 `None`）。

**示例**（源自 logger.rs）：
```rust
let export = IconButton::new(tokens, egui_phosphor::regular::EXPORT).tooltip("导出日志");
let export_clicked = SectionTitle::new(tokens, "日志")
    .action(export)
    .show(ui)
    .unwrap_or(false);
```

**反模式**：
- 在 SectionTitle 的 action 位放带文字的 Button（该位设计为 IconButton，见 API）；
- 用 SectionTitle 替代 Card 容器（它是标题行，不提供面板底色/边框）。

**相关组件**：IconButton（action 位）、Card（容器）、Label（纯文本，无计数/操作）。

**测试锚点**：`widgets/section_title.rs` `mod tests`（L72）——`heading_and_count_render` / `action_is_clickable`。

#### 13. Segmented（`widgets/segmented.rs`）

**用途**：分段选择器——bg_panel_alt track + 等宽分段，选中段 bg_panel 底 + accent 文字 + border。

**适用场景**：互斥且选项少（≤5）、需常显的分组切换（周期 1d|1w|1M、SEPA TOP 50|TOP 30）。选项多/可折叠 → Dropdown 系；非常显的切换 → Dropdown。

**变体**：无 enum 变体；`selected(usize)`（初始索引）、`height(f32)`（默认 control_md 32px）。

**API 要点**：`new(tokens, options)`；`.selected(usize)`；`.height(f32)`；`show(ui) -> Option<usize>`（点击段返回其索引）。

**示例**（源自 main.rs 周期切换）：
```rust
if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
    .selected(self.timeframe_index)
    .show(ui)
{
    self.set_timeframe(idx);
}
```

**反模式**：
- 超过 5 个选项硬塞 Segmented（宽度失控，改用 Dropdown）；
- 用 Segmented 表达「多选」（那是 MultiSelect/Checkbox）；
- 选中态自己维护（返回索引需调用方自持状态并回传 `.selected()`，这是唯一的受控接口）。

**相关组件**：Dropdown（互斥但选项多）、MultiSelect（多选）、Checkbox（独立布尔）。

**测试锚点**：`widgets/segmented.rs` `mod tests`（L96）——`clicking_segment_reports_index` / `all_options_render`。

#### 14. StatusDot（`widgets/status_dot.rs`）

**用途**：8px 状态点——Idle/Success/Warning/Error 常亮，Loading 为 800ms 呼吸脉冲（accent 色，alpha 0.4→1 sine）。

**适用场景**：状态指示（StatusBar 中段、需要轻量状态位的行/表头）。与 Toast 的分工：**持续存在的状态 → StatusDot；一次性事件反馈 → Toast**。

**变体**：`DotState` — `Idle`（默认，text_weak）/ `Success`（success 绿）/ `Warning`（warning 琥珀）/ `Error`（error 红，常亮）/ `Loading`（accent 蓝，脉冲）。

**API 要点**：`new(tokens, state)`；`.size(f32)`（默认 8）；`color() -> Color32`；`show(ui) -> Response`。

**示例**（源自 status_bar.rs 内部，经 `StatusBar::dot_state` 映射）：
```rust
StatusDot::new(tokens, DotState::Loading).show(ui);
```

**反模式**：
- 用 Loading 态做「常驻装饰」（脉冲是「进行中」语义，完成后必须切走）；
- 用 StatusDot 承载文案（点只表达状态，文字另用 Label；StatusBar 中段是「点+文字」组合，不是点的责任）；
- 非 Loading 态依赖动画（测试 `only_loading_state_animates`：仅 Loading 动画，其余常亮）。

**相关组件**：StatusBar（中段组合 StatusDot+Label）、Toast（事件反馈对照）。

**测试锚点**：`widgets/status_dot.rs` `mod tests`（L88）——`state_colors_follow_design` / `only_loading_state_animates`。

#### 15. Tag（`widgets/tag.rs`）

**用途**：短文本标签 pill——20px 高、radius_pill、caption 字号；Exchange 变体按交易所自动配色（SH 蓝 #2962FF / SZ 绿 #0E9F6E / BJ 紫 #8B5CF6，白字）。

**适用场景**：枚举/分类短标签——交易所（Sidebar 行 SH/SZ）、板块/行业、SEPA 排名/仓位/题材、工具栏「前复权」说明标签。**与 Badge 的分工：文本 → Tag；数字计数 → Badge**。

**变体**：`TagVariant` — `Exchange`（按文本自动配色，未知代码回退蓝）/ `Board`（accent 默认色 tint 底）/ `Industry`（text_secondary 默认色 tint 底）/ `Custom`（默认 accent；可 `.color(Color32)` 覆盖，如 sepa 仓位按分数色阶着色）。tint 约定：底色 = 基色 18% alpha，文字 = 基色（`tint()` 导出供自建 chip 复用）。

**API 要点**：`new(tokens, text: &str)`；`.variant(TagVariant)`；`.color(Color32)`；`colors() -> (bg, fg)`；`show(ui) -> Response`。导出 `exchange_color(&str)`、`tint(Color32, f32)`。

**示例**（源自 sepa.rs 仓位 + sidebar.rs 交易所）：
```rust
Tag::new(&tokens, &t.position).variant(TagVariant::Custom).color(pos_color).show(ui);
// Exchange 变体：
Tag::new(tokens, &item.exchange).variant(TagVariant::Exchange).show(ui);
```

**反模式**：
- 用 Tag 展示长文本/正文（胶囊形态不适合，用 Label）；
- 用 Tag 展示数字计数（用 Badge）；
- 自定义变体绕开 token 直接硬编码色值（Custom 也走 `.color()` 但色值应来自 token 语义色，如 `tokens.color.info`）；
- 给 Tag 加交互（**无点击 API**——标签是纯展示；仅 `Sense::hover()` 用于响应/测试，不产生点击语义，不要给 Tag 挂 `.clicked()` 逻辑）。

**相关组件**：Badge（数字对照）、Label（长文本对照）、Sidebar（Exchange 标签）、DataTable/SEPA 面板（分类标签）。

**测试锚点**：`widgets/tag.rs` `mod tests`（L114）——`exchange_colors_follow_design` / `exchange_tag_uses_white_text` / `custom_tag_tints_base_color` / `tag_text_is_queryable` / `many_tags_wrap_within_container_width`（35+ 题材 Tag 在 `horizontal_wrapped` 内必须换行、不溢出容器——ref #217 验收，渲染用 `allocate_exact_size` + painter + `ui.put` 而非 `Frame::show`）。

#### 16. Tooltip（`widgets/tooltip.rs`）

**用途**：统一 hover 提示——包装 egui `on_hover_text` / `on_hover_ui`，默认延迟 0.4s（写回全局 `interaction.tooltip_delay` 作用域内临时生效）。

**适用场景**：纯文本提示（`.text()`）与富内容提示（`.show_ui()`，自定义 UI）。IconButton 的 `tooltip()` 参数即纯文本提示的便捷形式——**组件交互元素缺图标/缩写语义时必须有 tooltip**。

**变体**：无 enum 变体；`delay(f32)`（默认 0.4s，`delay_secs()` 读取）。

**API 要点**：`new(tokens)`；`.delay(f32)`；`.delay_secs() -> f32`；`.text(response, text) -> Response`；`.show_ui(response, |ui| ...) -> Response`；`.tokens()`。

**示例**：
```rust
let resp = ui.button("Hover me");
Tooltip::new(&tokens).text(resp, "帮助提示");
```

**反模式**：
- 对无 hover 语义的元素硬挂 Tooltip（如纯展示 Label——用户无「探索可交互」预期）；
- 用 Tooltip 承载关键信息（键盘快捷键、错误细节应写在可见区域；tooltip 延迟 0.4s 且不可发现，不是内容容器）；
- 直接改全局 `tooltip_delay` 不还原（组件内部已做 save/restore，外部不要再改）。

**相关组件**：IconButton（内部便捷 tooltip）、Dropdown/Segmented 等可交互组件（hover 说明）。

**测试锚点**：`widgets/tooltip.rs` `mod tests`（L60）——`default_delay_is_design_spec` / `delay_is_configurable` / `tooltip_renders`。

### 复合组件（8）

#### 17. DataTable（`widgets/data_table.rs`）

**用途**：可排序表格——表头点击排序（箭头 ↑/↓ 指示）、斑马纹、行 hover、`DataCell` 类型化渲染（Text/Price/Count/Score/Rank）、行点击返回原始索引、内置空态与计数、选中行高亮。排序语义迁移自 screener（`sort_rows` 纯函数）。

**适用场景**：多列结构化数据（Screener 结果、SEPA 12 列表格）。列类型化——价格列用 `Price`（红涨绿跌）、排名列用 `Rank`（1-3 warning 强调）、色阶列用 `Score`（`score_color` 四档）、计数列用 `Count`；**涨跌幅列（值即百分比）用 `Price` 且 `value == change`——`render_cell` 自动识别为 percent_only 模式，渲染单一 `+2.50%` 而非「2.50 +2.50%」**（ref #217 验收）。

**变体**：无 enum 变体；配置——`ColumnSpec{header, numeric}`（numeric 右对齐+等宽）、`set_sort(col, desc)` 初始排序、`set_descending_default(col, bool)` 业务默认降序（如市值列）、`set_selected(Option<usize>)` 详情联动高亮。

**API 要点**：`new(tokens, columns: Vec<ColumnSpec>)`；`.set_rows(Vec<Vec<DataCell>>)`（每帧重设）；`.set_selected(Option<usize>)`；`.set_tokens(ThemeTokens)`（主题切换，保留排序与行）；`.set_sort(usize, bool)`；`.set_descending_default(usize, bool)`；`sort_descending() -> bool`；`show(ui) -> Option<usize>`（点击行的**原始索引**）。导出 `sort_rows()`、`score_color()`、`price_cell_color()`。**内置行为**：表体与表头同布局——`numeric` 列右对齐 + mono（`render_cell` 按列 `numeric` 标志右对齐，ref #217 验收）；横向溢出由组件内置 `ScrollArea::horizontal`（`auto_shrink` false）吸收，调用方无需自包滚动区。

**示例**（源自 sepa.rs）：
```rust
let mut table = DataTable::new(tokens, COLUMNS.to_vec());
table.set_rows(rows);
if let Some(orig_idx) = table.show(ui) { /* 行点击联动详情 */ }
```

**反模式**：
- **同一列混用不同 `DataCell` 类型**——`compare_cells` 对异型返回 `Ordering::Equal` 保序，这是「调用方错误」的兜底而非功能（代码注释明确）;
- 在 `show()` 之外自行排序再传入（排序由组件拥有，外部只传原始顺序）；
- 自行包 `ScrollArea` / 手拼 `TableBuilder`——末列自动 `remainder()`、横向溢出由内置 `ScrollArea::horizontal`（`auto_shrink` false）吸收，都是组件内置行为（ref #217 验收），外部重复包滚动区会叠滚动条；
- 直接使用 `egui_extras::TableBuilder` 绕过 DataTable（样式与排序约定会漂移）。

**相关组件**：PriceText（Price 单元格内部）、EmptyState（空行自动显示）、score_color（SEPA 色阶，经 `DataCell::Score` 消费）。

**测试锚点**：`widgets/data_table.rs` `mod tests`（L373）——排序纯逻辑（`sort_rows` 文本/价格/计数/Score/Rank/平局/稳定序）、`price_cell_colors_follow_up_down_convention`、`toggle_sort_*`、`empty_table_shows_empty_state`、`header_renders_sort_arrow_for_sort_state`、`score_color_*`、`set_selected_*`（形状层断言 selection_bg）、`percent_column_renders_single_signed_form`（value==change 单百分比）、`numeric_cell_renders_right_aligned` / `text_cell_renders_left_aligned`（表体与表头对齐）。

#### 18. Modal（`widgets/modal.rs`）

**用途**：阻塞对话框——全屏半透明 backdrop（黑 60%，吞点击）+ 居中面板（min_width 360、radius_lg、shadow_modal），标题 + 正文 + 右对齐 Cancel(Ghost) / Confirm(Primary|Danger) 按钮，Esc 关闭。

**适用场景**：需要用户阻断确认/引导的场景（kb/design/ui.md 已锁定三个真实绑定：启动数据缺失引导、日志导出、移除自选确认）。**一次只允许一个 Modal 实例**（main.rs 单实例复用）。

**变体**：无 enum 变体；开关——`set_danger(bool)`（Confirm 变 Danger）、`set_confirm_text`/`set_cancel_text` 文案覆盖。

**API 要点**：`new(tokens)`；`is_open()`；`set_tokens(ThemeTokens)`；`open(now: f64)` / `close(now)` / `toggle(now)`（**now 为 egui 虚拟时间** `ctx.input(|i| i.time)`，动画确定性契约 ref #171）；`set_title` / `set_body` / `set_danger` / `set_confirm_text` / `set_cancel_text`；`set_on_confirm(FnOnce)`（**消费一次**）；进度访问器 `entry_progress/panel_progress/close_progress`；`show(&mut self, ctx)`（每帧调用，关闭状态机在 show 内完成）。

**示例**（源自 main.rs 移除自选确认）：
```rust
self.modal.set_title("移除自选");
self.modal.set_body(format!("确定要从自选中移除 {symbol} 吗？"));
self.modal.set_danger(true);
self.modal.set_confirm_text("移除");
self.modal.set_cancel_text("保留");
self.modal.set_on_confirm(|| { *confirmed.borrow_mut() = true; });
self.modal.open(now);
// 每帧末尾： self.modal.show(ui.ctx());
```

**反模式**：
- **on_confirm 回调里执行副作用并期望可重入**——回调消费一次，再次确认只关闭（测试 `confirm_button_consumes_callback_exactly_once`）;
- 期望 Tab 焦点被锁定——egui::Area 无原生焦点 trap（代码注释明确声明已知限制；要完整焦点锁定需 egui::Window modal 模式，代价是平台边框）;
- 关闭动画期间继续交互——closing 期间按钮 `disabled`（`interactive = !self.closing`）;
- 用墙钟 `Instant::now()` 驱动打开/关闭（虚拟时间契约，慢 CI 墙钟漂移是已修 bug 根因，ref #168/#171）。

**相关组件**：Button（Cancel Ghost / Confirm Primary|Danger）、Toast（非阻塞反馈对照：确认类用 Modal，纯通知用 Toast）。

**测试锚点**：`widgets/modal.rs` `mod tests`（L388）——状态机（`close_starts_closing_state_machine` / `open_resets_closing_state` / `toggle_flips_state`）、动画进度边界、`cancel_closes_modal_without_calling_callback` / `confirm_button_calls_callback_and_closes` / `confirm_button_consumes_callback_exactly_once` / `escape_starts_closing_animation` / `danger_modal_confirms_on_click` / `set_tokens_updates_theme_after_switch`。

#### 19. MultiSelect（`widgets/multi_select.rs`）

**用途**：多选下拉——summary trigger（「全部」/「已选 N 个」+ caret）+ 弹层（搜索 Input + Checkbox 行 + 右对齐「完成」Primary·Sm 按钮）。**组件拥有选择状态**（`options`/`selected`/`open`/`filter` 为 pub 字段）。

**适用场景**：从固定选项集中多选（Screener 行业/交易所/板块）。与 Checkbox 平铺的选型：**选项多或空间受限 → MultiSelect；选项少且常显 → 平铺 Checkbox**。

**变体**：无 enum 变体；`selected(...)` 预选、`id_salt(&str)`（同一 Ui 内多实例共存**必需**——空 salt 时多弹层 Area id 冲突互相覆盖，实测 ref #220；screener 行业/交易所/板块三实例均已显式设置）。

**API 要点**：`new(tokens, options)`；`.selected(iter)`；`.set_tokens(ThemeTokens)`；`.id_salt(&str)`；`summary() -> String`；`toggle(&str) -> bool`；`show(&mut self, ui) -> bool`（本轮选择是否变化）。

**示例**（源自 screener.rs）：
```rust
let mut ms_exchange = MultiSelect::new(tokens, ["SH", "SZ", "BJ"]);
// ...每帧：
if ms_exchange.show(ui) { /* 选项变化 → 触发筛选 */ }
```

**反模式**：
- 同 Ui 内多个 MultiSelect 不设 `id_salt`（弹层 `popup_id` 冲突，空 salt 时多弹层 Area 互相覆盖——ref #220 实测；screener 三实例显式设 `"screener_industry"` / `"screener_exchange"` / `"screener_board"` 为范本）;
- 外部直接改 `selected` 而不同步 `summary`（组件状态所有权在实例内，用 `.selected()`/`toggle()` 操作）;
- 期望点击选项行即关闭弹层（行为是「累积选择不关闭」，只有「完成」或 Esc 关闭——测试 `clicking_rows_accumulates_selection_and_confirm_closes` 断言）;
- 期望 kittest 能模拟 Area/ScrollArea 内点击（组件为此把 `popup_content` 拆出可测，测试直接调它）。

**相关组件**：Checkbox（行内）、Input（搜索）、Button（完成）、Dropdown（单选对照）、SearchableDropdown（单选+搜索对照）。

**测试锚点**：`widgets/multi_select.rs` `mod tests`（L193）——纯逻辑（`summary_*` / `toggle_adds_and_removes_options`）+ 交互（`clicking_rows_accumulates_selection_and_confirm_closes` / `trigger_opens_and_escape_closes_the_popup` / `search_filter_hides_non_matching_options` / `set_tokens_updates_theme_after_switch`）。

#### 20. SearchableDropdown（`widgets/searchable_dropdown.rs`）

**用途**：可搜索输入下拉（迁移自 StockPicker）——Input 展示/输入 + 过滤弹层 + ↑↓/Enter 键盘导航 + Esc 关闭 + 「无匹配结果」空态。**泛型于行类型**（`StockProjection<T>` 三个 fn 指针），compass-ui 保持零业务依赖。

**适用场景**：**标的搜索**（工具栏主输入，`交易所 | 代码 | 名称` 三格式显示，查询支持 `600519`/`SH600519`/`sh.600519` 三种拼写，D11 决策）。选项量大 + 需要输入过滤 + 键盘操作 → 本组件；简单单选 → Dropdown。

**变体**：无 enum 变体；`StockProjection<T>` 由业务侧注入（`symbol_of` / `name_of` / `exchange_of` 三个访问函数）。

**API 要点**：`new(tokens, default_symbol, projection)`；`.set_tokens(ThemeTokens)`；`show(&mut self, ui, stock_list: &[T]) -> egui::Response`；pub 状态字段 `filter_text` / `selected_symbol`（带交易所前缀的规范形，如 `"SZ000001"`）/ `selected_name` / `selected_exchange` / `popup_open` / `highlighted`。导出 `filter_stocks()`（纯过滤+交易所过滤+按 symbol 排序）、`StockPicker<T>` 兼容别名、`strip_exchange_prefix()`。

**示例**（源自 main.rs 工具栏，投影由业务侧构造）：
```rust
let projection = StockProjection::new(
    |s: &StockBasic| &s.symbol,
    |s: &StockBasic| &s.name,
    |s: &StockBasic| s.exchange.as_deref(),
);
let response = self.stock_picker.show(ui, &self.stock_list);
// 选中变化：读 picker.selected_symbol
```

**反模式**：
- 在 compass-ui 内依赖业务类型（组件靠 `StockProjection` 泛型隔离，测试用本地 `TestStock` 验证——**禁止**把 `compass-core`/`compass-types` 引入 compass-ui）;
- 期望多选（单选语义，选中即关弹层）;
- 期望非 ASCII 查询崩溃防护缺失——`strip_exchange_prefix` 对多字节前缀安全（回归测试 `strip_exchange_prefix_non_ascii_does_not_panic`），但调用方仍应只喂合法代码;
- 用 Dropdown 替代本组件做标的搜索（丢失三种拼写匹配与键盘导航）。

**相关组件**：Input（内部）、Dropdown（简单单选）、MultiSelect（多选）、Sidebar（`strip_exchange_prefix` 复用自本组件）。

**测试锚点**：`widgets/searchable_dropdown.rs` `mod tests`（L430）——`filter_stocks_*`（三种拼写/名称/交易所过滤）、`format_display_*`、`strip_exchange_prefix_non_ascii_does_not_panic`、交互（`test_show_click_opens_popup` / `test_escape_closes_popup` / `test_row_click_selects_stock`）、键盘（`arrow_down_moves_highlight_wrapping` / `arrow_up_moves_highlight_backwards` / `enter_selects_highlighted_row_and_closes` / `enter_without_highlight_does_nothing`）、`empty_filter_shows_no_match_hint`、`set_tokens_updates_theme_after_switch`。

#### 21. Sidebar（`widgets/sidebar.rs`）

**用途**：分组自选列表——搜索行（Input+IconButton 添加）+ 分组（SectionTitle + 行列表），行含名称 + mono 代码 + 交易所 Tag + hover 删除 ×；**纯 UI，交互以 `SidebarEvent` 返回**。

**适用场景**：左侧自选股面板（main.rs `SidePanel::left` 240px）。**组件不持数据**——行数据（`SidebarGroup`）与搜索文本由调用方每帧提供。

**变体**：无 enum 变体；`SidebarItem{symbol, name, exchange, selected}` 与 `SidebarGroup{title, items}` 为数据入参。

**API 要点**：`new(tokens)`；`show(&self, ui, groups: &[SidebarGroup], search: &mut String) -> Vec<SidebarEvent>`。`SidebarEvent`：`Select{symbol}` / `DeleteRequest{symbol}` / `Search(String)` / `Add`。

**示例**（源自 main.rs render_sidebar）：
```rust
let events = sidebar.show(ui, &groups, &mut self.sidebar_search);
for event in events {
    match event {
        SidebarEvent::Select { symbol } => self.fetch_symbol(&symbol),
        SidebarEvent::DeleteRequest { symbol } => self.request_watchlist_removal(now, &symbol),
        SidebarEvent::Add => self.add_to_watchlist(&current_symbol),
        SidebarEvent::Search(_) => {}
    }
}
```

**反模式**：
- 让 Sidebar 直接操作 SharedState/持久化（组件只发事件，增删/持久化是调用方职责——main.rs 处理 DeleteRequest 后打开 Modal 确认）;
- 行数据在 `show` 内部持有（每帧由调用方重建入参，组件无状态——跨帧持有会脏）;
- 删除不做确认直接删（删除请求 → 调用方 Modal 确认，这是已锁定的交互链）;
- hover 删除 × 与「点击删除」混淆（× 只在 hover 或选中行出现，且触发的是 `DeleteRequest` 而非直接删除）。

**相关组件**：Input（搜索）、IconButton（添加/删除）、SectionTitle（分组标题）、Tag（交易所）、EmptyState（空自选）、Modal（删除确认，业务侧组合）。

**测试锚点**：`widgets/sidebar.rs` `mod tests`（L232）——`renders_groups_names_codes_and_tags` / `empty_groups_show_empty_state` / `clicking_row_emits_select_event` / `hovering_row_reveals_delete_button_and_click_emits_delete_request` / `typing_in_search_emits_search_event`。

#### 22. StatusBar（`widgets/status_bar.rs`）

**用途**：26px 三段式状态条——左：标的摘要（symbol+name+PriceText）；中：状态（StatusDot+文字）；右：数据源 + mono 时钟。**纯 UI，所有数据经 `StatusBarData` 单帧传入**（时钟字符串由调用方格式化，本 crate 不依赖 chrono）。

**适用场景**：应用底部全局状态条（main.rs `TopBottomPanel::bottom`）。

**变体**：无 enum 变体；`StatusKind` — `Idle`（默认）/ `Loading` / `Error` / `Success`，经 `dot_state(kind)` 映射为 `DotState`。

**API 要点**：`new(tokens)`；`dot_state(StatusKind) -> DotState`；`show(&self, ui, data: &StatusBarData)`。`StatusBarData{summary: Option<StockSummary>, status: StatusKind, status_text: String, source: String, clock: String}`；`StockSummary{symbol, name, price: Option<f32>, change: Option<f32>}`。

**示例**（源自 main.rs render_status_bar）：
```rust
StatusBar::new(&tokens).show(ui, &StatusBarData {
    summary: Some(StockSummary { symbol, name, price, change }),
    status, status_text,
    source: format!("本地数据源 · {} 只", self.stock_list.len()),
    clock: self.status_clock.clone(),
});
```

**反模式**：
- 让 StatusBar 自行取数（数据必须经 `StatusBarData` 传入——组件不接触 SharedState）;
- 在 StatusBar 内做时间格式化（调用方格式化后传入，组件保持 chrono-free）;
- 用 StatusBar 承载一次性事件反馈（那是 Toast；StatusBar 是持续状态）。

**相关组件**：StatusDot（中段）、PriceText（左段）、Label（各段文本）、Toast（一次性反馈对照）。

**测试锚点**：`widgets/status_bar.rs` `mod tests`（L134）——`dot_state_maps_kinds` / `renders_three_segments` / `renders_without_summary`。

#### 23. Toast（`widgets/toast.rs`）

**用途**：右上角堆叠通知（16px 锚定、280px 宽卡片）——等级色条 + 图标 + 文案 + 关闭按钮 + 底部 3px 生命周期进度条；入口右滑 +16px/150ms cubic-out，关闭 alpha→0 + 高度→0 / 100ms；**队列上限 10，超限淘汰最旧**。`ToastManager` 持有全部状态。

**适用场景**：一次性操作反馈（添加自选成功、主题切换、日志导出结果、错误提示）。**与 Modal 分工：非阻塞通知 → Toast；需阻断确认 → Modal；持续状态 → StatusBar**。

**变体**：`ToastLevel` — `Info`（info 色 + INFO 图标，3s）/ `Success`（success 色 + CHECK_CIRCLE，3s）/ `Warning`（warning 色 + WARNING，3s）/ `Error`（error 色 + X_CIRCLE，**8s**）。

**API 要点**：`ToastManager::new(tokens)`；`.set_tokens(ThemeTokens)`；`.len()` / `.is_empty()`；`.push(level, msg)`；`.pop() -> Option<Toast>`；`.render(ctx)`（每帧调用）；`ToastLevel::color(&tokens)` / `icon()`。

**示例**（源自 main.rs）：
```rust
self.toast.push(ToastLevel::Success, format!("已添加 {symbol} 到自选"));
// 每帧末尾： self.toast.render(ui.ctx());
```

**反模式**：
- **在帧外/后台线程 push**——`push()` 用 `last_frame_time`（缓存的上帧 egui 虚拟时间）打戳，帧外 push 会打陈旧时间戳、缩短/扭曲寿命（代码注释明确 precondition）;
- 期待 toast 常驻（Error 也只有 8s；需持久反馈用 StatusBar/面板内提示）;
- 用墙钟驱动动画（虚拟时间契约 ref #168）;
- push 超过 10 条期望全部保留（淘汰最旧是设计行为，测试 `test_push_cap_at_10_evicts_oldest` 断言）。

**相关组件**：Modal（阻塞确认对照）、StatusBar（持续状态对照）、StatusDot（状态指示）。

**测试锚点**：`widgets/toast.rs` `mod tests`（L359）——manager 基础（FIFO/上限/等级时长）、`test_push_stamps_last_frame_time_as_created_at`、`level_colors_follow_color_tokens` / `level_icons_are_distinct`、动画边界（`entry_progress_boundaries` / `close_progress_boundaries`）、状态机（`close_is_idempotent` / `test_render_expired_toast_closes_then_is_removed` / `test_close_button_starts_closing_animation`）、`set_tokens_updates_theme_after_switch`。

#### 24. Toolbar（`widgets/toolbar.rs`）

**用途**：40px 分组工具栏容器——bg_panel_alt 底 + 底部 1px border；组间自动插 strong 垂直 Divider + 双侧 `spacing.lg`；组内 `item_spacing.x = spacing.sm`。

**适用场景**：应用顶部工具栏（main.rs 四组：标的 / 周期 / 操作 / 显示）。任何「一组一组的操作区」都可复用（SEPA 面板内工具条是水平排列的手工实现，若未来需要组间分隔可收敛到本组件——现状未见约束）。

**变体**：无 enum 变体；无配置项，纯容器。

**API 要点**：`new(tokens)`；`show(ui, |tb, ui| { ... }) -> R`；`group(ui, |ui| { ... }) -> R`（首个 group 无分隔线，后续 group 自动前置分隔）。`#[derive(Clone)]`——测试/复用可克隆。

**示例**（源自 main.rs render_toolbar）：
```rust
Toolbar::new(&tokens).show(ui, |tb, ui| {
    tb.group(ui, |ui| { /* 标的 SearchableDropdown */ });
    tb.group(ui, |ui| { /* 周期 Segmented + 前复权 Tag */ });
    tb.group(ui, |ui| { /* 操作 Button(Primary, Lg, loading) */ });
    tb.group(ui, |ui| { /* 显示 IconButton + Dropdown */ });
});
```

**反模式**：
- 在 Toolbar 内手工加 Divider 分隔组（`group()` 已自动处理，重复加会双线）;
- 用 Toolbar 做非顶栏容器（它是 40px 横向布局语义；面板内工具条用普通 `ui.horizontal`）;
- 组内用 `spacing.lg` 级别间距（组内是 `sm`，组间才是 `lg`——语义已内置，勿覆盖）。

**相关组件**：Divider（内部自动）、Button / IconButton / Segmented / Dropdown / SearchableDropdown / Tag（组内放置）。

**测试锚点**：`widgets/toolbar.rs` `mod tests`（L69）——`group_count_increments_per_group` / `show_renders_all_groups`。

### 业务组件（4，在 compass crate）

> 业务层是**面板**而非可复用组件：它们组合 compass-ui 组件 + SharedState/Signal
> 数据流（citizen 模式），留在 `crates/compass/src/citizens/`（gui-upgrade.md
> §5.3 + D11 决策）。此处只列组合关系与已锁定的使用模式，不做 8 字段模板
> （非组件库成员、不可复用、无独立变体）。

#### ChartCitizen（`citizens/chart.rs`）

- 组合：`EmptyState`（未加载引导「输入代码并点击 Fetch」）、egui-charts 图表（token 经 `apply_to_chart` 映射）、MA/BOLL 图例行（自绘 overlay，非组件）、工具栏「前复权」`Tag`（Custom + info 色，非交互）。
- 使用模式：空态是默认态；数据就绪后渲染图表；指标实时计算不存储（缓存指纹防碰撞）。

#### LoggerPanel（`citizens/logger.rs`）

- 组合：`SectionTitle`（标题「日志」+ `IconButton` EXPORT 导出按钮）。
- 使用模式：导出按钮触发保存文件对话框 → 写日志 → Toast 反馈（Modal 场景 2 的组件链）。

#### ScreenerPanel（`citizens/screener.rs`）

- 组合：`Card`×2（基础面/技术面分区）+ `SectionTitle`（行业/交易所/板块/上市时长/市值）+ `MultiSelect`×3（行业/交易所/板块）+ `Checkbox`×5（排除退市/均线/突破新高/动量/量能）+ `Dropdown`×2（上市时长、均线类型）+ `Button`（筛选，**Primary**——面板主操作，与工具栏 Fetch、SEPA 刷新同为「每屏一个 Primary」场景）+ `DataTable`（结果区）。
- 使用模式：查询型心智模型（用户填条件 → 筛选 → 看结果表）；结果表行点击复用 `dispatch_symbol_fetch` 联动图表。

> ⚠️ 设计意图 vs 代码现状：见偏差清单 #1（原稿将筛选按钮写成 Default——代码现状即 Primary，且与本文 Button 选择规则「每屏一个 Primary（…筛选）」矛盾）、#2（原稿 `Checkbox`×4，实际 5 个）。

#### SepaPanel（`citizens/sepa.rs`）

- 组合：`Card`（市场温度计：图标 + score 色阶色 + 仓位 `Tag`(Custom+score 色) + 5 指标 chip 自绘）+ `Segmented`（TOP 50/TOP 30，本地截断不回写）+ `Button`（刷新，Primary + loading + ARROW_CLOCKWISE，纯手动触发）+ `DataTable`（12 列，Score/Rank/Price 单元格）+ `EmptyState`（无评分数据）+ `Tag`（排名 #N、题材）。
- 使用模式：报告型心智模型（每日预计算排名，打开即读）；行点击联动详情面板（`set_selected` 高亮）与图表。

---

## 层级组织原则

### 分类依据

| 层 | 判定标准 | 成员 | 所在 crate |
|---|---|---|---|
| **原子** | 单一职责；不组合其他 compass 组件；直接消费 `ThemeTokens` | 16 个（见清单） | compass-ui |
| **复合** | 组合 ≥1 个原子形成交互单元；仍零业务依赖 | 8 个（DataTable/Modal/MultiSelect/SearchableDropdown/Sidebar/StatusBar/Toast/Toolbar） | compass-ui |
| **业务** | 组合复合/原子 + 业务数据状态（SharedState/Signal） | 4 个 citizen 面板 | compass（bin） |

分类沿袭 gui-upgrade.md §5 + 决策 D11（基础/复合/业务三级，理由：atoms 无业务
依赖、molecules 组合复用、organisms 留 bin 与 citizen 模式契合；排除平铺——
业务组件混入通用库会模糊复用边界）。

### 依赖规则（硬边界）

1. **方向单向**：原子 → tokens；复合 → 原子 + tokens；业务 → compass-ui 全部 + 业务数据层。
2. **compass-ui 零业务依赖**：禁止 `use compass_core::*` / `compass_types::*` 等
   （SearchableDropdown 用 `StockProjection<T>` 泛型 + fn 指针隔离，测试用本地
   `TestStock` 验证——这是模式范本，新增组件沿用）。
3. **新组件落位**：可复用、无业务依赖 → 进 compass-ui 原子或复合层，**必须带
   kittest 测试**（测试锚点字段即该门槛）；强依赖业务数据/状态 → 留 compass
   业务层（citizen 面板），不进组件库。

### 状态所有权规则

- **无状态 builder 原子**（Badge/Button/Card/Checkbox/Divider/Dropdown/
  EmptyState/IconButton/Input/Label/PriceText/SectionTitle/Segmented/StatusDot/
  Tag/Tooltip）：每帧 `new()` 构造，借用 `&ThemeTokens`；Dropdown 的弹层开合存
  egui memory（temp data），组件本身无状态。
- **有状态复合**（DataTable / MultiSelect / SearchableDropdown / ToastManager /
  Modal）：**构造时拷贝 `ThemeTokens`**（代码注释：让组件可越过创建帧存活），
  持有排序/选中/队列/开合等状态；主题切换用 `set_tokens()` 刷新而不重建
  （四者均有 `set_tokens_updates_theme_after_switch` 测试锚点）。
- **纯 UI 复合**（Sidebar / StatusBar / Toolbar）：无状态，数据每帧传入
  （`SidebarGroup` / `StatusBarData` / 闭包），交互以事件/返回值出（
  `SidebarEvent` / `Option<usize>` / `bool`）。

### 组合模式速查

- 弹层（popup）：一律 `bg_panel` + 1px border + `radius.md` + `shadow.popup`
  （Dropdown / MultiSelect / SearchableDropdown 弹层视觉完全一致）。
- 对话框：Modal 统一（backdrop 60% 黑 + `shadow.modal` + `radius.lg`），业务
  侧不另做确认框。
- 空态：面板级用 EmptyState；弹层过滤空结果用轻量「无匹配结果」hint
  （Dropdown / SearchableDropdown 内置）；DataTable 空行内置「无符合条件」。

---

## 偏差跟踪（设计意图 vs 代码现状）

> 本节记录「设计意图 vs 代码现状」审查（用户修正：规范应规定**应然**，
> 不得固化代码**实然**）发现的偏差条目——均已按设计意图改写文档并加 ⚠️ 标注。
> **偏差 #4/#5/#6 已转 issue（#226/#227/#228）并修复关闭（2026-08-09），
> 文档 ⚠️ 标注已移除，下文仅留历史记录。**

1. **偏差 #1（BUG）** — ScreenerPanel 组合：筛选按钮变体。文档原写 `Default`，
   代码现状即 `Primary`（screener.rs:247），且与本文 Button 选择规则「每屏一个
   Primary（工具栏 Fetch、SEPA 刷新、筛选）」矛盾。→ 文档已改，代码无需变更。
2. **偏差 #2（BUG）** — ScreenerPanel 组合：`Checkbox` 数量 ×4 实为 5 个
   （排除退市/均线/突破新高/动量/量能，screener.rs:389-444）。→ 纯文档笔误，已改。
3. **偏差 #3（BUG）** — EmptyState 示例编造：原示例 title/描述互换并虚构
   「数据来自本地数据源」（代码库无此文案）。实际/设计意图：title「暂无图表数据」
   + 描述「输入代码并点击 Fetch」（chart.rs:88-94）。→ 文档已重写。
4. **偏差 #4（权宜实现，已修复 #226）** — IconButton 默认尺寸硬编码 `32.0`
   （icon_button.rs:22），未走 `control_md` token（`small()` 才走 `control_sm`）；
   文档声称「与 control_sm/control_md token 对齐」仅部分成立。→ 已实现
   `tokens.spacing.control_md` 并同步测试断言（`default_side_follows_control_md_token`）。
5. **偏差 #5（缺失能力，已修复 #227）** — Badge 设计规格「min-width 16px」
   （gui-upgrade.md §5.1）未实现（badge.rs:52-69 无 `min_size`）；另文档
   「Sense::hover() 语义」表述与实现不符（组件未设任何 Sense）。→ 已实现 min-width
   16px（`ui.set_min_width(16.0)`），测试覆盖 `single_digit_badge_meets_min_width_spec`。
6. **偏差 #6（权宜实现，已修复 #228）** — Dropdown 弹层搜索框用原生 `TextEdit`
   （dropdown.rs:107），绕过 `Input` 组件的统一外观/focus 描边约定（Input 适用场景
   应为「一切自由文本输入」）。→ 已复用 `Input` 组件并移除硬编码 hint「搜索…」；
   测试覆盖 `search_box_has_no_hardcoded_hint` / `searchable_typing_filters_options`。

> 偏差 #1/#2/#3 为文档自身修正，无代码改动；#4/#5/#6 转 issue（#226/#227/#228）
> 并已修复关闭（2026-08-09 PR #229）。

---

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 文档位置 | `kb/design/ui-widgets.md`（直接写权威版）/ `.omo/designs/ui-widgets.md`（过程归档） | `.omo/designs/ui-widgets.md` 过程归档，确认后主 agent 同步 kb/ | 遵循项目「kb/ 是知识唯一数据源、.omo/designs 仅归档」既有约定（ui.md 决策记录第 2 条）；ui-designer 只写 `.omo/designs/` | 直接写 kb/ 违反 ui-designer 写限与归档/权威分离约定 |
| 组件模板 | 统一 8 字段模板 / 按组件类型灵活模板 / 表格化精简模板 | 统一 8 字段（用途/适用场景/变体/API 要点/示例/反模式/相关组件/测试锚点） | 任务已锁定；8 字段恰好覆盖「选型（2/7）+ 变体与用法（3/4/5）+ 约束（6）+ 验证（8）」完整闭环；统一模板保证 24 个组件可并排对比 | 灵活模板制造对比噪音；精简模板丢失反模式/测试锚点这两个最重要的校验字段 |
| 分层方式 | 原子(16)/复合(8)/业务(4 citizen) 三层 / 平铺 24 个 | 三层 | 沿袭 gui-upgrade.md §5 + D11 既有决策；三层与依赖方向（compass-ui 零业务依赖）天然对齐；业务层独立小节保持组件库纯 UI | 平铺会让业务面板混入通用组件库，复用边界模糊（D11 排除理由） |
| 反模式依据 | 仅代码注释/测试/设计决策有据条目 / 允许「未见约束」占位 | 有据条目为主，「未见约束」显式标注 | 反模式是本文最高价值字段，但 24 组件中部分组件（Badge/Label/Tooltip 等）确实缺少既有约束记录；显式标注避免「看起来像规范」实则编造 | 只写有据条目会留下空白组件；不标注的编造违反「从代码出发」原则 |
| 业务层处理 | 纳入 8 字段模板 / 仅组合关系说明 | 仅组合关系说明（4 面板） | citizen 面板不可复用、无独立变体与 API 面，模板字段大量空洞；组合关系 + 使用模式已覆盖设计语言目标 | 套模板制造虚假规范性（已确认：维持组合关系说明） |
| 状态模型记录 | 记录无状态/有状态/纯 UI 三类 / 不记录 | 记录三类状态所有权规则 | 主题切换（set_tokens）与每帧构造模式（builder）是 compass 组件区别于普通 egui 组件的关键使用约束，业务侧常踩坑 | 不记录则新使用者易误以为所有组件都可跨帧持有 |
| 示例来源 | 从业务代码实际用法提炼（简化变量）/ 凭空设计示例 | 从 main.rs / citizens/* / 组件自身测试提炼 | 保证示例与真实 API 完全一致、可编译 | 编造示例会产生「看起来可用」的假 API |

---

## 附：三层组件速查表（24 组件 × 一句话）

| 层 | 组件 | 一句话使用规则 |
|---|---|---|
| 原子 | Badge | 数字计数 pill；数字 → Badge、文本 → Tag |
| 原子 | Button | 带文字操作；一屏一个 Primary，删除用 Danger |
| 原子 | Card | 面板容器；表单/数据分区 |
| 原子 | Checkbox | 布尔复选；直接绑 `&mut bool` |
| 原子 | Divider | 1px 分隔线；组间 strong 垂直 |
| 原子 | Dropdown | 简单单选（≤10 项）；不拥有 state |
| 原子 | EmptyState | 面板无数据引导；DataTable 空行内置 |
| 原子 | IconButton | 纯图标操作；必须带 tooltip |
| 原子 | Input | 单行文本输入；focus accent 边框 |
| 原子 | Label | token 文本层级；涨跌色请用 PriceText |
| 原子 | PriceText | 等宽价格 + 红涨绿跌 + 涨跌幅 |
| 原子 | SectionTitle | 面板头；标题+计数+右对齐 action |
| 原子 | Segmented | 互斥少选项（≤5）常显切换 |
| 原子 | StatusDot | 8px 状态点；Loading 呼吸脉冲 |
| 原子 | Tag | 分类短标签；Exchange 自动配色 |
| 原子 | Tooltip | 统一 hover 提示；默认 0.4s 延迟 |
| 复合 | DataTable | 可排序表格；单元格类型化 |
| 复合 | Modal | 阻塞确认/引导；on_confirm 只消费一次 |
| 复合 | MultiSelect | 多选下拉；组件拥有选择状态 |
| 复合 | SearchableDropdown | 标的搜索；三种拼写匹配 + 键盘导航 |
| 复合 | Sidebar | 分组自选列表；交互以 SidebarEvent 返回 |
| 复合 | StatusBar | 三段式状态条；数据经 StatusBarData 传入 |
| 复合 | Toast | 一次性通知；队列上限 10 |
| 复合 | Toolbar | 分组工具栏容器；组间自动分隔 |
| 业务 | Chart/Logger/Screener/Sepa | citizen 面板；组合组件 + SharedState/Signal |
