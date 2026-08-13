# 可视化条件构建器 UI（Epic #243 Batch 2，issue #245）

> **归档文档**：本文件是 ui-designer 产出的**过程归档**，非权威。经用户确认后，
> 最终设计要点同步至 `kb/design/ui.md`（权威）与 `kb/user/gui.md`（用户手册选股器章节）。
> 本文件不删不改。

---

## 目标

用 **Metabase 范式的条件卡片组（AND/OR 嵌套）** 替换现有固定条件表单
`ConditionForm`（`crates/compass/src/citizens/screener.rs`）：

1. UI 操作的是 Batch 1 #244 落地的 **Filter AST**（`compass-types/src/screener.rs`），
   不是新造数据模型——构建器产出/消费 `Filter` 本身。
2. 卡片组支持 **AND/OR 分组与至少 2 层嵌套**，可添加/删除卡片、切换组内连接语义、
   清空条件。
3. 现有基础条件（行业/交易所/板块/上市时长/市值/排除退市）与技术条件
   （均线/突破/动量/量能）**全部可通过预置卡片表达，行为不回归**。
4. 可测试性：egui_kittest 无头测试能稳定定位卡片/按钮/分组（组件 id 与 label 约定）。
5. i18n：新增键走 compass-i18n 既有点分命名（`screener.builder.*`），zh/en 对称。

---

## 现状

| 项目 | 现状 | 位置 |
|---|---|---|
| 条件表单 | 硬编码 `ConditionForm`（11 字段）+ `build_query` → `ScreenerQuery`；两张 Card（基础条件/技术面条件），条件与控件同行原子组布局 | `crates/compass/src/citizens/screener.rs` L23-39 / L205-235 / L323-538 |
| 数据契约 | `RunScreenerRequest { query: ScreenerQuery }`；后端 `run_screener(&Filter::from(req.query.clone()), ...)` 编译后执行 | `crates/compass/src/messages.rs` L32；`crates/compass/src/backend.rs` L149-150 |
| Filter AST | `Filter`（Meta/Series/And/Or/Not，递归，serde JSON）、`MetaCond` 6 变体、`SeriesFactor` 6 因子、`SeriesCond` 4 变体、`CmpOp` 6 算子、`FactorRef`（Const/Factor）、`and/or/not` + `&\|~` 重载、`From<ScreenerQuery>` 单向编译 | `crates/compass-types/src/screener.rs`（全文件 824 行） |
| 引擎边界 | Batch 1 后端 `run_screener` 走**受限反向转换**：接受 From 可产出的形状（Meta 各变体、`Close>Gt>Sma(20\|60)`、`Close>Gt>NDayHigh(n)`、ChangePct 双边界 And、VolumeSurge、BullishAlign 双节点 And、And 组合）；**拒绝 Or 节点、Not 节点、UpDays/Count、`Delisted(true)` 等 → `ScreenerError::UnsupportedFilter`** | `crates/compass-strategy/src/lib.rs`（Todo 5，ref #244） |
| 组件库 | 24 个组件全可用：Card/Segmented/IconButton/EmptyState/Dropdown/MultiSelect/Checkbox/DragValue(egui)/Badge/Tag/Label | `crates/compass-ui/src/widgets/`；规范见 `kb/design/ui-widgets.md` |
| i18n | `screener.*` 键 33 个（含 `screener.ma_above20` 等）；键 = 点分小写 snake_case，zh/en 对称强制 | `crates/compass-i18n/locales/zh.yml` L86-118 |
| 持久化 | `on_save: Fn(&ScreenerQuery)` → `save_screener_config` 写 `[screener]` TOML 节；`restore: Option<&ScreenerQuery>` 启动恢复 | `crates/compass/src/main.rs` L104 / L403 |
| 测试 | kittest 用 `get_by_label` 查文案、`LANG_LOCK` 串行化 locale；GROUP_ALIGNMENT 系列测试锁定「标签+控件同行」原子组约束 | `screener.rs` tests L632-1110 |

---

## 设计方案

### 1. 数据模型：卡片视图模型 ↔ Filter AST（模板双向映射）

**核心原则**：卡片不是新数据模型，是 `Filter` 的**视图模型**。UI 状态树持有
`Filter` 的结构等价物，两方向各一个纯函数：

```
filter_to_items(&Filter) -> Vec<CondItem>   // 反向：AST → 卡片（渲染/restore）
leaf_to_filter(&CondLeaf) -> Filter          // 正向：卡片参数 → AST（构建/发送）
group_to_filter(&CondGroup) -> Filter
```

