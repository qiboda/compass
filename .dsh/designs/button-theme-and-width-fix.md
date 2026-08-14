# Button 主题感知文字色 + loading 宽度行为修复

> 设计归档：`ui-designer` 产出，随实现 PR 提交（ref #217 相关）。
> 权威文档同步：`kb/design/ui.md`（决策记录）、`kb/design/ui-widgets.md`（Button 条目）。
> 日期：2026-08-09

## 目标

1. **问题 1**：修复 Primary/Danger 按钮在亮色主题下文字看不清——`light` 下
   `text_primary`（#1B2430 深黑）落在 `accent`（#2962FF 亮蓝）/`error`（#D93025 深红）
   底上对比度不足。要求：
   - dark + light 两主题都清晰可读
   - 不破坏 ref #217 验收精神（按钮文字色**跟随主题切换**，不是硬编码单一色值）
   - Default/Ghost 变体维持 `text_primary` 不变（浅底/透明底上深色字本就合理）
2. **问题 2**：修复 SEPA 刷新按钮 loading 时文本从「刷新」变「计算中…」但宽度不随
   文本变化的观感问题。给出确定性的宽度行为规范（跟随增长 / 固定防跳 / 最小宽度
   三选一），并评估 main.rs Fetch 按钮是否同病。
3. 输出影响面分析（组件、调用方、权威文档、测试），全部改动收敛在 `compass-ui`
   组件层 + 两处调用方 + 两份 kb 文档。

## 现状

### 问题 1 —— 文字色

- `crates/compass-ui/src/widgets/button.rs` L105-123 `variant_colors()`：四变体
  （Default/Primary/Danger/Ghost）文字色**全部**返回 `c.text_primary`；L155-162
  loading 保留变体文字色、仅真 `disabled` 降为 `text_disabled`。
- `crates/compass-ui/src/tokens/color.rs` L161-220：`accent = #2962FF` /
  `accent_hover = #4D7FFF` / `accent_pressed = #1E4FD6` 两主题**同值**；
  `text_primary` 各主题不同（dark `#D1D4DC` 浅灰 / light `#1B2430` 深黑）；
  `error` dark `#EF5350` / light `#D93025`。
- **对比度测算**（WCAG 2.1 相对亮度，sRGB 线性化）：

  | 前景 | 背景 | 对比度 | 判定（4.5:1 AA 正文） |
  |---|---|---|---|
  | #1B2430（light text_primary） | #2962FF（accent） | 3.19:1 | ✗ |
  | #D1D4DC（dark text_primary） | #2962FF（accent） | 3.30:1 | ✗（用户主观验收可读） |
  | #FFFFFF（白） | #2962FF（accent） | **4.90:1** | ✓ |
  | #FFFFFF（白） | #D93025（light error） | **4.77:1** | ✓ |
  | #1B2430（light text_primary） | #D93025（light error） | 3.28:1 | ✗ |
  | #FFFFFF（白） | #4D7FFF（accent_hover） | 3.62:1 | hover 瞬时态可接受 |
  | #FFFFFF（白） | #1E4FD6（accent_pressed） | 6.68:1 | ✓ |
  | #D1D4DC（dark text_primary） | #EF5350（dark error） | 2.35:1 | ✗ |
  | #FFFFFF（白） | #EF5350（dark error） | 3.48:1 | ✗（仍低于 AA，但显著优于浅灰） |

- 结论：**白字是 accent/error 底上唯一稳定的达标前景**；light 深色字在两底上都失败。
  ref #217 的「跟随主题」诉求 ≠ 必须用 `text_primary`——它要求的是**文字色由主题
  派生、切换主题时随之变化**，`on_accent`/`on_error` token（dark/light 各定义不同值）
  同样满足，且能针对性修复 light。

### 问题 2 —— 按钮宽度

- `crates/compass/src/citizens/sepa.rs` L269-294 工具栏：`ui.horizontal` 内
  `ui.with_layout(right_to_left)`，Primary 刷新按钮文本
  `if loading { "计算中…" } else { "刷新" }`，`.icon(ARROW_CLOCKWISE)` +
  `.loading(loading)`；L387 空态 EmptyState 同样有 Primary「刷新」按钮（无 loading）。
