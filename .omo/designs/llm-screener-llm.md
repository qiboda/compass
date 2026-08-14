# 自然语言 LLM 生成选股条件 UI（Epic #243 Batch 4，issue #247）

> **归档文档**：本文件是 ui-designer 产出的**过程归档**，非权威。经用户确认后，
> 最终设计要点同步至 `kb/design/ui.md`（权威）与 `kb/user/gui.md`（用户手册选股器章节）、
> `kb/user/config.md`（`[llm]` 配置节）。本文件不删不改。

---

## 目标

在选股器（#245 可视化条件构建器之上）增加**自然语言 → Filter AST** 的轻量入口：

1. 用户用自然语言描述选股意图（如「最近连续5天每天涨超3%、市值大于100亿的股票」），
   点击「AI 生成」→ LLM 输出 Filter AST JSON → serde 解析 + 语义校验。
2. 生成成功 → 以**条件卡片**形式并入 #245 构建器的根组（可编辑、可删除、可随
   「筛选」持久化），与手动卡片完全同构，无第二套数据模型。
3. 校验失败 / 网络失败 → 输入区下方**内联错误提示** + Error toast，不崩溃、不丢输入。
4. API key 未配置 → **隐藏整个 LLM 入口**（无占位、无禁用态，干净回退）。
5. LLM 入口是**辅助功能**：不引入模态、不新增组件、不重排构建器结构。

---

## 现状

| 项目 | 现状 | 位置 |
|---|---|---|
| 条件构建器 | 根组 Card「筛选条件」：组头 [Segmented 且/或 + Badge + 清空] → 卡片列表/空态 → 组底添加菜单；`builder_root: Vec<CondItem>` 为 UI 唯一真相 | `crates/compass/src/citizens/screener.rs` `condition_builder()` L264-304 |
| 视图模型 | `filter_to_items(&Filter) -> Vec<CondItem>` 反向识别（**含 `LeafKind::Unknown` 只读摘要卡兜底**——Batch 4 LLM 产物天然可渲染）；`group_to_filter` 正向构建 | `crates/compass/src/citizens/screener_builder.rs` L139-153 / L424-436 |
| 信号模式 | 每条后端通道 = `AsyncDispatcher<Req, Resp>`（backend.rs `attach_async`）+ 响应 handler 写 `SharedState` 三件套（`*_loading` / `*_error` / 结果）；错误 toast 在 main.rs 按 **None→Some 迁移**推送 | `crates/compass/src/backend.rs` L139-192；`crates/compass/src/main.rs` L863-870 |
| 组件 | `Input`（placeholder/前缀图标/focus accent 描边）、`Button`（loading 内嵌 spinner + 禁用）、`colored_label(error_fg_color)` 错误惯例 | `crates/compass-ui/src/widgets/input.rs`；SEPA 刷新按钮先例 |
| LLM 输出目标 | `Filter` serde tagged-union JSON：`{"Meta":{...}}` / `{"Series":{...}}` / `{"And":[...]}` / `{"Or":[...]}` / `{"Not":{...}}`；`CmpOp` snake_case；`FactorRef::{Const, Factor}` | `crates/compass-types/src/screener.rs` L17-210 |
| config | **无 `[llm]` 节**（本批引入）：`base_url` / `api_key` / `model`；key 缺失 → 隐藏入口 | `kb/user/config.md`（当前无此节） |

---

## 设计方案

### 1. 布局：LLM 入口行（构建器根组 Card 内、组头之下）