视图模型（compass crate 业务层，**不入 compass-ui**——compass-ui 零业务依赖
硬边界，Filter 属 compass-types）：

```rust
enum CondItem {
    Leaf(CondLeaf),
    Group(CondGroup),          // 递归 → 任意层嵌套
}

struct CondGroup {
    operator: BoolOp,          // And | Or（组头 Segmented 切换）
    items: Vec<CondItem>,
}

struct CondLeaf {
    kind: LeafKind,
    params: LeafParams,        // 按 kind 解释的参数区
    negated: bool,             // 「取反/排除」开关（待确认项 3）
}

enum LeafKind {
    Industry, Exchange, Board, ListYears, MarketCap, Delisted,   // Meta 6
    Ma, Breakout, Momentum, VolumeSurge,                         // Series 4（现状迁移）
    UpDays, Count,                                               // 序列函数（通达信风格）
}
```

### 2. 模板识别表（LeafKind ↔ AST 形状）

每张 Leaf 卡对应一个 **AST 子形状模板**，正反向互逆（round-trip 结构等价是
「现有功能不回归」的保证）：

| LeafKind | AST 形状（正向构建） | 参数控件 | 反向识别（AST → 卡） |
|---|---|---|---|
| Industry | `Meta(Industry(Vec<String>))` | MultiSelect | 同形状 |
| Exchange | `Meta(Exchange(Vec<String>))` | MultiSelect（SH/SZ/BJ 固定） | 同形状 |
| Board | `Meta(Board(Vec<String>))` | MultiSelect | 同形状 |
| ListYears | `Meta(ListYears(u32))` | Dropdown 不限/≥1/≥3/≥5 | 同形状 |
| MarketCap | `Meta(MarketCap{min,max})` | 两个 DragValue（亿） | 同形状 |
| Delisted | 勾选 → `Meta(Delisted(false))`；取消勾选 → **移除整卡** | Checkbox「排除退市」（默认勾） | `Meta(Delisted(false))` |
| Ma | 站上 MA20：`Cmp{Close,Gt,Factor(Sma(20))}`；站上 MA60：同上 60；多头排列：`And[Cmp{Sma5,Gt,Sma20}, Cmp{Sma20,Gt,Sma60}]` | kind Dropdown（站上 MA20 / 站上 MA60 / 多头排列） | 三种形状各归位 |
| Breakout | `Cmp{Close,Gt,Factor(NDayHigh(n))}` | DragValue n（默认 60，1-250） | 同形状 |
| Momentum | `And[Cmp{ChangePct(n),Ge,Const(min)}, Cmp{ChangePct(n),Le,Const(max)}]` | DragValue n / min% / max% | **成对识别**：And 内两个 Cmp 同 factor=ChangePct(n) 且 op 为 Ge+Le |
| VolumeSurge | `Series(VolumeSurge{days,times})` | DragValue days（1-80）/ times | 同形状 |
| UpDays | `Series(UpDays{n,min_pct})`（通达信 UPNDAY 风格） | DragValue n / min% | 同形状 |
| Count | `Series(Count{factor,op,value,window,at_least})`（通达信 COUNT 风格） | factor Dropdown + op Dropdown + value + window + at_least（待确认项 4） | 同形状 |

**组合节点识别规则**（`filter_to_items` 递归）：

- `And(vec)` / `Or(vec)` → `CondGroup{operator, items: 逐成员递归}`。
- **单成员 And/Or 折叠**：`And(vec![x])` → 直接折叠为 x 的卡片（避免 restore 时
  From 产出的单节点包裹产生多余子组；折叠不改变 AST 结构，round-trip 还原时
  `group_to_filter` 单成员仍产出 `And(vec![x])`——注意：折叠只影响**显示**，
  构建仍按模板产出原形状）。
- `Not(Box(x))` → 识别 x 的模板后置 `negated: true`（待确认项 3）。
- **无法识别的形状**（如 Batch 4 LLM 产出的自由 AST）→ 渲染为**只读摘要卡**：
  显示 `serde_json::to_string` 截断摘要（mono 弱化文本）+ 删除按钮，不可参数编辑。
  这是健壮性兜底（UI 必须能显示任意 `Filter`），不是新功能，成本约一行格式化。

### 3. 布局：条件卡片组结构

替换 `condition_form`（两张 Card）为**一个根组 Card**，自上而下：

