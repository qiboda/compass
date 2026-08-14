# 选股器基础条件布局（标签+控件原子组） — 视觉设计方案

> 归档：`.omo/designs/ui-fixes-screener-layout.md`（过程归档，非权威）
> 权威文档：`kb/design/ui.md`（经用户确认后由主 agent 同步最终要点）
> 日期：2026-08-09
> 关联：epic #217 / sub-issue #220

## 目标

1. 修复选股器条件表单的换行缺陷：当前单个 `ui.horizontal_wrapped` 承载
   全部条件组，**换行切点可能落在「标签」与「控件」之间**（如「行业」在
   行尾、下拉控件被挤到下一行），标签与控件脱节、可读性差。
2. 每个「标签 + 控件」组包进独立 `ui::horizontal`（不换行）原子单元，
   外层 `horizontal_wrapped`——**换行只发生在组间，组内标签与控件永远
   同排**（已锁定决策）。
3. 基础条件（basic_conditions）与技术面条件（technical_conditions）两张
   卡片同构改造；不改变控件行为、不改变条件语义、不引入新依赖。

## 现状

### 基础条件（`crates/compass/src/citizens/screener.rs` L339-391）

单个 `ui.horizontal_wrapped` 顺序平铺 6 组，组间 `add_space(tokens.spacing.md)`：

| # | 标签 | 控件 | 说明 |
|---|---|---|---|
| 1 | `SectionTitle("行业")` | `ms_industry`（MultiSelect） | 标签 14px heading，控件 32px |
| 2 | `SectionTitle("交易所")` | `ms_exchange`（MultiSelect） | |
| 3 | `SectionTitle("板块")` | `ms_board`（MultiSelect） | |
| 4 | `SectionTitle("上市时长")` | `Dropdown`（`width(100.0)`） | |
| 5 | `SectionTitle("市值(亿)")` | `DragValue(min).prefix("min ")` + `DragValue(max).prefix("max ")` | 两个 DragValue 间无间距 |
| 6 | `Checkbox("排除退市")` | 自带标签的单控件 | 无独立 SectionTitle |

**缺陷**：`horizontal_wrapped` 在**任意 widget 之间**换行——切点落在
`SectionTitle` 与控件之间时，标签与控件分居两行。

### 技术面条件（L397-451）

同一模式，4 组，每组 = `Checkbox`（启用开关）+ 启用时显示的**条件参数段**：

| # | 开关 | 参数段（启用时显示） |
|---|---|---|
| 1 | 均线 | `Dropdown`（`width(210.0)`，3 选项） |
| 2 | 突破新高 | `"N:"` label + `DragValue`（1..=250） |
| 3 | 动量 | `"N:"` + DragValue + `"min%:"` + DragValue + `"max%:"` + DragValue |
| 4 | 量能 | `"N:"` + DragValue + `"倍数:"` + DragValue |

### 组件与间距（compass-ui）

- 间距 token（`crates/compass-ui/src/tokens/spacing.rs`）：xs 4 / sm 8 /
  md 12 / lg 16；控件高度 control_md 32px（Dropdown/MultiSelect 触发按钮
  `min_size(0, control_md)`）。
- `SectionTitle`（`widgets/section_title.rs`）：纯文本 heading（14px strong
  text_primary），内部自带 horizontal 但仅单行标签，无下划线/边框——
  **组内嵌套安全**。
- `MultiSelect` 触发按钮宽度 = 内容自适应（summary 如「全部 (N)」「已选 3 项」，
  弹层固定 220px）；`Dropdown` 显式宽度（100 / 210）。
- 卡片：`Card` + `CardPadding::Md`（`condition_form` L320-334），两卡之间
  `ui.add_space(tokens.spacing.sm)`。

## 设计方案

### 1. 原子组结构（两卡同构）【设计决策，锁定】

