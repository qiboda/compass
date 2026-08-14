# Chart 日期中文格式 — 视觉设计方案

> 归档：`.omo/designs/ui-fixes-chinese-date.md`（过程归档，非权威）
> 权威文档：`kb/design/ui.md`（经用户确认后由主 agent 同步最终要点）
> 日期：2026-08-09
> 关联：epic #217 / sub-issue #219

## 目标

1. K 线图 **x 轴刻度**、**十字光标**、**tooltip** 三处日期显示全部改中文，
   消除英文缩写月（"Jan 15"/"Jun"）。
2. 格式分两档（已锁定）：**x 轴紧凑式**（`1月` / `5月15日` / `2024`）、
   **十字光标与 tooltip 完整式**（`2024年5月15日`）。
3. 定义三类刻度场景（Year / Month / DayOfMonth）在**日线 / 周线 / 月线**
   下的中文紧凑格式规则，明确跨年窗口下的歧义消解策略。
4. 不改变刻度生成逻辑、不改变字体策略、不引入新依赖；改动全部落在
   vendored egui-charts fork（`/data/codes/compass-project/egui-charts`，
   branch `compass`，compass 以 git 依赖引用：`Cargo.toml` L20）。

## 现状

### 渲染链路（vendored fork，三处硬编码）

| 位置 | 文件:行 | 现格式（chrono） | 示例 |
|---|---|---|---|
| x 轴刻度 | `src/scales/time_formatter.rs` L59/L61（`DefaultTimeFormatter::format`） | `Year → "%Y"`、`Month → "%b"`、`DayOfMonth → "%b %d"` | 2024 / Jun / Jun 15 |
| 十字光标 | `src/chart/renderers/crosshair.rs` L218/L221 | 小时档 `"%b %d %H:%M"`、日线+档 `"%b %d, %Y"` | Jan 15 14:30 / Jan 15, 2024 |
| tooltip | `src/chart/renderers/tooltip.rs` L57（Floating `"Time: {}"` 行）L182（Tracking 时间段） | `"%Y-%m-%d %H:%M:%S"` / `"%H:%M:%S"` | 2024-05-15 00:00:00 / 00:00:00 |

### 刻度类型如何产生（确定 mark 的归属逻辑）

`src/scales/timescale_marks.rs`：

- 主刻度类型由**标记间隔**决定（`determine_mark_type_and_weight` L303-321）：
  <1s TimeWithSeconds；<1h Time；<24h Time；<30d `DayOfMonth`；<365d `Month`；
  其余 `Year`。
- 每个候选标记按边界升级（`classify_time_boundary` L393+）：1月1日 → `Year`
  （权重 YEAR，最高），每月 1 日 → `Month`，其余日 → `DayOfMonth`。
- 标签经 `format_time_label` → `formatter.format(time, mark_type)` 输出
  （L482-483）；formatter 由 `chart/renderers/labels.rs` L118 经
  `TickMarkGenerator::with_formatter` 注入，实例在 `src/widget/mod.rs`
  L1221-1231 由 `TimeFormatterBuilder`（默认 `DefaultTimeFormatter`）构造。

因此**改 `DefaultTimeFormatter::format` 一处即可覆盖全部 x 轴刻度**；
十字光标与 tooltip 的格式串各自独立硬编码，需分别修改。

### 各 time frame 下的实际刻度构成

compass 工具栏周期 `1d | 1w | 1M`（`chart.rs` L114 仅设 `set_timeframe_label`，
不改变刻度逻辑），默认可见 100 根柱：

| time frame | 100 根 ≈ 跨度 | 主刻度类型 | 边界升级 |
|---|---|---|---|
| 日线 1d | ~4–5 个月 | DayOfMonth | 月初→Month；窗口内通常无年首→Year |
| 周线 1w | ~2 年 | DayOfMonth 或 Month（取决于标记间隔是否 ≥30 天） | 年首→Year（年锚点每 ~12–16 根一根） |
| 月线 1M | ~8–9 年 | Month | 年首→Year（年锚点密集） |

### 相关既有约束

- 字体：compass 内嵌 **SourceHanSansCN**（中文）+ JetBrains Mono（数字）；
  fork 刻度/十字光标标签用 `FontId::proportional`（`labels.rs` L74、
  `crosshair.rs` L156/L234）——中文经思源渲染，无需改字体（`kb/design/ui.md`
  字体章节；egui 无字符级 fallback，不引入混排）。