```
┌─ Card「筛选条件」──────────────────────────────────────────────┐
│  组头行: [Segmented 且(AND)|或(OR)]  Badge(条件数)  [清空] ──── │
│                                                              │
│  · LLM 行（llm_enabled=true 时渲染，否则整行不出现）:            │
│    [⚡ 用自然语言描述选股条件…           ]  [✨ AI 生成]         │
│    （生成中：Input 禁用 + 按钮内嵌 spinner「生成中…」）           │
│  · 错误行（llm_error 为 Some 时）:                              │
│    ⚠ colored_label(error_fg_color) 生成失败文案                 │
│                                                              │
│  · 卡片列表 / 空态（与现状完全一致）                             │
│  · 组底: [＋ 添加条件 ▾]                                       │
└──────────────────────────────────────────────────────────────┘
[筛选]（Primary，位置与现状一致）
```

- **位置**：根组 Card 内、组头行与卡片列表之间（`render_root_header` 之后、
  EmptyState/列表之前）。理由：语义关联最强——「在此描述，卡片出现在下方」；
  零新增容器（轻量）；`llm_enabled=false` 时整行不渲染，Card 回到 #245 原貌。
  清空按钮天然覆盖 LLM 生成的卡片（同一根组，一致心智）。
- **行组成**：`ui.horizontal` 原子组（沿用 ref #220 原子组惯例，窄窗口整组换行）：
  1. `Input::new(tokens, &mut llm_input)`——placeholder 短提示
     「用自然语言描述选股条件…」（`screener.llm.placeholder`），前缀图标
     `egui_phosphor::regular::LIGHTNING`，宽度 = 可用宽 − 按钮宽 − `spacing.md`；
  2. `Button::new(...).variant(Primary).icon(SPARKLE)`「AI 生成」
     （`screener.llm.generate`）——Primary 与「筛选」一致，表达这是主生成动作。
- **垂直间距**：LLM 行与组头 `spacing.sm`，与卡片列表 `spacing.sm`（现状行距习惯）。
- **`llm_enabled` 来源**：启动时 `config.llm.api_key.is_some()`（key 缺失 = 入口隐藏，
  需求 4），作为 `ScreenerPanel::new` 新增构造参数传入（静态标志，每帧零开销，
  不污染 SharedState）。

### 2. 生成结果合并：append + AND 展平（纯函数）

成功响应的 `Filter` 经 `filter_to_items` 反向识别后**并入根组**，规则（纯函数，
单测可精确断言）：

```
fn llm_merge_into_root(root_items: &mut Vec<CondItem>, root_op: BoolOp, gen: Filter):
  items = filter_to_items(&gen)
  if root_op == And && items == [CondItem::Group(g)] && g.operator == And:
      root_items.extend(g.items)      # AND 结合律：And[And[a,b], 默认卡…] == And[a,b,默认卡…]
  else:
      root_items.extend(items)        # 裸叶子直接平铺；Or/Not/嵌套保持子组
```

- **常见案例干净**：「A 且 B 且 C」生成 `And[a,b,c]` → 平铺为三张根级卡，与默认
  6 张基础卡并排，视觉是「条件列表被追加」，无多余嵌套。
- **复杂案例正确**：生成 `Or[...]` / `Not(...)` / 深层嵌套 → 保持为子组（根组为
  And 时语义上本就需要子组承载）。
- **不替换、不删除**用户已有条件（非破坏性）；**不去重**——生成同 kind 卡片与
  现有卡并存（交集语义等价，多余可手动删除；Metabase 同惯例，轻量优先）。
- **空结果**：语义校验阶段由后端拒绝（空 `And`/`Or` → 解析错误文案），GUI 侧
  不会遇到「生成了个寂寞」。
- **未知形状**：若 LLM 产出模板外的 AST（如 `Count`），`filter_to_items` 的
  `Unknown` 只读摘要卡兜底——可删除、可随「筛选」发送（#246 引擎求值任意 AST），
  **不崩溃**（#245 已锁定的健壮性设计，此处直接复用）。
- 合并后卡片参与 `build_filter()` → 点击「筛选」→ `RunScreenerRequest{filter}`，
  `on_save` 持久化——**整条链路零改动**。

### 3. 状态管理：SharedState 集成（第五通道）

SharedState 新增 5 个 `Dynamic` 字段（镜像现有 `*_loading/*_error` 三件套惯例）：

