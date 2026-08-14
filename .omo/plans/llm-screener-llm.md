# llm-screener-llm - Work Plan

## TL;DR (For humans)

**What you'll get:** 选股器内嵌 LLM——用户用自然语言描述选股意图（"找最近连续5天每天涨超3%、市值大于100亿的股票"），一键生成可执行的筛选条件：LLM 调用（OpenAI 兼容端点）返回结构化 Filter AST JSON，经类型校验后直接以条件卡片形式落入 #245 可视化构建器（可编辑、可持久化、可运行）。无 API key 时隐藏入口；校验/网络失败时优雅提示，不崩溃。

**Why this approach:** LLM 输出目标就是 #244 的 Filter AST serde 格式（与 GUI/引擎/持久化共享同一表示）；通用 LLM 客户端（HTTP 调用 + JSON 解析）放 compass-core 供未来 #153（行业新闻分析）复用；语义校验独立成纯函数，GUI/后端/测试共用。

**What it will NOT do:** 不做自由文本解析（LLM 输出必须是结构化 JSON）；不引入 OpenAI/anthropic SDK（reqwest 直调）；不做 LLM 缓存/重试/多轮对话；不改 #245 构建器卡片模型；不隐藏 API key（明文存 config.toml）。

**Effort:** Medium（8 个实现任务，涉及 4 个 crate + i18n + docs）
**Risk:** Medium - 主要风险在 LLM 输出的不可预测性，由 serde 解析 + 语义校验双层防护兜底；网络调用走既有 AsyncDispatcher 模式（与 screener/sepa 同构）
**Decisions to sanity-check:** D1（LLM 客户端放 compass-core）、D2（语义校验独立纯函数放 compass-types）、D3（prompt/解析业务层放 compass crate）、D4（第 5 个 AsyncDispatcher 通道）、D5（明文存 config）

Your next move: approve the plan, then execute in this worktree session (feat/llm-screener-llm).

---