```
┌─ Card「筛选条件」────────────────────────────────────────────┐
│  组头行: [Segmented 且(AND)|或(OR)]  Badge(条件数)  [清空] ──── │
│                                                              │
│  · Leaf 卡行（每张一行，水平包裹）:                            │
│    [类型 Dropdown ▾] [参数控件组……] [取反 ⇄] [删除 ×]          │
│  · 子组容器（Frame 轻量容器，左缩进）:                          │
│     组头行: [Segmented 且|或] [删除 ×]                        │
│     子卡片列表（递归，任意层）                                 │
│  · 空态（根组为空时）: EmptyState「暂无筛选条件」               │
│                                                              │
│  组底行: [＋ 添加条件 ▾]（Dropdown 选类型，含「子分组」项）      │
└──────────────────────────────────────────────────────────────┘
[筛选]（Primary，位置与现状一致）
```

- **根组**：`Card::new(tokens).title(t!("screener.builder.card_title")).padding(Md)`
  —— 替换现有两张卡。
- **子组容器**：**不用 Card-in-Card**（ui-widgets.md 反模式：嵌套叠边框）。
  用轻量容器：`Frame` 内边距 + `bg_panel_alt` 底 + 1px `border_strong` + `radius_sm`
  + 左缩进 `spacing.md`。层级靠边框与底色区分，任意深度递归渲染。
- **Leaf 卡行**：沿用现有**原子组**模式（`basic_group`/`technical_group` 的
  `scope_builder` 技巧）——类型 Dropdown + 参数控件 + 操作按钮作为**一个原子组**，
  窄窗口整组换行、组内不拆行（保留 ref #220 约束，GROUP_ALIGNMENT 测试迁移到新结构）。
- **垂直间距**：卡间 `spacing.sm`（8px）；组内列表 `spacing.xs` 级；与现有行距习惯一致。
- **宽度**：参数控件组用 `horizontal_wrapped`，最宽不超面板可用宽（现有 600px 最小
  支持约束不变）。

### 4. 组件复用清单

| 用途 | 组件 | 说明 |
|---|---|---|
| 根组容器 | `Card` | 标题「筛选条件」 |
| 子组容器 | `Frame`（egui 原生）+ token 色 | 见 §3，规避 Card 嵌套反模式 |
| AND/OR 切换 | `Segmented` | 组头，选项 `[且 (AND), 或 (OR)]`，`selected` 受控，`show → Option<usize>` |
| 条件类型选择 / 添加类型选择 | `Dropdown` | Leaf 卡头类型切换、组底添加菜单；**全部显式 `id_salt`**（ref #220 教训：同 Ui 多弹层冲突） |
| 参数多选（行业/交易所/板块） | `MultiSelect` | 显式 `id_salt`，选项每帧刷新（同现状） |
| 排除退市 | `Checkbox` | 勾选态=卡存在，取消=删卡 |
| 数值参数 | `egui::DragValue` | 同现状（N:/min%:/max%:/倍数:/窗口:/至少:） |
| 删除 / 清空 / 取反 | `IconButton` | 必须带 `tooltip`（kb/design/ui.md 图标约定）；`small()` |
| 条件数 | `Badge` | 组头「N」计数（数字计数 → Badge 而非 Tag，ui-widgets.md） |
| 空态 | `EmptyState` | 根组空时引导 |
| 只读摘要卡 | `Label`（mono, Weak） | 无法识别的 AST 形状 |
| 运行中/错误 | 现状 spinner / colored_label / toast | 不动 |

**无新增 compass-ui 组件需求**——现有 24 个组件够用；构建器整体是 compass crate
的业务层模块（`citizens/screener.rs` 内重构或同目录新文件，实现 agent 定）。

### 5. 交互设计