- 涨跌色/A 股惯例：日期非涨跌语义，不涉及 token 颜色。
- 默认 tooltip 配置：`TooltipMode::Floating`、`show_time: true`
  （`src/config/tooltip.rs` L107-116）；compass 未覆写（`chart.rs` 无 tooltip
  配置）——Floating 时间行当前显示 `Time: 2024-05-15 00:00:00`。

## 设计方案

### 1. x 轴紧凑式格式（按 TickMarkType 固定映射）【设计决策】

| TickMarkType | 现格式 | 新格式（chrono） | 示例 |
|---|---|---|---|
| `Year` | `%Y` | `%Y`（不变） | 2024 |
| `Month` | `%b` | `%-m月` | 6月 |
| `DayOfMonth` | `%b %d` | `%-m月%-d日` | 6月15日 |
| `Time` | `%H:%M` | 不变 | 14:30 |
| `TimeWithSeconds` | `%H:%M:%S` | 不变 | 14:30:45 |

> **格式锁定（Metis B1 修正，2026-08-09）**：chrono 0.4.45 的 `%m`/`%d` 是**零填充**（输出 `06月`/`06月15日`），与中文习惯不符。**所有月/日格式串统一用 `%-m`/`%-d`（去填充修饰符）**，输出 `6月`/`6月15日`/`2024年5月15日`。零填充断言（`06月`）为回归测试目标。

规则要点：

- **数字用半角阿拉伯数字 + 汉字单位**（`6月15日`，非 `六月十五日`）：与
  价格/代码等数据值的半角数字视觉一致；同花顺/东财 A 股终端惯例。
- **月初标记自然呈现为 `6月`**：`classify_time_boundary` 把每月 1 日升级为
  Month 类型 → 显示 `6月`（隐含 6月1日），非月初的日标记显示 `6月15日`。
  格式串层面无需特判。
- 纯数字 `2024` 不加「年」字：紧凑式优先；相邻 Month 标签（`6月`）自带
  「月」单位，无混淆；用户锁定示例即 `2024`。

### 2. 跨 time frame 的刻度构成与歧义消解【设计决策】

| time frame | x 轴标签序列（示意） | 年份信息从哪来 |
|---|---|---|
| 日线 1d | `6月` `6月15日` `7月1日` …（约 4–5 个月跨度） | 窗口内通常无 `2024` 锚点 → 由十字光标/tooltip 完整式兜底 |
| 周线 1w | `2024` `2月` `3月` `4月` … 或 `1月2日` `1月16日` … | 年首 `2024` 每 ~12–16 根一根，锚定年份 |
| 月线 1M | `2024` `2月` `3月` … `2025` `1月` … | 年首 `2024`/`2025` 密集出现 |

**歧义消解策略（有意为之）**：紧凑式标签**刻意不含年份**——x 轴承担
概览（密度优先），精确日期由完整式（十字光标悬停值 / tooltip）承担。
两条信息通道互补，这正是「紧凑式 x 轴 + 完整式十字光标/tooltip」双层
设计的价值：不做「跨年窗口内 DayOfMonth 自动加年」这类动态格式切换
（复杂度高、格式不稳定、破坏刻度一致性）。

### 3. 十字光标与 tooltip 完整式格式【设计决策】

统一按 **bar 周期分档**（与 crosshair 现有 L200-225 分档逻辑对齐，
compass 当前只用日/周/月线，fork 保留盘中场景通用性）：

| bar 周期 | 现格式 | 新格式（chrono） | 示例 |
|---|---|---|---|
| 子秒 | `%H:%M:%S%.3f` | 不变 | 14:30:45.123 |
| 秒 | `%H:%M:%S` | 不变 | 14:30:45 |
| 分钟 | `%H:%M` | 不变 | 14:30 |
| 小时 | `%b %d %H:%M` | `%-m月%-d日 %H:%M` | 5月15日 14:30 |
| 日线及以上（日/周/月） | `%b %d, %Y` | `%Y年%-m月%-d日` | 2024年5月15日 |

- **十字光标**（`crosshair.rs` L218/L221）：仅替换两档格式串。
- **tooltip Floating 时间行**（`tooltip.rs` L57）：日线+ → `2024年5月15日`
  （纯日期）；盘中 → `5月15日 14:30:45`。**效果**：日线 bar time 恒为
  当天 00:00:00，分档后不再显示无意义的尾随时间。