```
ui.horizontal_wrapped(|ui| {
    // —— 组（原子单元）：组内永不换行 ——
    ui.horizontal(|ui| {
        SectionTitle::new(&tokens, "行业").show(ui);
        self.ms_industry.show(ui);
    });
    ui.add_space(tokens.spacing.md);          // 组间水平间距（保持现状 12px）

    ui.horizontal(|ui| { SectionTitle("交易所"); ms_exchange; });
    ui.add_space(tokens.spacing.md);
    // … 板块 / 上市时长 / 市值(亿) …
    ui.horizontal(|ui| { SectionTitle("市值(亿)"); DragValue(min); DragValue(max); });
    ui.add_space(tokens.spacing.md);

    // 单控件组（排除退市）：Checkbox 自带标签，天然原子，不嵌套
    Checkbox::new(&tokens, &mut self.form.exclude_delisted, "排除退市").show(ui);
});
```

**技术面同构**——原子组 = 开关 + 其全部条件参数：

```
ui.horizontal_wrapped(|ui| {
    ui.horizontal(|ui| {
        Checkbox(均线); if ma_enabled { Dropdown(210).show(ui); }
    });
    ui.add_space(tokens.spacing.md);
    ui.horizontal(|ui| {
        Checkbox(突破新高); if enabled { "N:"; DragValue; }
    });
    ui.add_space(tokens.spacing.md);
    ui.horizontal(|ui| {   // 动量：整组原子，含 3 个参数
        Checkbox(动量); if enabled { "N:"; DragValue; "min%:"; DragValue; "max%:"; DragValue; }
    });
    ui.add_space(tokens.spacing.md);
    ui.horizontal(|ui| { Checkbox(量能); if enabled { "N:"; DragValue; "倍数:"; DragValue; } });
});
```

规则：

- **组内结构不变**：各组内部现有的标签顺序、控件参数（width、prefix、
  range）逐项保留，仅改变嵌套层级。
- **组间水平间距**保持 `tokens.spacing.md`（12px，现状一致）。
- **组内间距**保持 egui 默认（标签与控件间 ~8px；市值组 min/max 间
  现状无间距，保持）。
- **排除退市**：单控件（Checkbox 自带标签）本身不可拆，**不包**
  `horizontal`——嵌套与否行为等价，不包减少一层嵌套。
- 若参数段隐藏（开关关闭）时组内只剩 Checkbox，组宽收缩——wrap 切点
  随之自适应，无固定宽度要求。

### 2. 换行与窄窗口行为【设计决策】

| 场景 | 行为 |
|---|---|
| 宽度充足（默认 1440 窗口） | 所有组排在第一行（或按需 2 行），组内标签+控件恒同排 |
| 宽度不足（面板拖窄） | **整组**移至下一行；组内永不拆开（`horizontal_wrapped` 对嵌套 `horizontal` 子 ui 按整组宽度判定） |
| 面板窄于最宽组 | 最宽组（动量 ~450px）整组溢出被裁剪——已知极限行为，接受（条件表单不做滚动/折叠，属本 issue 范围外） |

**宽度预算**（客观估算，body 12.5px + 控件）：

| 组 | 估算宽 | 备注 |
|---|---|---|
| 行业/交易所/板块 | ~150px 各 | 标签 ~28px + MultiSelect summary 80–120px（内容自适应） |
| 上市时长 | ~180px | 标签 ~56px + Dropdown 100px |
| 市值(亿) | ~220px | 标签 ~56px + 2×DragValue ~70px |
| 排除退市 | ~90px | Checkbox 自带标签 |
| 均线组 | ~290px | Checkbox ~64px + Dropdown 210px |
| 动量组 | ~450px | Checkbox + 3×（label+DragValue）——最宽组 |

6 组基础条件合计 ≈ 1100–1200px；Screener 卡片可用宽度（1440 窗口 − 侧边栏
240 − dock padding）≈ 1100px → 典型布局为**第一行 4–5 组、第二行 1–2 组**，
切点只发生在组间——正是期望行为。

**注意**：MultiSelect 摘要宽度随选项数变化（「全部 (N)」vs「已选 3 项」），
组宽因此漂移、wrap 切点随之移动——这是可接受的（组间换行本就允许漂移），
**不为此固定组宽**。

### 3. 实施位置【实现细节，非设计决策】