| 操作 | 交互 | 说明 |
|---|---|---|
| 添加条件 | 组底「＋ 添加条件」→ Dropdown 选类型 → **组尾插入该类型默认参数 Leaf 卡**，参数就地编辑 | Metabase 向导式（选类型→填参数→确认）成本高且引入模态状态；**就地常驻编辑**更直接、kittest 更好测（待确认项 6） |
| 添加子组 | 添加菜单选「子分组」→ 插入空子组（默认 AND）→ 子组内继续添加（可再嵌套） | 嵌套深度不限（验收 ≥2 层）；**不做拖拽移动**（egui 拖拽成本高，过度设计） |
| 编辑参数 | 参数控件常驻卡上，改动即时写入卡片模型（无「确认」按钮） | 筛选时统一构建 AST |
| 切换类型 | 点 Leaf 卡头 Dropdown → 类型切换 → 参数区重置为该类型默认值 | |
| 删除单卡 | Leaf 卡头 ×（tooltip「删除条件」）→ 直接移除，**无确认** | Metabase 惯例；条件可轻易重建 |
| AND-OR 切换 | 组头 Segmented → 仅改组类型，卡片内容不变 | 切换立即反映到 AST 构建 |
| 取反 | Leaf 卡头「取反」开关 → `Not` 包裹（待确认项 3） | |
| 清空 | 根组头「清空」IconButton → 根组 items 清空 → 空态引导；**无确认 Modal** | 清空非破坏性（可重建）；验收测试项 |
| 空态 | 根组空 → EmptyState「暂无筛选条件 / 点击添加条件，或直接筛选查看全市场」；子组空 → 弱化 Label「空分组」+ 添加按钮 | |
| 运行 | 「筛选」按钮逻辑、loading/error、结果表格**全部不动**；错误文案对 UnsupportedFilter 额外加一句说明（键 `screener.builder.unsupported_run`） | 见 §7 引擎边界 |

### 6. i18n 键组织（`screener.builder.*` 前缀）

新增键（zh/en 对称，`crates/compass-i18n/locales/{zh,en}.yml`）：

```yaml
screener:
  builder:
    card_title: 筛选条件
    add_condition: 添加条件        # 组底按钮 + Dropdown trigger
    add_group: 子分组              # 添加菜单项
    group_and: 且 (AND)
    group_or: 或 (OR)
    empty_title: 暂无筛选条件
    empty_desc: 点击「添加条件」构建筛选，或直接筛选查看全市场
    empty_group: 空分组
    clear_tooltip: 清空条件
    delete_tooltip: 删除条件
    negate_tooltip: 取反（排除）
    unknown_shape: 高级条件
    unsupported_run: "该条件类型引擎暂不支持，将在后续版本可用: %{e}"
    cond_industry: 行业
    cond_exchange: 交易所
    cond_board: 板块
    cond_list_years: 上市时长
    cond_market_cap: 市值(亿)
    cond_delisted: 排除退市
    cond_ma: 均线
    cond_ma_above20: 站上 MA20
    cond_ma_above60: 站上 MA60
    cond_ma_bullish: 多头排列 MA5>MA20>MA60
    cond_breakout: 突破新高
    cond_momentum: 动量
    cond_volume_surge: 放量
    cond_up_days: 连续上涨
    cond_count: N 日内满足
    param_window: "窗口:"
    param_at_least: "至少:"
    param_value: "值:"
```

- **复用现有键**：`screener.industry/exchange/board/list_years/market_cap/
  exclude_delisted/ma/breakout/momentum/volume/n_label/min_pct/max_pct/times`、
  `widgets.multi_select.*`、`widgets.data_table.*`、`common.*`——卡片类型名与参数
  标签尽量复用旧键，`cond_*` 仅在语义不同（如「放量」vs 旧「量能」）时新建。
- 枚举选项（MaKind 三值）标签复用 `screener.ma_above20/ma_above60/ma_bullish`。

### 7. 现有功能不回归

| 现状行为 | 迁移后保证 |
|---|---|
| 默认 `exclude_delisted = true`，空条件 = 全市场非退市前 100 | **默认根组预置 6 张基础卡**（行业/交易所/板块/上市时长/市值/排除退市），排除退市勾选；行为与现状逐项一致（待确认项 1） |
| 技术条件默认全关 | 不预置技术卡，用户按需添加 |
| 启动恢复 `[screener]` TOML | `ScreenerPanel::new(restore: Option<&ScreenerQuery>)` **签名不变**；内部 `Filter::from(restore)` → `filter_to_items` 渲染构建器；构建后 AST 与 `Filter::from(restore)` 结构等价（模板可逆性） |
| 点筛选 → loading/error → 结果表 | 全部不动；消息改携 Filter（见下） |
| 「标签+控件同行」原子组（ref #220） | Leaf 卡行沿用同技巧；GROUP_ALIGNMENT 系列测试迁移到新结构（仍断言组内不拆行） |
| 21 个既有语义测试（Batch 1 已迁 Filter 入口） | 不受影响（引擎层不动） |

**实现契约变更**（设计指明，实现 agent 落地，非 UI 设计本身）：