- `crates/compass/src/main.rs` L1030-1040 工具栏 Fetch：同模式，
  `if loading { "加载中…" } else { "Fetch" }` + `.icon(DOWNLOAD_SIMPLE)` +
  `.loading(loading)`，`ButtonSize::Lg`。
- `crates/compass-ui/src/widgets/button.rs` L170-198：`.min_size(Vec2::new(0.0, self.height()))`
  —— 宽度下限 0，**理论应跟随文本**；loading 时在 `response.rect` 上画
  `from_black_alpha(102)` 遮罩 + 右缘 14px spinner。
- **egui 0.35 Button 底层机制**（源码确认）：Button 已重构为 `AtomLayout`
  （`atomics/atom_layout.rs` + `atomics/atom.rs` + `widgets/button.rs`）：
  1. `atom_ui` → `layout.min_size(min_size).allocate(ui)` →
     `AtomLayout::measure(ui, ui.available_size())`；
  2. **`ui.available_size()` 在 `right_to_left` 子 ui 中返回整个子 ui 的可用宽度**
     （layout.rs：RightToLeft 初始 cursor `min.x = -INFINITY`、`avail.max.x = cursor.max.x`，
     即**全部剩余宽**，不是按钮自身的自然宽）；
  3. Button 默认 `wrap_mode` 取 `ui.wrap_mode()`（egui 默认 `TextWrapMode::Truncate`）；
     `measure` 中 `wrap_mode != Extend` 时把第一个 Text atom 标记为 `shrink`；
  4. shrink 文本的可用宽 = `available_inner_size - 其他 atom`（按钮只有文本+icon，
     icon 是字符拼进 label 的，故 shrink 文本拿到的可用宽 ≈ 整个子 ui 宽）；
  5. `into_sized` 中 `wrap_mode` 仍为 Truncate，galley 以 `max_width = 子 ui 宽`
     布局——**文本比 max_width 短时 galley 宽 = 文本自然宽，不拉伸**（text_layout.rs
     `rows_from_paragraphs` 早退路径 `paragraph_width <= wrap_width`）。
- **根因判断**：静态分析下按钮宽度**应当**跟随文本（`min_size.x = 0`，Truncate 不拉伸）。
  用户反馈「宽度未变」的候选根因（按可能性排序）：
  a. **loading 遮罩 + spinner 的视觉干扰**：遮罩覆盖 `response.rect` 全宽、spinner
     固定在右缘，文本变化被压暗，观感上「宽度没变」（遮罩复用 rect 但 rect 是否真的
     变宽需实现阶段用 kittest 断言复现确认）；
  b. **`ui.horizontal` 外层宽度钳制**：horizontal 布局中第一个 label 之后
     `with_layout(right_to_left)` 子 ui 的 `max_rect` 可能被 `available_rect_before_wrap`
     限制，若按钮文本变长后子 ui 可用宽不变、而 Button 的 `min_size` 或某处存在
     隐性宽度下限（egui 无默认 min width，但需实测确认）;
  c. **galley 缓存/尺寸缓存**：同 id 按钮的尺寸被 egui memory 缓存（需实测排除）。
- **结论**：根因无法纯静态定死，方案应**不依赖根因**而给出确定性宽度行为——
  用「最小宽度策略」让 idle/loading 两态宽度一致，无论根因是 a/b/c 都能修复观感。

## 设计方案

### 问题 1 —— 主题感知文字色

**核心思路**：引入两个「实色底上的对比文字色」token——`on_accent` 与 `on_error`，
语义与 Material Design 的 `on-*` 颜色一致：**某个彩色实底上承载的前景文字色**。
两主题各定义独立值，dark/light 切换时文字色随之变化（满足 ref #217 精神），
但值不再绑定 `text_primary`（它只描述「普通浅底上的主文字」）。

**token 定义**（`tokens/color.rs` `ColorTokens` 新增两字段）：

| token | dark | light | 依据 |
|---|---|---|---|
| `on_accent` | `#FFFFFF` | `#FFFFFF` | accent 底上白字 4.90:1 达标；accent 两主题同色，其上的对比前景同色自洽（用户确认 2026-08-09：dark 用纯白，主流深色主题 GUI 亮蓝底白字为常态） |
| `on_error` | `#FFFFFF` | `#FFFFFF` | dark error #EF5350 上白字 3.48:1（优于现状浅灰 2.35:1）/ light error #D93025 上白字 4.77:1 达标 |