| 字段 | 类型 | 职责 |
|---|---|---|
| `llm_input` | `Dynamic<String>` | 提示词文本缓冲——**放 SharedState**：切 tab/重建面板草稿不丢 |
| `llm_loading` | `Dynamic<bool>` | 生成中（驱动按钮 loading + 禁用） |
| `llm_error` | `Dynamic<Option<String>>` | 最近一次生成错误（None→Some 迁移 → Error toast） |
| `llm_result` | `Dynamic<Option<Filter>>` | 待消费的生成结果（backend handler 写入，面板消费后 `set(None)`） |
| `llm_seq` | `Dynamic<u64>` | 请求序号（Esc 取消守卫，见 §5） |

消息通道（`messages.rs`，第五通道，与 `RunScreenerRequest/Response` 同构）：

```rust
pub struct RunLlmRequest { pub prompt: String, pub seq: u64 }
pub struct RunLlmResponse { pub filter: Option<Filter>, pub error: Option<String>, pub seq: u64 }
```

backend.rs：`AsyncDispatcher::<RunLlmRequest, RunLlmResponse>` + `attach_async`
（handler = LLM 客户端调用 + JSON 提取 + serde 解析 + 语义校验，见 §4）。
响应 handler 中 **seq 守卫**：`resp.seq != llm_seq.get()` → 直接丢弃（不写任何状态）；
匹配 → 写 `llm_result` / `llm_error` / `llm_loading=false`。

面板消费（`show()` 每帧，置于条件构建器渲染前）：

```
if !llm_loading.get():
    if let Some(gen) = llm_result.get():
        llm_merge_into_root(&mut builder_root, builder_root_operator, gen)
        llm_result.set(None)
```

- `llm_loading` 为 true 时不消费——新请求在途时绝不让旧结果混入（双保险）。
- 发送：面板持 `llm_signal: &Signal<RunLlmRequest>`（`show()` 新增参数，
  与 `run_screener_signal` 并列，main.rs/tabs.rs 接线）。

### 4. 校验管线（后端契约，GUI 只显示结果）

```
prompt → OpenAI 兼容 chat completions（prompt 内嵌 Filter AST schema + 示例，约束 JSON-only）
  → 原始文本 → 提取 JSON（容错 markdown fence 包裹）→ serde 反序列化 Filter
  → 语义校验（拒绝：空 And/Or、嵌套深度 > 8、window ≤ 0、at_least > window、非法枚举等）
  → Ok(Filter) / Err(已翻译为 i18n 文本的错误消息)
```

- 错误消息**由后端翻译**（rust-i18n `t!()` 可用，`error.parquet_open` 先例）——
  GUI 侧哑显示 `llm_error`，不匹配错误类型、不本地化。
- 错误分两类文案：网络/服务失败（`screener.llm.error_network`）vs 输出无法解析
  （`screener.llm.error_parse`，提示「换一种说法或手动添加条件」）。
- 无 key 防御：即使入口被隐藏，若请求仍被发出（理论不可能），后端返回
  `error_network` 类文案——不崩溃。

### 5. 交互设计