- `messages.rs::RunScreenerRequest { query: ScreenerQuery }` → `{ filter: Filter }`。
  理由：构建器产出的是 AST，若继续经 ScreenerQuery 中转，Or/Not/序列卡等
  无法表达的节点会丢信息；后端 `run_screener` 本就收 `&Filter`（backend.rs L149-150
  去掉 `Filter::from` 即可）。
- `on_save` 持久化边界见待确认项 5（Batch 3 才做 AST 持久化）。

### 8. 可测试性（egui_kittest 锚点）

- **纯函数层**（单测，无头断言 AST 结构）：
  - `leaf_to_filter` / `group_to_filter`：每种 LeafKind 构建产物与模板表精确相等
    （断言 `Filter::PartialEq`）。
  - `filter_to_items`：反向识别——`filter_to_items(f) → 卡片 → to_filter → == f`
    **round-trip 等价**；对 From 产出的全部 11 类形状逐类断言。
  - 折叠规则、Not 包裹、未知形状兜底、Momentum 成对识别（同 factor 才成对，
    不同 n 的两 Cmp 不合成动量卡）。
- **kittest 集成层**（仿现有：`LANG_LOCK` + `Harness::new_ui`）：
  - **id 约定**：构建器内全部弹层组件显式 `id_salt`（`cond_{卡路径}_{字段}`，
    如 `cond_root_0_market_cap_min`）；卡行组件 id 由 Ui 层级派生，测试用 label 查询。
  - **label 锚点**：添加按钮「添加条件」、空态标题「暂无筛选条件」、Segmented
    「且 (AND)」「或 (OR)」、类型 Dropdown 选项文本、删除按钮 = IconButton
    phosphor 字形（现有惯例 `get_by_label("\u{E20C}")`；多卡同图标用
    `query_all_by_label_contains` 按文档序取第 i 个）。
  - **交互路径测试**：添加（选类型→卡片出现→参数生效→AST 断言）；删除（卡消失）；
    AND-OR 切换（AST operator 翻转）；清空（空态出现）；嵌套（子组内加卡→
    AST 嵌套 And/Or）；restore（`ScreenerPanel::new(Some(&legacy_query))` →
    kittest 渲染 → 点击筛选 → 断言发出的 `RunScreenerRequest.filter` 与
    `Filter::from(legacy)` 等价）。

---

## 交互效果

egui 无 CSS/布局过渡——**动画克制**（避免过度设计）：

| 触发 | 效果 | 时长/easing | 说明 |
|---|---|---|---|
| 添加/删除卡片 | 瞬时出现/消失，无布局动画 | — | egui 无布局过渡；不强求 |
| AND-OR 切换 | Segmented 选中态即时更新 | 组件内建 | 无额外动画 |
| 卡片 hover | Leaf 卡行/按钮 hover 反馈 | 组件内建 | Card/IconButton/Button 自带 hover/press |
| 根组空 → 空态 | EmptyState 即时替换卡片列表 | — | 组件内建 |
| 筛选运行 | 「筛选」按钮 loading + spinner（现状） | — | 不动 |
| 取反开关 | 卡头图标/文字态切换 | — | 用 IconButton 选中态或 Tag「排除」标识 |
| 不实现 | 拖拽排序、卡片移动、展开折叠动画、过渡 | — | 成本高、非验收项 |

---

## 待确认

1. **默认条件布局**：根组预置 6 张基础卡（推荐——与现状行为逐项一致，含默认
   排除退市）vs 空态起步（更 Metabase 极简，但「空条件=全市场非退市」的默认
   行为需要靠空 AST 语义维持，UI 上无显式排除退市卡，观感变化大）。
2. **Or 组 / 序列卡（UpDays/Count）的运行时边界**：Batch 1 后端对 Or/Not/
   UpDays/Count 返回 `UnsupportedFilter`（Batch 3 引擎才支持）。Batch 2 选项：
   a) 允许构建，运行报错并展示友好提示（推荐——验收标准只要求 UI 落地 AND/OR
   嵌套，端到端运行等 Batch 3）；b) 添加菜单中置灰 + tooltip「引擎支持后续版本」。
3. **Leaf 卡「取反（Not）」开关**：a) 提供（推荐——AST 完整操作，Metabase
   exclude 范式；运行受 Batch 1 限制）；b) Batch 2 不做，Not 留给 Batch 4 LLM
   文本路径。
4. **Count 卡**（`N 日内满足`，5 个参数）是否 Batch 2 提供：a) 提供；b) 延后
   （参数多、引擎未支持，先只提供 UpDays）。