> 用户确认（2026-08-09）：dark 主题 on_* 采用纯白 `#FFFFFF`（选项 A），
> 排除冷白 `#F2F4F8`（4.45:1 略低于 AA 4.5 阈值，不推荐）。

**variant_colors() 改动**（`widgets/button.rs` L105-123）：

```rust
ButtonVariant::Default => (c.bg_panel_alt, c.bg_hover, c.bg_active, c.text_primary),
ButtonVariant::Primary => (c.accent, c.accent_hover, c.accent_pressed, c.on_accent),
ButtonVariant::Danger  => (c.error, c.error.gamma_multiply(1.15),
                           c.error.gamma_multiply(0.85), c.on_error),
ButtonVariant::Ghost   => (Color32::TRANSPARENT, c.bg_hover, c.bg_active, c.text_primary),
```

- Default/Ghost 不动（`text_primary` 在浅底/透明底上合理）。
- Primary/Danger 改用 `on_accent`/`on_error`。
- loading 保留变体色逻辑（L155-162）不变——loading 仍用变体文字色（即新的
  `on_accent`/`on_error`），只有真 `disabled` 降为 `text_disabled`。
- 现有测试 `loading_button_keeps_variant_text_color`（断言 loading 渲染
  `text_primary`）需改为断言渲染变体色 token。

**对 ref #217 验收的兼容性**：
- 验收原文是「fetch 按钮文字颜色不跟随主题」——指硬编码白字不随主题切换。
- 新方案中按钮文字色由各主题 token 派生（属于 `on_accent` token 体系），不再硬编码；
- 若用户希望 dark/light 视觉上有差异，可在「待确认」中开放 dark 用冷白
  `#F2F4F8`、light 用纯白 `#FFFFFF`，仍属主题感知。

### 问题 2 —— 宽度行为（最小宽度策略）

**方案选型**：三选项中选「**最小宽度策略**」。

| 策略 | 表现 | 问题 |
|---|---|---|
| 跟随文本增长 | loading 时按钮随「计算中…」变宽 | 宽度跳变；right_to_left 布局中变宽方向向左挤压 Segmented，观感抖动；且可能触发根因 b/c |
| 固定宽度防跳 | 按钮恒宽 | 简单但 idle 时「刷新」右侧留白大，与工具栏其余控件不对齐 |
| **最小宽度（推荐）** | 宽度 = max(文本自然宽, min_width)；min_width 取「两种文本中的较宽者」 | idle/loading 两态宽度一致，无跳动；文本更长时仍跟随增长，不失通用性 |

**组件改动**（`widgets/button.rs`）：

1. 新增字段 `min_width: f32`（默认 0.0）+ builder `.min_width(f32)`；
2. `show()` 中 `.min_size(Vec2::new(self.min_width, self.height()))`；
3. loading 遮罩/spinner 复用 `response.rect` 不变——宽度稳定后遮罩范围与
   spinner 右缘位置也稳定，视觉更干净。

**调用方改动**：

- `sepa.rs` L276 工具栏刷新按钮：`.min_width(96.0)`——覆盖「刷新」（icon+2 字）与
  「计算中…」（icon+4 字）中较宽者的近似宽度；具体数值由实现阶段按
  `tokens.typography.body` + phosphor 字形实测后微调（设计给初值 96，理由：
  body 12.5px 中文 4 字 ≈ 50px + icon ≈ 16px + 两侧 padding 24px ≈ 90px，取整 96）。
- `main.rs` L1030 工具栏 Fetch 按钮：`.min_width(104.0)`——「加载中…」同 4 字，
  但 `ButtonSize::Lg` 高度 40px、padding 更大，初值 104，实现时实测。
- `sepa.rs` L387 空态「刷新」按钮：文本固定为「刷新」，无 loading 切换，
  **不需要** min_width（保持内容自适应即可）；为与工具栏对齐也可加 `.min_width(96.0)`，
  但建议不加——空态按钮独立居中，不参与工具栏对齐。

