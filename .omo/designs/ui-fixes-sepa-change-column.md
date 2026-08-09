# 涨跌幅列样式修复 — GUI 设计方案

> 状态：待用户确认（过程归档，非权威；确认后由主 agent 同步到 `kb/design/ui.md`）
> 面向：实现 agent（compass GUI 修复，不新增任何 UI 依赖、不新增 DataCell 变体）

## 目标

修复「涨跌幅」列同一值渲染两次的 bug，并重新设计该列样式与表头：

1. **修复**：SEPA 排名表「涨跌幅」列与 screener「20日涨跌幅」列当前渲染为
   `2.50 +2.50%` —— 同一数值出现两次（一次裸数字 `2.50` 无 % 无符号，一次
   `+2.50%`），用户无法分辨哪个是「值」哪个是「百分比」
2. **保持** `PriceText`「同时渲染 value 和 change」的双值语义（StatusBar
   三段式 `1500.00 +2.50%` 为既定设计，`kb/design/ui.md` L88-90/L117）
3. **约束**：只修 bug，不新增 `DataCell` 变体
4. **重设计**：本列单元格形态与表头，使用户一眼分辨「值」与「百分比」

## 现状

| 关注点 | 现状 | 文件 |
|---|---|---|
| PriceText 文本 | `text()` = `{price:.2} {format_change(change)}`（如 `1500.00 +1.23%`）；`change=None` 时仅 `{price:.2}`；`format_change` 产出 `+1.23%` / `-0.45%` / `0.00%` | `crates/compass-ui/src/widgets/price_text.rs` L64-68、L103-108 |
| PriceText 着色 | `color()` = `auto_tone(change)` → `Tone::Up/Down/Flat`（红涨绿跌） | `price_text.rs` L53-61、L83-89 |
| DataCell::Price | `{ value, change }` 变体；`render_cell` 构造 `PriceText::new(value).change(change)` 渲染；`value` 用于数值排序 | `data_table.rs` L36-41、L296-302、L267 |
| ColumnSpec | `{ header: &'static str, numeric: bool }`；numeric 列右对齐 | `data_table.rs` L63-68、L193-196 |
| SEPA 涨跌幅列 | `Price { value: row.change_pct as f32, change: Some(row.change_pct as f32) }` —— value 与 change 为同一值 | `crates/compass/src/citizens/sepa.rs` L428-431 |
| SEPA 表头 | 12 列 `COLUMNS`，第 11 列 `header: "涨跌幅", numeric: true` | `sepa.rs` L38-87（L83-86） |
| screener 涨跌幅列 | `Price { value: row.change_20d as f32, change: Some(row.change_20d as f32) }` —— 同一值 | `crates/compass/src/citizens/screener.rs` L307-309 |
| screener 表头 | 6 列 `COLUMNS`，第 3 列 `header: "20日涨跌幅", numeric: true` | `screener.rs` L61-86（L74-77） |
| 设计 token | `TypeTokens.mono` = 12px（JetBrains Mono）；`ColorTokens.up` = #EF5350 / `down` = #26A69A / `flat` = 主文本色（红涨绿跌） | `tokens/typography.rs` L15、`tokens/color.rs` L140-144/L179-180 |

### 根因（渲染层，非调用方赋值错误）

调用方把 `value` 与 `change` 设为同一 change_pct **是有意且合理的**：

- `value` 的唯一职责是**数值排序**（`sort_rows` 按 `Price.value` 比较，`data_table.rs` L267）——涨跌幅列按涨跌幅数值排序，value 必须是 change_pct 本身
- `change` 的唯一职责是**着色与渲染**（红涨绿跌）——change 必须是 change_pct

真正的缺口在 **`PriceText::text()`**：它只有「价格 + 涨跌幅」一种双段渲染
（`{price:.2} {change}`），**没有「值本身就是百分比」的单值模式**。于是当
value == change（涨跌幅列）时，同一数字被渲染两次：`2.50 +2.50%`。

> 佐证：StatusBar（`status_bar.rs` L92-97）传 `price=1500.25, change=1.23`，
> 两值不等，双段渲染 `1500.25 +1.23%` 语义正确，不受影响。