- **tooltip Tracking 时间段**（`tooltip.rs` L182）：同规则分档。
- 日线+ 档含年份（`2024年5月15日`）而 x 轴紧凑式不含——完整式是精确
  信息通道，必须携带年份（用户锁定示例）。

### 4. 实施位置与影响面【实现细节，非设计决策】

| 修改点 | 内容 |
|---|---|
| `time_formatter.rs` L59/L61 | Month → `%-m月`；DayOfMonth → `%-m月%-d日`（`%-` 去填充，避免零填充 "06月"） |
| `crosshair.rs` L218/L221 | 两档格式串替换（`%-m月%-d日 %H:%M` / `%Y年%-m月%-d日`） |
| `tooltip.rs` L57/L182 | 时间行按 bar 周期分档；需把 bar_duration_ms 传入 Floating/Tracking renderer（可从 `visible_data` 用中位数法推导，同 `labels.rs` L90-103 现有逻辑）；前缀 7 个中文化（`Time:`→`时间:`、`Open:`→`开盘:`、`High:`→`最高:`、`Low:`→`最低:`、`Close:`→`收盘:`、`Volume:`→`成交量:`、`Change:`→`涨跌:`，L75 `Change:` 默认 show_change=true 必然显示）+ tracking 缩写中文化（`O:/H:/L:/C:/Vol:`→`开:/高:/低:/收:/量:`，L186-193） |
| 测试同步 | `time_formatter.rs` L372（`"Jun"` → `"6月"`）、`timescale_marks.rs` L644-669（`"Jun"`/`"Jun 15"` → `"6月"`/`"6月15日"`）；**新增 crosshair/tooltip 格式断言测试**（两文件现无 mod tests，验收标准"2024年5月15日"必须有测试覆盖） |
| 不动 | `classify_time_boundary`/`determine_mark_type_and_weight`（刻度生成）；`Time`/`TimeWithSeconds` 档；`labels.rs` 字体与 min_spacing；`widget/mod.rs` formatter 装配 |

备选（已否决）：tooltip 时间行统一 `%Y年%-m月%-d日 %H:%M:%S`
（不区分盘中/日线）——代价是日线显示 `2024年5月15日 00:00:00`。**用户已
确认分档方案**（2026-08-09），与 crosshair 行为一致。

### 5. 字体与宽度预算【设计决策 + 客观校验】

- 标签字体保持 `FontId::proportional`（思源渲染中文），**不改字体策略**；
  数字在 CJK 字体下为半宽字形，宽度预算（body 12.5px，CJK 全宽 ≈12.5px、
  半宽数字 ≈6px/字符）：
  - `6月15日` ≈ 3×6 + 2×12.5 ≈ 43px（现 `Jun 15` ≈ 36px，+20%）
  - `2024年5月15日` ≈ 4×6 + 3×12.5 ≈ 61.5px（现 `Jun 15, 2024` ≈ 55px，+12%）
  - x 轴 `min_spacing` 60–140px（`labels.rs` L106）> 43px 标签宽 → 相邻
    刻度不重叠；十字光标标签宽度变化不影响布局（底边居中单标签）。
- 结论：**无重叠/溢出风险**，无需调整刻度密度参数。

## 交互效果

| 场景 | 触发 | 显示变化 |
|---|---|---|
| x 轴刻度（任何 time frame） | 数据加载/平移/缩放 | 英文月缩写 → 中文紧凑式（`6月` / `6月15日` / `2024`）；跨年窗口出现 `2024` 年锚点 |
| 十字光标（hover） | 光标悬停 K 线 | 底边时间标签：日线+ → `2024年5月15日`；盘中 → `5月15日 14:30` |
| tooltip Floating | 光标悬停 K 线 | 时间行 `Time: 2024年5月15日`（日线+，无尾随 00:00:00） |
| tooltip Tracking | 切换 Tracking 模式 | 时间段 `2024年5月15日`（日线+） |

无动画/过渡/快捷键变化；纯格式替换，交互行为零改动。

## 待确认（已全部确认，2026-08-09）

1. ~~**tooltip 时间行分档 vs 统一格式**~~：**用户确认分档**（日线+ 纯日期，无
   `00:00:00`），需给 Floating/Tracking renderer 传 bar_duration 参数。