**根因调查（实现前置步骤，用户确认 2026-08-09：需先查根因，怀疑被其他 UI 遮挡）**：
本方案不依赖根因即可修复观感，但用户明确要求**先锁定根因再实现**。实现阶段
第一步必须用客观断言（非视觉猜测）确认「宽度未变」的真实机制：

1. **断言 rect 是否真的变化**：kittest 打开 popup 前后分别取 `response.rect.width()`——
   若 loading 后宽度确实变大 → 根因是「视觉遮挡/观感」（遮罩压暗、spinner 固定右缘
   掩盖变化），min_width 是治本；若宽度真的没变 → 根因是「布局钳制/缓存」，继续排查。
2. **遮挡检查**：验证是否有其他元素覆盖按钮——accesskit 树检查按钮节点 bounds 与
   z-order，或像素采样确认按钮边缘是否被遮罩/popup Area/后续 widget 覆盖。
3. **根因锁定后处理**：若确实被遮挡 → 修复遮挡本身（遮罩绘制时机/范围）；min_width
   作为宽度一致性加固，两者都做。若根因是布局钳制 → 单独排查 `ui.horizontal` 外层
   `available_rect_before_wrap` 与 egui 尺寸缓存，修根因 + min_width 兜底。
4. 调查结论与证据（断言输出）写入实现 commit message 与 reflections。

## 交互效果

| 触发 | 现状 | 修复后 |
|---|---|---|
| light 主题渲染 Primary 按钮 | 蓝底黑字，对比 3.19:1 看不清 | 蓝底白字，对比 4.90:1 达标 |
| light 主题渲染 Danger 按钮 | 红底黑字，对比 3.28:1 看不清 | 红底白字，对比 4.77:1 达标 |
| dark 主题渲染 Primary/Danger | 浅灰字（维持现状，用户已验收） | 白字，对比从 3.30/2.35 提升至 4.90/3.48，更清晰 |
| hover Primary（light） | 蓝亮底黑字 | 白字在 #4D7FFF 上 3.62:1，hover 瞬时态可接受 |
| pressed Primary（light） | — | 白字在 #1E4FD6 上 6.68:1，达标 |
| SEPA 刷新按钮点击 | 文本「刷新」→「计算中…」，宽度可能跳变/观感不变 | 两态宽度一致（min 96px），遮罩与 spinner 位置稳定 |
| Fetch 按钮点击 | 文本「Fetch」→「加载中…」，同上 | 两态宽度一致（min 104px） |
| loading 文字色 | 保留变体色（现状） | 保留变体色（现为 on_accent/on_error），行为不变 |

无新增动画/过渡——本修复是静态视觉与布局一致性修复，不引入动效。

## 影响面分析

### 组件层（compass-ui）

| 文件 | 改动 | 测试影响 |
|---|---|---|
| `tokens/color.rs` | `ColorTokens` 新增 `on_accent`/`on_error` 字段 + dark()/light() 初始化 | `dark_palette_matches_design_spec` / `light_palette_matches_design_spec` 各加 2 条断言 |
| `widgets/button.rs` | `variant_colors()` 两变体文字色换 token；新增 `min_width` 字段 + builder + 应用到 `min_size` | `variant_colors_follow_design`（L224/228 断言文字色改 on_*）；`loading_button_keeps_variant_text_color`（断言改 on_accent）；新增 `min_width_keeps_loading_width_stable` 测试 |

### 调用方（crates/compass）

| 文件 | 位置 | 改动 |
|---|---|---|
| `main.rs` | L1030 Fetch 按钮 | `.min_width(104.0)` |
| `sepa.rs` | L276 工具栏刷新 | `.min_width(96.0)` |
| `sepa.rs` | L387 空态刷新 | 不加（可选） |
| `screener.rs` | L249 筛选按钮 | **无改动**——无 loading 文本切换，Primary 文字色由组件统一变化，视觉自动修复 |

> 所有使用 Primary/Danger 的按钮（Modal Confirm、EmptyState action、MultiSelect
> 完成等）**无需代码改动**，文字色由组件层统一修复；仅 loading 文本切换的两处
> （Fetch/SEPA）需要 min_width。

### 权威文档同步