## 设计方案

### 1. 渲染形态（单元格）

**涨跌幅列单元格 = 单一百分比文本**：

```
修复前:  2.50 +2.50%        ← 同一值渲染两次，无法分辨
修复后:  +2.50%             ← 单一百分比，带符号 + % 后缀
```

- 正值 `+2.50%`（显式 `+` 号），负值 `-1.23%`，零值 `0.00%`（无符号）
- **分辨逻辑**：表格内带 `%` 后缀的就是涨跌幅（值 = 百分比合一），无 `%`
  的是价格——「最新价」列 `1500.00`（flat 中性色）与「涨跌幅」列 `+2.50%`
  （红涨绿跌）并排，用户一眼即辨
- 复用 `format_change`（`price_text.rs` L103-108）——格式、符号、% 后缀、
  两位小数、零值无符号，全部既有逻辑，不新造格式

### 2. 表头文字

**保持原文案，不做改动**：

| 表格 | 表头（现状） | 决策 |
|---|---|---|
| SEPA 排名表 | `涨跌幅`（`sepa.rs` L84） | 保持 |
| screener | `20日涨跌幅`（`screener.rs` L75） | 保持 |

理由：

- 中文「涨跌幅」语义即百分比，无歧义；单元格带 `%` 后缀已提供「值 vs 百分比」
  的第一重分辨，表头加 `%` 冗余且加宽列
- 与既有权威/归档文档一致（`kb/design/ui.md` L117、`.omo/designs/sepa-gui.md` L90）
- 「20日涨跌幅」的周期语义（20 日）只存在于表头，是必要信息，保持

`numeric: true` 保持（右对齐 + 排序 + mono）。

### 3. 颜色与字体

| 项 | 值 | 来源 |
|---|---|---|
| 字体 | JetBrains Mono，`TypeTokens.mono` 12px | `typography.rs` L15 |
| 正涨 | `ColorTokens.up` #EF5350（红） | `color.rs` L179 |
| 下跌 | `ColorTokens.down` #26A69A（绿） | `color.rs` L180 |
| 平盘 | `ColorTokens.flat`（主文本色） | `color.rs` L144 |
| 排序 | 按 `value`（= change_pct）数值序 | `data_table.rs` L267，不回归 |

全部复用既有 token 与组件通道，零硬编码、零新依赖。

### 4. 渲染层落点：`PriceText` 增加「值即百分比」模式 + `render_cell` 触发

**修复分两层，均落在渲染层，调用方传参保持不变：**

#### 4a. `PriceText` 新增 `percent_only()` 模式（`price_text.rs`）

```rust
pub struct PriceText<'a> {
    tokens: &'a ThemeTokens,
    price: f32,
    change: Option<f32>,
    tone: Tone,
    percent_only: bool,          // 新增：true 时 value 即 change（百分比列）
}

/// 值本身就是百分比：渲染为单一 `+2.50%`（不再拼 price）。
pub fn percent_only(mut self) -> Self { self.percent_only = true; self }

pub fn text(&self) -> String {
    match self.change {
        Some(change) if self.percent_only => format_change(change),   // 新增分支
        Some(change) => format!("{:.2} {}", self.price, format_change(change)),
        None => format!("{:.2}", self.price),
    }
}
```

- `color()` **零改动**——`auto_tone(self.change)` 已正确给出红涨绿跌
- 双值语义（`price + change` 双段）完整保留：`percent_only` 默认 false，
  StatusBar（`status_bar.rs`）不调用该方法，行为与现完全一致
- 命名建议 `percent_only()`；实现时可换 `as_percent()` 等更贴切的名字，
  语义为「value 本身是百分比，仅渲染 change」

#### 4b. `DataTable::render_cell` 的 `Price` 分支识别百分比列（`data_table.rs` L296-302）

```rust
DataCell::Price { value, change } => {
    let mut price = PriceText::new(tokens, *value);
    if let Some(change) = change {
        price = price.change(*change);
        if *change == *value {           // 约定：value 即 change → 百分比列
            price = price.percent_only();
        }
    }
    price.show(ui);
}
```