| 操作 | 交互 | 说明 |
|---|---|---|
| 提交 | 点「AI 生成」或 **Enter**（输入框聚焦时 `key_pressed(Enter)`） | 空/纯空白输入时按钮禁用（不发无意义请求）；`seq` 自增后发送，`llm_loading=true` |
| 生成中 | 按钮 loading（内嵌 spinner + 禁用，SEPA 刷新先例）；**Input 禁用**（在途请求携带的是发送时的文本） | Enter 在途无效；不允许多重提交 |
| **Esc 取消** | 生成中按 Esc → `llm_seq` 自增（作废在途请求）+ `llm_loading=false` + `llm_error=None` | 在途响应到达时 seq 不匹配 → 静默丢弃，绝不把「已取消的卡片」混入根组（seq 守卫是取消安全性的关键） |
| 成功 | 卡片即时出现在根组（瞬时，无动画）；**输入文本保留**（便于微调措辞再生成）；无 success toast（卡片出现即反馈） | 不打断「描述→生成→微调→再生成」迭代 |
| 失败 | 输入区下方内联 `colored_label(error_fg_color)` + main.rs None→Some 迁移推 Error toast；**输入文本保留**，用户可直接改描述重试 | 内联 = 上下文可操作；toast = 全局注意（screener_error 同款双通道） |
| 结果卡片 | 完全等同手动卡片：可编辑参数、可删除、可取反、可随筛选持久化 | 无「AI 生成物」特殊标记——它就是条件列表的一员 |
| 无 key | 整行不渲染（需求 4），无占位、无禁用态、无 tooltip | 干净回退 |
| 输入框内 Esc（空闲时） | 仅失焦，**不清空文本** | 防误触丢草稿；「清空」用选中删除即可，不为辅助功能加清除按钮 |

### 6. i18n 键（`screener.llm.*` 前缀，zh/en 对称）

```yaml
screener:
  llm:
    placeholder: 用自然语言描述选股条件…        # en: Describe conditions in plain language…
    generate: AI 生成                          # en: AI Generate
    generating: 生成中…                         # en: Generating…
    error_network: 生成失败：无法连接 AI 服务，请稍后重试   # en: Generation failed: AI service unreachable, retry later
    error_parse: 生成失败：未能理解描述，请换一种说法，或手动添加条件  # en: Generation failed: couldn't parse the description; rephrase it or add conditions manually
```

- 沿用 `screener.builder.*` 的模块子命名空间惯例（ref #222）。
- 复用现有键：`common.*`、`error.*` 样式不变；无新增组件键。

### 7. 可测试性锚点

- **纯函数层**（单测）：
  - `llm_merge_into_root`：And 展平（`And[a,b]` → 两张根级卡）、裸叶子平铺、
    Or 保持子组、Not 保持子组、与根组 Or 算子交互、空 items 不 panic。
  - seq 守卫逻辑：匹配消费 / 不匹配丢弃 / Esc 后旧响应被弃。
- **kittest 集成层**（`LANG_LOCK` + `Harness::new_ui` 先例）：
  - label 锚点：Input 用 placeholder 查询（`get_by(|n| n.placeholder()==...)`，
    Input 组件测试先例）；按钮「AI 生成」`get_by_label`；loading 态「生成中…」；
    错误行 `get_by_label_contains("生成失败")`。
  - 交互路径：输入 → 点生成 → 断言 `llm_loading` + 按钮禁用 → 注入响应 →
    卡片数量 +N、输入保留；失败 → 错误行出现、输入保留、根组不变；Esc → 
    `llm_loading=false` + 延迟到达的旧响应不混入根组；`llm_enabled=false` →
    placeholder 查询计数为 0（入口隐藏）。
- **后端**（stub HTTP，门禁 3.5/4 步已列）：成功 / 非 JSON / 字段缺失 / 超深度 /
  网络超时 / 5xx / 空响应 / 无 key 路径。

### 8. 实现契约变更（设计指明，实现 agent 落地）

| 变更点 | 内容 |
|---|---|
| `ScreenerPanel::new` | 新增 `llm_enabled: bool` 参数（构造时传入，静态） |
| `ScreenerPanel::show` | 新增 `llm_signal: &Signal<RunLlmRequest>` 参数（与 `run_screener_signal` 并列） |
| `messages.rs` | 新增 `RunLlmRequest{prompt, seq}` / `RunLlmResponse{filter, error, seq}` |
| `SharedState` | 新增 `llm_input / llm_loading / llm_error / llm_result / llm_seq` 五字段 |
| `backend.rs` | 第五通道：`AsyncDispatcher<RunLlmRequest, RunLlmResponse>` + LLM 客户端 + 解析校验 + seq 守卫 |
| `main.rs` | 接线新通道；`llm_error` None→Some 迁移 → Error toast（screener_error 同款） |
| `config` | 新增 `[llm]` 节（`base_url` / `api_key` / `model`，key 可选）；doc-sync 到 `kb/user/config.md` |
| doc-sync | `kb/user/gui.md`（选股器章节）+ `kb/design/ui.md`（LLM 入口章节）+ `kb/design/architecture.md`（LLM 客户端基础设施，供 #153 复用） |