| 文档 | 条目 | 同步内容 |
|---|---|---|
| `kb/design/ui.md` | 决策记录 L261（Button 文字主题色） | 更新为：Primary/Danger 用 `on_accent`/`on_error`（theme-aware，替代统一 text_primary）；追加「最小宽度策略」决策行 |
| `kb/design/ui.md` | 决策记录 L262（loading 文字色） | 补充 loading 保留变体色（on_* token） |
| `kb/design/ui-widgets.md` | Button 条目 L119/L121 | 变体文字色描述从「全部 text_primary」改为「Default/Ghost text_primary；Primary/Danger on_accent/on_error」；API 要点补 `.min_width()` |
| `kb/design/ui-widgets.md` | Button 条目 L147 测试锚点 | 补充 min_width 新测试名 |

## 待确认（已定稿）

1. ~~dark 主题 on_* 是否用冷白~~ → **已确认（2026-08-09）：两主题均纯白 `#FFFFFF`**
   （选项 A）。冷白 #F2F4F8 对比度 4.45:1 略低于 AA 阈值，排除。
2. ~~min_width 初值~~ → **已确认：96（SEPA）/ 104（Fetch）为估算初值，实现阶段按
   `tokens.typography.body` 实测微调。**
3. ~~空态刷新按钮是否加 min_width~~ → **已确认：不加**（文本固定「刷新」无 loading
   切换，内容自适应；空态独立居中不参与工具栏对齐）。若未来要求两按钮视觉一致再议。
4. ~~根因 b/c 是否单独修~~ → **已确认：先查根因（用户怀疑被其他 UI 遮挡），
   根因锁定后再修**（见「根因调查」章节）。min_width 保留为宽度一致性加固，
   不作为掩盖根因的唯一手段。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| Primary/Danger 文字色来源 | 维持统一 `text_primary` / 硬编码白 / **新增 `on_accent`/`on_error` token** | 新增 on_* token | light 下 text_primary 深字在 accent/error 底上对比 3.19/3.28:1 不达标；白字 4.90/4.77:1 达标；token 由主题派生满足 ref #217「跟随主题」精神 | text_primary 无法区分「普通浅底前景」与「彩色实底前景」两种语义，light 无解；硬编码白回归 ref #217 缺陷 |
| `on_accent` 两主题取值 | dark 白 / light 白（同值）/ dark 冷白 / light 白（异值） | 两主题同值纯白 | accent 本身两主题同色，其上对比前景同色逻辑自洽；token 体系仍保证「由主题派生、可独立演进」 | 异值制造无意义的视觉差异，且验收精神靠 token 机制而非色值差保证 |
| `on_error` dark 取值 | 白字 / 维持浅灰 / 深色字 | 白字 | dark error #EF5350 上白字 3.48:1 优于浅灰 2.35:1，显著提升；与 on_accent 同风格 | 深色字在亮红底上虽对比更高但视觉违和（暗红底深字不可读）；浅灰维持现状不解决可读性 |
| loading 文字色 | 维持变体色 / 降 text_disabled | 维持变体色（on_*） | 现状已验收（ref #217：spinner+遮罩表达 busy，转灰在彩色底上不可见）；token 替换不改变行为 | 降灰在 accent 底上对比不足，回归已验收缺陷 |
| SEPA/Fetch 宽度策略 | 跟随文本增长 / 固定宽度 / **最小宽度 min_width** | 最小宽度 | 两态宽度一致防跳变；min 值取较宽文本，短文本补足、长文本仍自适应；不动 horizontal 布局、不依赖根因定位 | 跟随增长导致 right_to_left 布局向左挤压抖动；固定宽度在「刷新」态右侧留白大、与工具栏不对齐 |
| min_width 数值 | 精确 glyph 测量 / **估算初值 + 实现实测** | 估算初值 96/104 + 实现实测 | body 12.5px 中文 4 字 + icon + padding 约 90px；Lg 按钮 padding 更大取 104 | 纯测量需实现阶段跑字体渲染，设计阶段无法精确；估算不影响方案结构 |
| 空态刷新按钮 | 加 min_width / 不加 | 不加 | 空态按钮文本固定、独立居中，无 loading 切换需求；保持内容自适应 | 加 min_width 引入无收益的固定宽，且空态布局不参与工具栏对齐 |