> TL;DR (machine): Medium effort, Medium risk, 8 implementation todos + final verification wave; LLM client in compass-core (reusable by #153), validate_filter pure function in compass-types, prompt/parse business logic in compass, 5th AsyncDispatcher channel in compass backend, [llm] config section, UI entry in ScreenerPanel.

## Scope

### Must have
- `compass-core`：新模块 `llm.rs` —— 通用 OpenAI 兼容 chat completions 客户端 `LlmClient`（`LlmConfig{base_url, api_key, model}` + `chat_json(system, user) -> Result<Value, LlmError>`）+ `LlmError`（EmptyConfig/Network/Http{status,body}/NoContent/InvalidJson），thiserror 错误类型，供 #153 复用
- `compass-types`：`screener.rs` 新增纯函数 `validate_filter(&Filter) -> Result<(), String>` —— 窗口参数 > 0、MarketCap min<=max、数值有限性（非 NaN/Inf）、嵌套深度上限（防栈溢出）
- `compass`：新模块 `llm_screener.rs` —— `build_screener_prompt(desc)`（system 注入 AST schema + 枚举值 + 示例 + 严格 JSON 约束）+ `parse_filter_response(content)`（strip ```json 围栏 → serde 反序列化 → validate_filter）
- `compass`：`messages.rs` 新增 `RunLlmRequest{prompt}` / `RunLlmResponse{filter: Option<Filter>, error: Option<String>}`；`backend.rs` 第 5 个 AsyncDispatcher 通道（与 screener/sepa/index 同构）；`state.rs` 新增 `llm_loading`/`llm_error` 信号
- `compass`：`main.rs` `FullConfig` 新增 `[llm]` 节（`LlmSection{base_url, api_key, model}`，api_key 可选，`Default` 提供 base_url/model）；`wire_backend` 接线；未配置 api_key → GUI 隐藏 LLM 入口
- `compass`：`citizens/screener.rs` ScreenerPanel 新增 LLM 输入区（按 `.omo/designs/llm-screener-llm.md` ui-designer 方案）：自然语言输入框 + 生成按钮 + 加载态 + 错误提示；生成成功 → `filter_to_items` → `builder_root` 替换（可编辑/持久化/运行）
- i18n：`compass-i18n/locales/{zh,en}.yml` 新增 llm 相关文案（输入占位、生成按钮、加载、错误分类提示）
- 测试：compass-core（httpmock mock /v1/chat/completions 成功/非法 JSON/5xx/空响应）、compass-types（validate_filter 规则矩阵）、compass（parse_filter_response 围栏/非法/成功 + egui_kittest UI + backend llm 通道 roundtrip）
- doc-sync：`kb/user/config.md`（[llm] 节）、`kb/user/gui.md`（选股器 LLM 入口）、`kb/design/architecture.md`（LLM 基础设施章节）、决策记录 `## 决策记录` 章节

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不引入 OpenAI/anthropic/任何 LLM SDK crate（reqwest 直调）
- 不做自由文本 → AST 的本地解析（一切依赖 LLM 结构化输出 + serde 校验）
- 不做 LLM 多轮对话 / 结果缓存 / 重试退避 / 流式输出
- 不改 #245 构建器的 `CondItem`/`CondLeaf`/`filter_to_items` 模型（LLM 结果直接复用现有渲染）
- 不做 #153 行业新闻分析（仅预留可复用的 LlmClient 基础设施）
- API key 不做加密/混淆（明文存 `~/.config/compass/config.toml`，与项目其他配置同级）
- 不隐藏 API key 于 UI（输入框仅接受自然语言描述）
- 不新增第三方运行时依赖（仅 compass 加 httpmock dev-dep）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试 RED → 实现 GREEN）+ rstest/tokio::test 既有模式（见 kb/dev/testing.md）；HTTP 用 httpmock（workspace 已有，compass-core 已有 dev-dep，compass 需补 dev-dep）
- Evidence: `.omo/evidence/<task>-llm-screener-llm.txt`
- 覆盖率：`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json`（compass-core/compass-types 95%，compass 90%）
- 构建：`cargo build` / `cargo clippy -- -D warnings` / `cargo fmt --check`（mold 链接器已配置）

## Execution strategy

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. compass-types validate_filter | — | 3,8 | 2 |
| 2. compass-core llm 客户端 | — | 5,6 | 1 |
| 3. compass llm_screener prompt/parse | 1,2 | 6,8 | 4 |
| 4. config [llm] 节 + FullConfig | — | 6,7 | 1,2 |
| 5. backend 第 5 通道 | 2 | 6,8 | 4 |
| 6. ScreenerPanel LLM 输入区 UI | 3,4,5,设计 | 7,8 | — |
| 7. i18n 文案 | 6 | 8 | — |
| 8. doc-sync + 决策记录 | 1,3,4,5,6,7 | F-wave | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE - never rewrite the headers above. -->
- [ ] 1. compass-types: 实现 `validate_filter(&Filter) -> Result<(), String>` 纯函数 + 规则矩阵测试
  What to do / Must NOT do: 在 crates/compass-types/src/screener.rs 新增 `pub fn validate_filter(filter: &Filter) -> Result<(), String>`（screener_builder 同 crate 可复用）。规则（全部返回 Err(String) 带可读消息，含字段名与值）：① 所有窗口/计数参数 > 0 —— `Sma(n)`/`ChangePct(n)`/`AvgVolume(n)`/`NDayHigh(n)` 的 n、`Count.window`/`Count.at_least`/`VolumeSurge.days`/`UpDays.n`（n==0 或 1 均合法但 0 非法）；② `Count.at_least <= Count.window`；③ `MarketCap{min,max}` 当两者均 Some 时 min <= max；④ 所有 f64 数值字段有限（`is_finite`）：`Const(v)`、`UpDays.min_pct`、`VolumeSurge.times`、`MarketCap.min/max`、`Count.value` 的 Const；⑤ 递归深度上限 32（And/Or 子节点、Not 内部深度累加，超限返回 Err("nesting too deep")）。递归用内部带深度参数的 helper。空 `And(vec![])`/`Or(vec![])` 合法（构建器空状态）。MUST NOT: 不改 Filter/AST 类型定义；不把校验逻辑写进 GUI 或引擎；不做"语义合理性"判断（如 min_pct 是否在 0-100 之间——只做结构性非法检查）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3,8
  References: crates/compass-types/src/screener.rs:18-29（Filter 定义）、91-110（MetaCond MarketCap）、121-135（SeriesFactor）、146-210（CmpOp/SeriesCond 各变体）; crates/compass-types/src/lib.rs（测试风格）; .omo/handoff.md:50-53（关键设计点 2：语义校验窗口>0、pct 范围、嵌套深度上限）
  Acceptance criteria (agent-executable): `cargo test -p compass-types` 新增测试全绿：合法 Filter（正常窗口/合法边界）→ Ok；每个非法规则至少一条测试 → Err 且消息含相关字段名；嵌套深度 32 合法、33 非法（构造深层 Not 链或嵌套 And）；`validate_filter(&Filter::And(vec![]))` → Ok
  QA scenarios: happy: `cargo test -p compass-types` 全绿; failure: 手写非法 Filter（n=0、min>max、NaN、深度 33）逐一断言 Err; Evidence .omo/evidence/task-1-llm-screener-llm.txt
  Commit: Y | `feat(types): validate_filter semantic validation for screener AST` + 独立成行 `ref #247`

- [ ] 2. compass-core: 实现 `llm` 模块（LlmConfig/LlmError/LlmClient::chat_json）+ httpmock 测试
  What to do / Must NOT do: 新增 crates/compass-core/src/llm.rs（lib.rs `pub mod llm;`）。类型：`#[derive(Debug, Clone, Deserialize)] pub struct LlmConfig { pub base_url: String, pub api_key: String, pub model: String }`（serde 反序列化自 config.toml [llm] 节）；`#[derive(Debug, thiserror::Error)] pub enum LlmError { #[error("llm not configured: {0}")] EmptyConfig(String), #[error("network error: {0}")] Network(#[from] reqwest::Error), #[error("http {status}: {body}")] Http { status: u16, body: String }, #[error("empty response content")] NoContent, #[error("invalid JSON in response: {0}")] InvalidJson(String) }`。`pub struct LlmClient { config: LlmConfig, http: reqwest::Client }`；`pub fn new(config: LlmConfig) -> Self`（不校验 api_key 非空——EmptyConfig 在调用路径判断或 new 时校验 api_key 为空则 Err？决策：`new` 接受配置，`chat_json` 前调用方自行判断 key 存在，客户端只做 HTTP；但为防误用，`new` 校验 `base_url`/`model` 非空，返回 `Result<Self, LlmError>`）；`pub async fn chat_json(&self, system: &str, user: &str) -> Result<serde_json::Value, LlmError>`：POST `{base_url}/chat/completions`（base_url 末尾已含 /v1 时直接拼接 /chat/completions，否则补 /v1——决策：约定 base_url 是完整 API 根如 `https://api.openai.com/v1`，直接拼 `/chat/completions`），body JSON `{"model", "messages":[{"role":"system","content":system},{"role":"user","content":user}], "temperature":0.0, "response_format":{"type":"json_object"}}`，Authorization Bearer api_key，Content-Type application/json；2xx → 解析 `choices[0].message.content`（缺失/空 → NoContent）→ `serde_json::from_str`（失败 → InvalidJson(raw)）；非 2xx → Http{status, body}。reqwest 超时：`Client::builder().timeout(Duration::from_secs(60))`。MUST NOT: 不做重试/退避/流式；不写 Filter/prompt 相关逻辑（业务层在 compass）；不隐藏 key（无日志打印 key）；base_url 规范化仅约定 `/v1` 前缀由配置方保证。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5,6
  References: crates/compass-core/Cargo.toml:12-13（reqwest/serde_json 已有）、24-29（httpmock/tempfile/rstest dev-deps 已有）; crates/compass-core/src/data/duckdb.rs（crate 内模块风格）; Cargo.toml:28（workspace reqwest 0.12 json+rustls-tls）; .omo/handoff.md:46-48（关键设计点 1：reqwest 调 OpenAI 兼容端点）
  Acceptance criteria (agent-executable): `cargo test -p compass-core` 新增测试全绿（httpmock）：mock `POST /v1/chat/completions` 返回合法 JSON content（`{"foo":1}`）→ chat_json Ok; content 非法 JSON → Err(InvalidJson); 5xx → Err(Http{status:500,..}); content 缺失（choices 空）→ Err(NoContent); 网络错误（mock 不匹配路径 404 或无效 URL）→ Err(Network/Http)。测试中 base_url 指向 httpmock server（如 `http://127.0.0.1:{port}/v1`）
  QA scenarios: happy: `cargo test -p compass-core` llm 模块全绿; failure: 每种错误路径断言对应 LlmError 变体; Evidence .omo/evidence/task-2-llm-screener-llm.txt
  Commit: Y | `feat(core): OpenAI-compatible LLM chat client (LlmClient)` + 独立成行 `ref #247`

- [ ] 3. compass: 实现 `llm_screener.rs`（prompt 构建 + 响应解析 + 校验集成）+ 测试
  What to do / Must NOT do: 新增 crates/compass/src/llm_screener.rs。`pub fn build_screener_prompt(description: &str) -> String`：system prompt 内容——你是 A 股选股条件生成助手；严格输出单个 JSON 对象（Filter AST），禁止 markdown 代码围栏/注释/多余文本；给出 Filter 的 serde tagged-union JSON 格式说明（`{"Meta":{...}}`/`{"Series":{...}}`/`{"And":[...]}`/`{"Or":[...]}`/`{"Not":{...}}` 递归结构）；列出可用 `MetaCond` 变体（Industry/Exchange/Board/ListYears/Delisted/MarketCap）、`SeriesFactor`（Close/Sma/ChangePct/DayPct/AvgVolume/NDayHigh）、`CmpOp`（eq/ne/gt/ge/lt/le）、`SeriesCond`（Cmp/UpDays/Count/VolumeSurge）与字段语义（市值单位亿元、涨跌幅单位 %、A 股红涨绿跌）；给 1 个完整示例（"最近5天每天涨超3%" → `{"Series":{"UpDays":{"n":5,"min_pct":3.0}}}`）；强调数值必须合理（窗口 ≥1、min_pct 非 NaN）。`pub fn parse_filter_response(content: &str) -> Result<Filter, String>`：trim → strip 首尾 ```json/``` 围栏（正则或手写 trim）→ `serde_json::from_str::<Filter>`（Err → `format!("invalid filter JSON: {e}")`）→ `validate_filter(&f)`（Err → 原样返回）→ Ok(f)。MUST NOT: 不把 prompt 模板硬编码进 GUI 层（保持纯函数可测）；不在本模块做 HTTP 调用（客户端在 compass-core）；不做自由文本启发式解析。
  Parallelization: Wave 2 | Blocked by: 1,2 | Blocks: 6,8
  References: crates/compass-types/src/screener.rs:18-29（Filter serde 格式——prompt 依据）; crates/compass/src/main.rs:277-286（ScreenerSection 风格——本模块放 compass 的既有业务模块风格参考）; .omo/handoff.md:30-35（锁定设计：serde tagged union 输出、类型校验失败回退）
  Acceptance criteria (agent-executable): `cargo test -p compass` 新增测试全绿：parse_filter_response 对带 ```json 围栏的合法内容 → Ok(Filter)；裸合法 JSON → Ok；未知 tag → Err 含 "invalid filter JSON"；语义非法（n=0）→ Err 含校验消息；空内容 → Err；build_screener_prompt 断言包含 "Filter"、"UpDays"、示例 JSON 片段（防止 prompt 退化）
  QA scenarios: happy: `cargo test -p compass` llm_screener 全绿; failure: 非法输入逐一断言 Err 且消息分类正确; Evidence .omo/evidence/task-3-llm-screener-llm.txt
  Commit: Y | `feat(compass): screener LLM prompt builder + Filter response parser` + 独立成行 `ref #247`

- [ ] 4. compass: config 新增 `[llm]` 节（LlmSection + FullConfig 接入）+ 测试
  What to do / Must NOT do: main.rs 新增 `#[derive(Deserialize, Default)] struct LlmSection { #[serde(default)] base_url: String, #[serde(default)] api_key: String, #[serde(default)] model: String }`（Default：base_url = "https://api.openai.com/v1"，model = "gpt-4o-mini"——实现 Default 手动而非 derive 空串）；`FullConfig` 增加 `#[serde(default)] llm: LlmSection` 字段；提供 `impl LlmSection { pub fn is_configured(&self) -> bool { !self.api_key.is_empty() } }` 与 `pub fn to_client_config(&self) -> Option<LlmConfig>`（is_configured 时 Some，否则 None——空 base_url/model 回退 Default）。main.rs 装配处：读取 `config.llm` 传入 ScreenerPanel 构造（隐藏/显示入口）与 `wire_backend`（LLM 通道）。MUST NOT: 不把 LlmSection 放 compass-core（config 解析属 compass 装配层；LlmConfig 类型在 compass-core 用于客户端）；不改既有 config 键；不写 API key 到日志。
  Parallelization: Wave 2 | Blocked by: — | Blocks: 6,7
  References: crates/compass/src/main.rs:256-264（FullConfig 结构）; crates/compass/src/main.rs:266-298（ScreenerSection 双格式模式——LlmSection 参考）; crates/compass/src/main.rs:318-345（load_config 模式）; crates/compass-core/src/llm.rs（Todo 2 产出的 LlmConfig）
  Acceptance criteria (agent-executable): `cargo test -p compass` 或编译期验证：`cargo check -p compass` 通过；配置含 `[llm] api_key="sk-x"` 时 `is_configured()==true`；缺省时 `to_client_config()==None`；缺省 base_url/model 正确回退 Default 值（单测断言 LlmSection::default()）
  QA scenarios: happy: `cargo check -p compass` + LlmSection 单测全绿; failure: 无 api_key 时 is_configured false 且 to_client_config None（GUI 隐藏入口的依据）; Evidence .omo/evidence/task-4-llm-screener-llm.txt
  Commit: Y | `feat(compass): [llm] config section (base_url/api_key/model)` + 独立成行 `ref #247`

- [ ] 5. compass: backend 第 5 通道（RunLlmRequest/RunLlmResponse + wire_backend + SharedState 信号）
  What to do / Must NOT do: messages.rs 新增 `#[derive(Clone)] pub struct RunLlmRequest { pub prompt: String }` 与 `#[derive(Clone)] pub struct RunLlmResponse { pub filter: Option<Filter>, pub error: Option<String> }`。state.rs SharedState 新增 `pub llm_loading: Dynamic<bool>`、`pub llm_error: Dynamic<Option<String>>`（构造初始化）。backend.rs：`wire_backend` 增加第 5 组 signal/slot + AsyncDispatcher：入参新增 `llm_config: Option<LlmConfig>`（None → handler 立即返回 error「LLM 未配置」）；handler：`LlmClient::new(cfg)` → `build_screener_prompt(&req.prompt)` 系统/用户 prompt → `client.chat_json(system, &req.prompt)` → `parse_filter_response(content)` → RunLlmResponse{filter: Some, error: None}；任何 LlmError → error: Some(格式化消息)；`result_slot.start` 写 `state.llm_loading=false`、`state.llm_error`、请求 `repaint_ctx.request_repaint()`（成功时 filter 由 UI 侧从响应中取用——响应经信号返回 UI，UI 直接处理，无需写 SharedState 的 filter 信号）。`wire_backend` 返回元组增加 `Signal<RunLlmRequest>`；`BackendHandle` 增加 `_llm_dispatcher` 字段。**更新所有 wire_backend 调用点**（main.rs + backend.rs 内全部测试的解构元组）。i18n 错误消息模板（error.llm_not_configured）可暂用 t!() 或 format，Todo 7 补全。MUST NOT: 不在 handler 内做 UI 渲染；不阻塞 UI 线程（全走 AsyncDispatcher）；不打印 api_key。
  Parallelization: Wave 2 | Blocked by: 2 | Blocks: 6,8
  References: crates/compass/src/backend.rs:39-44（BackendHandle 结构）、55-65（wire_backend 签名）、139-192（screener 通道完整模式）、328-340（返回元组）; crates/compass/src/messages.rs:30-46（RunScreenerRequest/Response 模式）; crates/compass/src/state.rs:25-31（screener 信号模式）、62-65（构造初始化）; crates/compass/src/llm_screener.rs（Todo 3）; crates/compass-core/src/llm.rs（Todo 2）
  Acceptance criteria (agent-executable): `cargo check -p compass` 通过（所有 wire_backend 调用点更新）；backend 测试新增：llm 通道 roundtrip（httpmock 起 mock server，LlmConfig 指向它，发送 RunLlmRequest → 断言 state.llm_loading 复位 false、llm_error None、响应 filter Some）；llm_config=None → 响应 error 含「未配置」且 filter None；mock 5xx → llm_error Some。compass Cargo.toml 增加 `httpmock = { workspace = true }` dev-dep
  QA scenarios: happy: `cargo test -p compass` backend llm 通道全绿; failure: 无配置/5xx 路径断言 error 而非 panic; Evidence .omo/evidence/task-5-llm-screener-llm.txt
  Commit: Y | `feat(compass): LLM screener backend channel (RunLlmRequest/Response)` + 独立成行 `ref #247`

- [ ] 6. compass: ScreenerPanel LLM 输入区（按 .omo/designs/llm-screener-llm.md）+ egui_kittest 测试
  What to do / Must NOT do: 按 ui-designer 产出并经用户确认的 `.omo/designs/llm-screener-llm.md` 实现（**实现前必须先读该文件**，以下为设计要点与底线）：ScreenerPanel 新增字段 `llm_input: String`、`llm_llm_enabled: bool`（构造时由外部传入是否配置 key）；`show()` 签名新增 `run_llm_signal: &Signal<RunLlmRequest>` 参数（调用点 main.rs 更新）；LLM 区渲染：仅当 `llm_enabled` 时显示——输入框（egui TextEdit 单行/多行，placeholder 自然语言示例）+ 生成按钮（icon SPARKLE + i18n 文案，点击 → send RunLlmRequest{prompt}，清空 llm_error）；`shared_state.llm_loading` 为 true 时禁用输入与按钮并显示 spinner/生成中文案；`shared_state.llm_error` 非 None 时显示错误（colored_label，error_fg_color）；**生成成功回调**：UI 从 llm 结果信号接收 `RunLlmResponse`（通过一个 `Signal<RunLlmResponse>` 或复用 slot 模式——决策：wire_backend 的 llm result_slot 写 state，同时提供一个 `Signal<RunLlmResponse>` 供 ScreenerPanel 订阅；实现细节：`wire_backend` 返回 `Signal<RunLlmRequest>` 供 UI 发送，UI 侧建 result 订阅 `factory::create_signal_slot::<RunLlmResponse>()` 传入 ScreenerPanel，backend 的 llm result_slot 转发或 UI 直接由 SharedState 轮询——**以 ui-designer 设计文件为准，保持与 screener 通道一致的信号模式**）；成功 → `filter_to_items(&filter)` 替换 `builder_root`（`builder_root_operator` 重置 And、`builder_multi_selects` 清空）；失败 → llm_error 显示，builder 不变。i18n 文案键（Todo 7 落 yml，此处用 t! 引用）。MUST NOT: 不改 #245 构建器卡片渲染逻辑（LLM 只是把结果灌入既有模型）；不显示 API key；不用 UI 线程发 HTTP。
  Parallelization: Wave 3 | Blocked by: 3,4,5,设计 | Blocks: 7,8
  References: .omo/designs/llm-screener-llm.md（ui-designer 设计，**必读**）; crates/compass/src/citizens/screener.rs:89-106（ScreenerPanel 字段）、127-159（new）、170-211（show 结构）、260-304（condition_builder 渲染模式）; crates/compass/src/citizens/screener.rs:1122-1165（egui_kittest 测试模式）; crates/compass/src/main.rs（ScreenerPanel 构造调用点）
  Acceptance criteria (agent-executable): `cargo test -p compass` 新增 kittest 测试全绿：llm_enabled=true 渲染输入框与生成按钮（get_by_label）；llm_enabled=false 不渲染（query 计数 0）；点击生成按钮发送 RunLlmRequest（订阅断言收到 prompt）；llm_loading=true 时按钮禁用；llm_error 非 None 时渲染错误文本。编译通过 + 既有 screener 测试不回归
  QA scenarios: happy: kittest 全绿 + `cargo test -p compass` 无回归; failure: 未配置 key 时 UI 无 LLM 入口痕迹; Evidence .omo/evidence/task-6-llm-screener-llm.txt
  Commit: Y | `feat(compass): natural-language LLM input in screener panel` + 独立成行 `ref #247`

- [ ] 7. i18n: zh/en 文案新增（llm 输入区 + 错误分类）
  What to do / Must NOT do: crates/compass-i18n/locales/zh.yml 与 en.yml 的 screener: 节新增（键与 Todo 5/6 的 t! 引用一致）：`screener.llm.input_placeholder`（zh: "用自然语言描述选股条件，如：最近5天每天涨超3%、市值大于100亿"）、`screener.llm.generate`（生成）、`screener.llm.generating`（生成中…）、`screener.llm.not_configured`（未配置 LLM API key）、`error.llm_request`（LLM 请求失败：{e}）、`error.llm_invalid_filter`（无法解析生成的筛选条件，请调整描述后重试）。en 对应翻译。MUST NOT: 不新增非 screener/error 命名空间之外的键；不改既有文案。
  Parallelization: Wave 3 | Blocked by: 6 | Blocks: 8
  References: crates/compass-i18n/locales/zh.yml:91（screener: 节位置）; crates/compass-i18n/locales/en.yml（对应位置）; crates/compass-i18n/src/lib.rs（t! 宏用法）
  Acceptance criteria (agent-executable): `cargo test -p compass-i18n` 或编译验证 t! 键存在；grep zh.yml/en.yml 确认键名一一对应
  QA scenarios: happy: 两语言文件键集一致; failure: 缺键则 t! 回退原始键名（测试断言不出现原始键名输出）; Evidence .omo/evidence/task-7-llm-screener-llm.txt
  Commit: Y | `feat(i18n): LLM screener UI strings (zh/en)` + 独立成行 `ref #247`

- [ ] 8. doc-sync + 决策记录
  What to do / Must NOT do: 按变更类型 → kb/ 映射表（AGENTS.md）更新：`kb/user/config.md` 新增 `[llm]` 配置节说明（base_url/api_key/model + 默认值 + 未配置行为）；`kb/user/gui.md` 选股器章节新增 LLM 自然语言入口说明（入口可见条件、用法、失败行为）；`kb/design/architecture.md` 新增 LLM 基础设施章节（LlmClient 归属 compass-core、prompt/解析在 compass、第 5 通道、#153 复用点）；`kb/design/` 相关文件补 `## 决策记录` 章节（what + why + why-not 表格，含 D1 客户端放 compass-core、D2 校验纯函数放 compass-types、D3 业务层放 compass、D4 第 5 通道、D5 明文 key 五项决策）；`.omo/designs/llm-screener-llm.md` 设计归档随本 commit 提交（.gitignore 已放行 .omo/designs）。MUST NOT: 不写其他 batch 的文档；AGENTS.md 仅当项目级约定变化才更新（本批次不涉及）。
  Parallelization: Wave 4 | Blocked by: 1,3,4,5,6,7 | Blocks: F-wave
  References: kb/user/config.md（[screener]/[watchlist] 节格式参考）; kb/user/gui.md（选股器章节）; kb/design/architecture.md（crate 布局、线程模型章节）; AGENTS.md（kb/ 映射表）; .omo/handoff.md（决策来源）
  Acceptance criteria (agent-executable): grep 确认 kb/design/ 相关文件含 `## 决策记录` 表格且含 D1-D5 五项；config.md 含 `[llm]` 节说明；gui.md 含 LLM 入口说明；architecture.md 含 LlmClient 归属说明
  QA scenarios: happy: grep 全部命中; failure: 缺任一决策行或文档节则 grep 失败; Evidence .omo/evidence/task-8-llm-screener-llm.txt
  Commit: Y | `docs: LLM screener config/UI docs + decision records` + 独立成行 `ref #247`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对 8 个 todo 的 Acceptance criteria 证据落盘 `.omo/evidence/task-{1..8}-llm-screener-llm.txt`；`git log` 确认每个 commit 含独立成行 `ref #247`；`cargo test --workspace` 全绿
- [ ] F2. Code quality review: `cargo clippy --workspace -- -D warnings` 无新警告；`cargo fmt --check` 通过；新增 pub 项全部带 `///` 文档注释（missing_docs 规范）；无 unwrap/`as` 滥用；API key 不出现在日志/错误消息
- [ ] F3. Coverage: `cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json` 退出码 0（compass-core/compass-types ≥95%、compass ≥90% 门槛不降）
- [ ] F4. Scope fidelity: 核对 Must NOT have 清单——无 LLM SDK 依赖（`grep "openai\|anthropic" Cargo.toml*` 无命中）；无自由文本解析逻辑；无缓存/重试/流式；filter_to_items/LeafKind/CondItem 模型未改；#153 未实现；api_key 未出现在 UI/日志

## Commit strategy
- 每个 commit 独立成行 `ref #247`（hook 校验，指向 OPEN issue）；epic 批量子 issue 引用
- 顺序：1→2→3→4→5→6→7→8（Todo 5 与 4 可并行；Todo 6 需等设计文件 + 3/4/5）
- Commit → Review：每个子 issue commit 后运行 `/review-work`（goal/quality/security/QA/context 5 并行），发现问题最多 2 轮修复
- 禁止 auto-push：用户确认后 push；push 前 `git fetch origin master && git rebase origin/master`
- push 前写反思（/skwy-reflect），反思 commit 随 PR 同批推送
- push 后追加完成 comment（`gh issue comment 247`）+ 关闭 issue #247；epic #243 总结 comment + 关闭（本批为最终批次）

## Success criteria
- [ ] LLM 客户端（compass-core::llm）httpmock 测试全绿（成功/非法 JSON/5xx/空响应）（Todo 2 验收）
- [ ] validate_filter 规则矩阵测试全绿（窗口>0/min<=max/有限性/深度上限）（Todo 1 验收）
- [ ] prompt 构建 + 响应解析（围栏剥离/非法拒绝/校验通过）测试全绿（Todo 3 验收）
- [ ] [llm] config 节 + is_configured/to_client_config 逻辑（Todo 4 验收）
- [ ] backend 第 5 通道 roundtrip（成功/未配置/5xx）测试全绿（Todo 5 验收）
- [ ] ScreenerPanel LLM 输入区 kittest 测试全绿（入口显隐/发送/加载/错误）（Todo 6 验收）
- [ ] zh/en i18n 文案键集一致（Todo 7 验收）
- [ ] kb/ 文档同步 + `## 决策记录` 章节（D1-D5）（Todo 8 验收）
- [ ] 全部 commit 引用 `ref #247`，push 后 issue 收尾（comment + close，epic 总结）