2. ~~**tooltip 的 `Time:`/`Open:`/`High:` 等英文标签前缀**~~：**用户确认一并
   中文化**（时间:/开盘:/最高:/最低:/收盘:/成交量:/涨跌:），范围扩大至 tooltip
   全部 7 个前缀 + tracking 缩写（开:/高:/低:/收:/量:）；tooltip 以外的 GUI 文本
   归 #222（GUI 全面中文化）。
3. **日线跨年窗口**（如用户 fetch 超 1 年日线数据时，x 轴出现 `2024` 年
   锚点）：紧凑式默认不含年份，接受由十字光标兜底——**确认接受此权衡**。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| x 轴紧凑式格式（锁定） | `6月15日` / `15日` / `6/15` / `Jun 15` | `%-m月%-d日` + 汉字单位 | 用户 grill-me 锁定示例（1月/5月15日/2024）；A 股终端（同花顺/东财）惯例；汉字单位消解「月/日/年」歧义 | 纯数字 `6/15` 在月/年刻度混排时无单位歧义风险更高；保留英文违背 issue 目标 |
| Month 刻度格式（锁定） | `6月` / `06` / `Jun` | `%-m月` | 月初边界隐含 6月1日，与日标记 `6月15日` 视觉同构；锁定示例即 `1月` | 纯数字 `06` 与价格刻度混淆 |
| Year 刻度格式（锁定） | `2024` / `2024年` | `%Y` 纯数字 | 锁定示例；相邻 Month 带「月」单位无混淆；紧凑式优先 | 加「年」字增加宽度且无信息增益 |
| 紧凑式是否含年份 | 跨年窗口 DayOfMonth 动态加年 / 固定不含年 | **固定不含年**，靠年首 Year 锚点 + 完整式兜底 | 格式稳定一致、实现简单；x 轴=概览、十字光标/tooltip=精确，双层互补 | 动态切换格式随窗口漂移、破坏刻度一致性、实现复杂度高 |
| 完整式年份粒度 | 日线+ 含年（`2024年5月15日`）/ 不含年 | 含年 | 完整式是精确通道，必须携带年份（锁定示例）；日线窗口内 x 轴无年锚点，完整式是唯一年份来源 | 不含年则完整式信息不足 |
| 完整式是否分档 | 按 bar 周期分档（日线+ 纯日期）/ 统一 `%Y年%-m月%-d日 %H:%M:%S` | 分档（用户确认） | 与 crosshair 现有 L200-225 分档逻辑一致；日线 bar time 恒 00:00:00，分档消除无意义尾随时间 | 统一格式需传参更多（实现细节层面分档也只多传一个 duration） |
| 标签字体 | 保持 proportional（思源）/ 数字段混排 JetBrains Mono | 保持 proportional | egui 无字符级 fallback，混排需自定义 FontFamily 成本高；刻度标签无需列对齐（非表格场景）；宽度预算验证无重叠 | 数字等宽混排在 x 轴无对齐收益，徒增字体复杂度 |
| 改动落点 | fork 三处硬编码 / compass 侧 formatter 覆写 | fork 三处 | x 轴 formatter 本就在 fork 装配（`widget/mod.rs` L1221）；crosshair/tooltip 格式串 fork 独有，compass 无覆写通道 | compass 侧覆写只能覆盖 x 轴，crosshair/tooltip 无法经公开 API 定制（`%b` 硬编码），仍需 fork 改动 |
| tooltip 英文标签前缀 | 本次一并中文化 / 保持现状另立 issue | **一并中文化**（用户确认 2026-08-09） | 用户明确选择完整中文化体验；7 前缀 + tracking 缩写全部处理 | 保持现状会留 "Time: 2024年5月15日" 混排 |
| 格式化串零填充 | `%m`/`%d`（零填充）vs `%-m`/`%-d`（去填充） | **`%-m`/`%-d`**（Metis B1 修正锁定） | chrono `%m` 输出 "06月" 与中文习惯不符；`%-` 去填充输出 "6月" | 零填充与设计示例（6月）矛盾，RED 测试会失败 |

> 全部待确认项已由用户确认（2026-08-09），最终要点同步至 `kb/design/ui.md`。

> 待确认项 1/2/3 经用户确认后，主 agent 将最终要点同步至 `kb/design/ui.md`。