- **触发条件**：`change == Some(v) && v == *value`——即调用方传入「value 与
  change 同一值」的既有传法（SEPA/screener 正是如此）
- **不新增 `DataCell` 变体、不改 `ColumnSpec`、不改 `DataTable` 公开 API**——
  满足全部约束
- f32 相等判断的稳健性：`row.change_pct as f32` 为 f64→f32 的确定性转换
  （round-to-nearest），同源两次转换结果必然相等 → 恒触发；StatusBar 不走
  `render_cell`（直用 `PriceText`），不受该判断影响

**约定风险与缓解**：若未来某调用方恰传 `Price { value: 价格, change: Some(价格) }`
（价格数字恰等于涨跌幅数字，如 2.5 元涨 2.5%），会被误判为百分比列。缓解：
① 该场景现有代码不存在（StatusBar 直用 PriceText 不经过此分支）；② 在
`render_cell` 与两处调用点加注释说明「value == change 即百分比列」约定；
③ 若将来出现更多百分比列用法，可显式化（如 `ColumnSpec` 增格式字段），
本期不做（超出「只修 bug」范围）。

### 5. 调用点改动（`sepa.rs` / `screener.rs`）

**传参保持不变**（value = change_pct 用于排序、change = change_pct 用于着色，
两职责都正确），**仅补注释**说明约定：

- `sepa.rs` L428-431：注释「涨跌幅列：value 与 change 同为 change_pct
  （value 供排序 / change 供红涨绿跌着色）；render_cell 据 value == change
  识别为百分比列，渲染单一 `+2.50%`」
- `screener.rs` L307-309：同上，注明 20 日口径

> 若实现 agent 评估后认为改传参更直观（如改为某显式表达），需先回本方案
> 确认——默认方案是传参不动、渲染层识别，改动面最小且不触碰排序语义。

## 交互效果

本修复**不引入任何新交互/动画**（信息密度优先，符合金融终端风格）：

| 触发 | 表现 | 目标状态 |
|---|---|---|
| 涨跌幅单元格 hover | 行底色 `bg_hover`（既有） | 与 DataTable 现有 hover 一致 |
| 行选中 | `selection_bg` 行高亮，单元格保留红涨绿跌语义色 | 与现有选中行为一致（`render_cell` L283-287 已有保护） |
| 表头点击排序 | 按 `value`（涨跌幅数值）排序，`↓`/`↑` 箭头 | 与现有排序行为一致，不回归 |
| 主题切换 | `set_tokens` 刷新，百分比文本随 up/down/flat token 换色 | 与现有主题切换一致 |

## kittest 验证建议

### compass-ui（`price_text.rs`、`data_table.rs`）

1. `price_text.rs` 新增 `percent_only_text`：
   - `PriceText::new(&tokens, 2.5).change(2.5).percent_only().text() == "+2.50%"`
   - `PriceText::new(&tokens, 2.5).change(-1.23).percent_only().text() == "-1.23%"`
   - `PriceText::new(&tokens, 2.5).change(0.0).percent_only().text() == "0.00%"`
   - 双值语义不回归：`.text()`（不加 percent_only）仍为 `2.50 +2.50%`（现有
     `rendered_text_contains_price_and_change` 测试保持绿）
2. `price_text.rs` 新增 `percent_only_color`：
   - 正 → `tokens.color.up`；负 → `tokens.color.down`；零 → `tokens.color.flat`
3. `data_table.rs` 新增 `price_cell_equal_value_change_renders_single_percent`：
   - 行含 `Price { value: 2.5, change: Some(2.5) }` → `harness.get_by_label("+2.50%")`
   - **断言 `2.50 +2.50%` 不存在**（`get_by_label("2.50 +2.50%")` 应 panic/失败）——
     锁死 bug 回归
   - 同表含 `Price { value: 1500.0, change: None }` → `"1500.00"`（最新价列不回归）
   - 同表含 `Price { value: 10.0, change: Some(2.0) }` → `"10.00 +2.00%"`（双值列不回归）