| 修改点 | 内容 |
|---|---|
| `screener.rs` `basic_conditions`（L339-391） | 6 组各自包 `ui::horizontal`，组间 `add_space(md)` 保留 |
| `screener.rs` `technical_conditions`（L397-451） | 4 组同构改造；参数段随组整体移动 |
| 不动 | `condition_form` 卡片结构（L320-334）；`SectionTitle`/`MultiSelect`/`Dropdown`/`Checkbox` 组件；表单状态与持久化逻辑 |

> 组件层（compass-ui）无改动——原子组是**布局嵌套**，不是新组件。

### 4. 对齐与视觉【设计决策】

- 组内标签（SectionTitle 14px）与控件（32px）在 `horizontal` 中垂直居中
  对齐，行内各控件按各自高度自然基线——egui 默认行为，无需额外处理。
- 行间距：`horizontal_wrapped` 的 `item_spacing.y` 默认 ~3px，多行时行距
  偏紧；建议设为 `tokens.spacing.sm`（8px）提升多行可读性（见待确认 1）。
- 两行布局下，第二行组与第一行组的组间水平间距仍为 md——行距与组距
  各自独立，不互相影响。

## 交互效果

| 触发 | 行为 | 变化 |
|---|---|---|
| 窗口/面板拖拽变窄 | 组间换行 | 换行切点从「任意 widget 之间」变为「组之间」：标签与控件永不同行分离 |
| 切换主题 | 无 | 布局不变，颜色走 token |
| 开关技术面参数（勾选/取消） | 参数段显隐 | 组宽收缩/扩展，wrap 切点自适应 |
| 多选摘要变化 | 组宽变化 | wrap 切点随 MultiSelect 摘要长度漂移（可接受） |

无动画、无新增快捷键、无反馈状态变化——纯布局修复。

## 待确认（已全部确认，2026-08-09）

1. ~~**多行行距**~~：**用户确认提至 `tokens.spacing.sm`（8px）**——外层
   `horizontal_wrapped` 设 `item_spacing.y = sm`，多行可读性提升。
2. **市值组 min/max 间距**：**保持现状**（最小 diff，prefix "min "/"max "
   已起分隔作用）——维持推荐，无需用户决策。
3. **排除退市是否包组**：**不包**（单控件天然原子）——维持推荐。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 原子组容器（锁定） | 每组 `ui::horizontal` / 自定义换行逻辑 / 表格布局 | 每组 `ui::horizontal`，外层 `horizontal_wrapped` | egui 原生嵌套 horizontal 即可整组判定换行，零新依赖、零自定义布局；实现与测试成本最低 | 自定义换行需自算宽度与坐标，脆弱；表格布局与条件表单控件高度/显隐动态不符 |
| 组间间距 | `spacing.md`（12px）/ sm / lg | 保持 `spacing.md` | 与现状一致（现有代码即 md），仅改嵌套不改间距语义 | 改间距引入与本次修复无关的视觉变化 |
| 排除退市组 | 包 `horizontal` / 不包 | 不包 | Checkbox 自带标签的单控件不可拆，包与不包行为等价 | 包了仅多一层嵌套，无行为收益（若用户偏好代码统一可包，等价） |
| 组宽策略 | 固定每组建宽 / 内容自适应 | 内容自适应 | 控件宽度本由组件决定（MultiSelect summary 自适应、Dropdown 显式宽度）；固定宽度需给 MultiSelect 加 width API，扩大改动面 | 固定宽度在选项文字变化时反而产生多余空白/裁剪 |
| 最窄面板行为 | 组内 overflow 裁剪 / 组内允许换行 / 表单滚动 | 组内不换行，整组溢出裁剪（接受） | 「组内永不换行」是锁定决策；最宽组 ~450px 远小于任何实际窗口（Screener 面板可拖拽但 dock 区最小宽 >600px） | 组内换行违背锁定决策；表单滚动引入 ScrollArea 层级，超出本 issue 范围 |
| 行距 | 默认 ~3px / `spacing.sm` 8px | **`spacing.sm`（用户确认 2026-08-09）** | 两行表单行距 3px 视觉过紧；sm 与组内间距梯级一致 | 默认值零改动，但可读性差 |

> 待确认项已全部由用户确认（2026-08-09），最终要点同步至 `kb/design/ui.md`。