5. **持久化边界**（Batch 3 才做 AST 持久化）：Batch 2 的 `on_save` 如何收尾？
   a) 保持 legacy 保存（AST 可压缩为 ScreenerQuery 时保存，不可表达部分丢弃 +
   toast 提示）；b) 本批不保存（重启回到默认/legacy 恢复）；c) 顺手切 AST JSON
   持久化（超出 #245 验收范围，牵动 config 格式与迁移）。
6. **卡片编辑交互**：就地常驻编辑（推荐）vs Metabase 向导式（选类型→填参数→
   确认）。推荐就地：无模态状态、kittest 直给。

---

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 构建器代码归属 | compass crate 业务层（screener.rs 内新模块）/ compass-ui 新复合组件 | compass crate 业务层 | 视图模型依赖 `compass-types::Filter`；compass-ui 零业务依赖是硬边界（ui-widgets.md）；AST↔卡片映射是 screener 领域逻辑非通用组件 | 入 compass-ui 违反依赖方向（ui 引 types） |
| 卡片数据模型 | 视图模型双向映射 Filter / 新造独立查询模型 | 视图模型（`CondItem/CondGroup/CondLeaf` + `leaf_to_filter`/`filter_to_items` 纯函数） | UI 必须操作 AST（handoff 锁定）；双向纯函数保证 round-trip 结构等价 → 现有功能不回归可测试 | 新模型造成双份真相、与 Batch 4 LLM 路径分叉 |
| 嵌套组容器 | Card 内嵌 Card / Frame 轻量容器（bg_panel_alt + border_strong） | Frame 轻量容器 | ui-widgets.md 反模式：卡片内套卡片叠边框；缩进+底色+边框足够表达层级 | Card-in-Card 视觉脏、破坏嵌套深度 |
| 添加/编辑交互 | 就地常驻编辑（插入默认卡 + 卡上参数控件）/ Metabase 向导式（选类型→填参数→确认） | 就地常驻编辑 | 零模态状态机、kittest 直接驱动、改动即时写入模型；egui 向导式需临时态/弹层成本高 | 向导式更贴近 Metabase 视觉但实现与测试成本高（待确认项 6 保留给用户） |
| 连接符（卡间「且/或」小标签） | 组头 Segmented + 卡间连接符 / 仅组头 Segmented | 仅组头 Segmented | 组类型已在组头显式；连接符对单卡组冗余、增加垂直空间 | Metabase 视觉还原度略降，收益低于成本 |
| 删除/清空确认 | 无确认直接删 / 清空走 Modal | 无确认（删除单卡与清空均直接执行） | Metabase 惯例；条件可轻易重建，非破坏性数据 | Modal 打断高频编辑流（side bar 删除确认因持久化数据才需要） |
| 嵌套交互 | 添加菜单「子分组」+ 递归添加 / 拖拽卡片进组 | 添加菜单递归 | 拖拽在 egui 实现成本高、非验收项、Metabase 也有菜单式入口 | 拖拽易出误操作且测试难 |
| 未知 AST 形状 | 只读摘要卡 / 报错拒绝渲染 | 只读摘要卡（mono 弱化 JSON 摘要 + 删除） | UI 必须能显示任意 `Filter`（Batch 4 LLM 产物）；健壮性兜底一行成本 | 报错拒绝导致 restore/LLM 路径直接崩 |
| 消息契约 | `RunScreenerRequest` 改携 `Filter` / 保留 `ScreenerQuery` | 改携 `Filter` | 后端本就收 `&Filter`（backend.rs L149）；保留 ScreenerQuery 则 Or/Not/序列卡无法传输 | 保持现状需 UI 侧受限反向转换，丢 AST 表达力 |
| i18n 键 | `screener.builder.*` 子命名空间 / 扁平加键 | `screener.builder.*` | 既有模块前缀点分惯例（ref #222）；构建器是 screener 面板子域 | 扁平键混入既有 screener.* 语义不清 |
| 测试策略 | 纯函数 AST 断言 + kittest 交互路径双层 / 仅 kittest | 双层 | round-trip 等价与模板识别是纯逻辑，单测快且精确；交互路径 kittest 覆盖（验收要求） | 仅 kittest 对 AST 结构断言脆弱 |
| 动画范围 | 全部瞬时（组件内建 hover/press）/ 自定义布局动画 | 全部瞬时 | egui 无布局过渡，硬做动画成本高、kittest 难稳定 | 自定义动画违背克制原则 |