4. 排序不回归：现有 `price_column_sorts_numerically*` 测试已覆盖 `Price.value`
   排序，无需改动，跑绿即证

### compass（`sepa.rs`、`screener.rs` 集成）

5. `sepa.rs` `results_renders_rows_thermometer_and_detail`（L750-767）追加：
   `harness.get_by_label("+2.50%")`（sample_data `change_pct: 2.5`，L649）——
   并确认 UI 中不存在 `"2.50 +2.50%"`
6. `screener.rs` 集成测试（若有结果表断言）追加对应 `+2.50%` 断言；若 sample
   数据的 `change_20d` 为负值则断言 `-x.xx%`，以实际 fixture 为准
7. 全部用 `get_by_label`（精确匹配）而非 `get_by_label_contains`，避免「值仍
   出现两次」的宽松断言漏检

## 待确认

1. **表头保持原文案**（「涨跌幅」「20日涨跌幅」，不加 `%`）——理由见 §2；
   如需表头显式标注（如「涨跌幅 %」）请指出，实现侧仅改两处 `header` 字符串
2. **`PriceText` 新方法命名**：建议 `percent_only()`；备选 `as_percent()` /
   `percent()`——语义均为「value 即 change，仅渲染 change」，实现侧自选
3. **隐式约定 vs 显式化**：默认采用 `value == change` 隐式识别（改动最小，
   满足「不新增变体/只修 bug」）；若倾向显式（如 `ColumnSpec` 增格式标记），
   属超范围改动，需另行确认

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 涨跌幅单元格形态 | 单一百分比 `+2.50%` / 保留双段 `2.50 +2.50%` / 仅裸数字 `2.50` | 单一百分比 `+2.50%` | 值即百分比，重复显示无信息增量；带符号 + % 后缀使「值 vs 百分比」一眼可辨；复用 `format_change` 既有格式 | 双段是 bug 本体；裸数字丢 % 身份且需脑内换算 |
| 修复落点 | 渲染层（PriceText + render_cell）/ 调用方改传参 / 新增 DataCell 变体 | 渲染层：`PriceText::percent_only()` + `render_cell` 识别 | value 供排序、change 供着色是调用方的**正确**职责分配（`data_table.rs` L267 排序契约），改传参必然破坏其一；根因在 `PriceText::text()` 缺「值即百分比」模式；不新增变体满足约束 | 调用方改传参会丢红涨绿跌（change=None → flat）或丢排序语义；新增 `DataCell::Percent` 违反「不新增变体」约束 |
| 百分比列识别方式 | `value == change` 隐式约定 / `ColumnSpec` 增格式字段 / `DataCell::Price` 增字段 | `value == change` 隐式约定（render_cell 内判断 + 注释文档化） | 改动面最小（仅 render_cell 6 行）；现有两处调用方传参天然满足；f32 同源转换相等判断确定性强；StatusBar 直用 PriceText 不受影响 | 显式化属超范围（改组件公开 API/结构）；隐式约定的误判场景（价格数字恰等于涨跌幅）现有代码不存在，注释 + 文档化即够 |
| 表头文字 | 保持「涨跌幅」「20日涨跌幅」/ 加 `%` 后缀 | 保持原文案 | 中文「涨跌幅」语义即百分比，无歧义；单元格带 % 已分辨值/百分比；与权威/归档文档一致（ui.md L117、sepa-gui.md L90）；「20日」周期信息仅存于表头不可省 | 加 `%` 冗余、加宽列，无信息增量 |
| PriceText 能力 | 新增 `percent_only()` 模式 / render_cell 内手写 label | 新增 `percent_only()` 模式 | 「值即百分比」是价格文本的自然形态变体，作为组件一等能力自洽、可测、可复用（将来其他面板可用）；color() 零改动复用 auto_tone | render_cell 手写 label 重复 PriceText 的 mono/size/color 规格，且能力困在表格内无法复用 |
| 排序语义 | 按 `value`（= change_pct）数值序 | 保持现状（value 不变） | 涨跌幅列按涨跌幅数值排序是正确语义；传参不动即零回归 | 改为按 change 或显示文本排序无必要且破坏契约 |