---

## 交互效果

egui 无 CSS/布局过渡——**动画克制**（LLM 入口是辅助功能，全部瞬时）：

| 触发 | 效果 | 时长/easing | 说明 |
|---|---|---|---|
| 提交生成 | 按钮进入 loading（内嵌 spinner + 禁用，文字「生成中…」）+ Input 禁用 | 组件内建 | SEPA 刷新按钮同款 |
| 成功 | 卡片瞬时出现在根组列表；输入文本保留 | 瞬时 | 卡片出现即反馈，无 success toast |
| 失败 | 错误行瞬时出现在输入区下方（`colored_label` error 色）+ Error toast 右上角 | 瞬时 / toast 8s 自动消失 | 内联 + toast 双通道 |
| Esc 取消 | spinner 停止、按钮恢复、在途响应被 seq 守卫丢弃 | 瞬时 | 无「取消动画」 |
| 输入框 hover/focus | Input 组件内建：focus 时 accent 1.5px 描边 | 组件内建 | 复用现有 |
| 按钮 hover/press | Button 组件内建 | 组件内建 | 复用现有 |
| 入口隐藏（无 key） | 整行不渲染，无任何占位 | — | 干净回退 |

---

## 待确认

1. **成功合并语义**：append + AND 展平（推荐——非破坏、常见案例平铺为根级卡、
   Or/Not 保持子组、round-trip 语义等价）vs 替换根组（丢弃用户手动条件，危险）
   vs 一律保留嵌套子组（视觉标记「AI 产物」但常见案例多一层嵌套）。
2. **成功/失败后输入文本**：保留（推荐——支持「微调措辞→再生成」迭代，聊天式
   保留便于修改）vs 成功清空（输入框干净，但丢失迭代上下文）。
3. **同 kind 去重**：不去重（推荐——交集语义等价、轻量、用户可手动删；Metabase
   同惯例）vs 生成时替换同 kind 已有卡（少冗余但需匹配/排序逻辑，成本上升）。
4. **Esc 取消实现**：seq 守卫取消（推荐——请求已发出，仅停止 spinner 而忽略
   结果会让「已取消的卡片」随机出现；seq 守卫约数行成本）vs 仅清除焦点（最轻
   但「取消」语义名不副实）。
5. **成功 toast**：无（推荐——卡片出现即反馈，安静）vs Info toast「已生成 N 个条件」。
6. **入口位置**：根组 Card 内顶部（推荐——语义关联 + 零新容器 + 隐藏即还原）
   vs 独立小 Card「AI 生成」置于构建器上方（视觉分区更强但多一层卡片、隐藏时
   构建器布局不变但垂直空间多一卡）。
7. **错误分类粒度**：后端二分（网络/解析，推荐——GUI 哑显示）vs 结构化
   error_kind 枚举（扩消息契约，成本高收益低）。

---

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| LLM 入口位置 | 根组 Card 内组头之下 / 独立「AI 生成」Card / 构建器与筛选按钮之间 | 根组 Card 内顶部 | 语义关联最强（描述→下方出卡）；零新增容器符合「轻量辅助」定位；隐藏时 Card 还原 #245 原貌 | 独立 Card 多一层 chrome、占用垂直空间；行间游离无容器语义弱 |
| 结果合并 | append + AND 展平 / 替换根组 / 一律嵌套子组 | append + AND 展平（纯函数 `llm_merge_into_root`） | 非破坏（用户手动条件绝不丢）；AND 结合律下展平语义等价，常见案例（A 且 B 且 C）平铺为根级卡与默认卡并排；Or/Not/嵌套天然保子组；单测可精确断言 | 替换根组丢弃手动条件（危险）；一律嵌套使常见案例多一层冗余子组 |
| 成功反馈 | 卡片出现即反馈（无 toast）/ Info toast「已生成 N 个」 | 无 toast | 卡片即时出现是最直接反馈；辅助功能保持安静 | toast 打扰高频迭代流 |
| 输入保留 | 成功/失败均保留 / 成功清空 | 保留 | 支持「微调措辞→再生成」迭代；失败保留可直接重试；聊天式心智 | 清空丢失上下文，失败后重试要重打 |
| 同 kind 去重 | 不去重 / 生成时替换同 kind 卡 | 不去重 | 交集语义等价（多一张卡无害）；零匹配/排序逻辑；Metabase 同惯例 | 替换需识别匹配卡与顺序处理，成本上升、超出辅助功能定位 |
| 状态归属 | SharedState 五信号（input/loading/error/result/seq）/ 面板局部字段 | SharedState | 切 tab/重建面板草稿与在途状态不丢（citizen 模式）；与 screener/sepa 三件套惯例同构；kittest 可直接断言 | 面板局部字段随 citizen 生命周期丢失输入草稿 |
| 入口显隐 | `ScreenerPanel::new` 构造参数 `llm_enabled` / SharedState 标志 / 每帧读 config | 构造参数 | key 配置是启动静态事实；每帧零开销；与 `restore`/`on_save` 构造注入惯例一致 | SharedState 放静态配置污染状态层；每帧读 config 浪费 |
| 请求通道 | 新第五通道 `AsyncDispatcher<RunLlmRequest, RunLlmResponse>` / 复用 run_screener 通道 | 新通道 | 与 sepa/index 通道模式完全同构；LLM 是独立后端职责（网络 I/O + 解析校验），不应与求值混流 | 复用 screener 通道破坏单一职责、错误语义混杂 |
| Esc 取消 | seq 守卫取消（响应 seq 匹配才写入）/ 仅停 spinner 忽略结果 | seq 守卫 | 请求已在途，忽略结果会让「已取消的卡片」在未知时刻混入根组；seq 比对约数行、纯逻辑可单测 | 仅停 spinner 是假取消，产生幽灵结果 |
| 错误处理 | 后端翻译为 i18n 文本 + GUI 哑显示（内联 + toast）/ GUI 匹配错误类型再本地化 | 后端翻译 + 双通道呈现 | `error.parquet_open` 先例；GUI 侧零本地化逻辑；内联（上下文可操作）+ toast（全局注意）双通道与 screener_error 一致 | GUI 匹配类型引入消息契约扩展，成本高 |
| 空结果/未知形状 | 后端语义校验拒绝空 And/Or；模板外形状走 `Unknown` 只读卡 | 双层兜底 | 空结果属「未能理解描述」应在解析期拦截；`Unknown` 卡是 #245 已锁定的任意 AST 兜底，LLM 产物直接复用（可删、可筛选、不崩溃） | 空结果放行会让 GUI 多一个「无变化」的困惑分支 |
| 动画范围 | 全部瞬时（组件内建 hover/press/loading spinner）/ 自定义动画 | 全部瞬时 | 与 #245 构建器一致；egui 无布局过渡；kittest 稳定 | 自定义动画违背克制原则、成本高 |

---

## 参考

- 基础构建器设计：#245 → `.omo/designs/llm-screener-ui.md`（本设计的基础）
- 权威设计系统：`kb/design/ui.md`（条件构建器章节 + 反馈状态 + 快捷键）
- 组件规范：`kb/design/ui-widgets.md`
- epic 上下文：`.omo/handoff.md`（#247 验收 + 锁定设计）
