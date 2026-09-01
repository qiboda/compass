# Plan: K 线图复权方式切换（前复权/后复权/不复权）+ 修复 adjclose 口径错误

**Issue**: #345（OPEN，C-Feature/A-GUI/A-Data）
**分支**: feat/adjust-mode（基于 origin/master 38bd8dc，同步无落后）
**日期**: 2026-09-01
**设计依据**: `.dsh/designs/adjust-mode.md`（用户已确认 4 项决策：联动图=主图 / en QFQ-HFQ-None / 96px / 立即重载）

---

## 1. 目标

K 线主图（ChartCitizen，主 Chart tab；SEPA/screener 行点击联动同一图）支持三档复权
切换（qfq/hfq/none），修复 ref #176 前复权化导致的 adjclose 口径错误（Dolt
adjclose 实为**后复权**，原 factor=adjclose/close 直接放大为后复权显示）。

**范围**（grill-me 锁定，不可更改）：
- 仅 K 线图表显示（fetch_bars）跟随三档；SEPA 面板「最新价」保持 close（backend.rs:486 `latest: last.close`）；选股器/回测不变。
- 指数/板块（is_index_or_board）隐藏切换控件；指数永远 factor=1.0。
- config 新增 `default_adjust`（默认 qfq）；运行中切换不持久化。

**已确认设计要点**：
- Group B 静态「前复权」Tag → `Dropdown` 三档（复用 compass-ui/dropdown.rs，零改组件），`id_salt("adjust")`、`width(96)`、32px 同高、原位（Segmented 后，sm 8px）。
- i18n：`toolbar.adjust.qfq/hfq/none`（zh 前复权/后复权/不复权；en QFQ/HFQ/None）；删除旧键 `toolbar.adjust`；常量+ALL_KEYS+main.rs:3934 对照表同步。
- 前复权锚点：全序列最新交易日，`factor_i' = (adjclose_i/close_i) ÷ 最新日比率`，最新 bar=现价；adjclose NULL/非有限/close<=0 → factor 1.0。
- 1w/1M 聚合（duckdb.rs SQL）同样三档：先缩放后聚合；qfq 时 scale 需除最新日比率。
- `set_adjust` 仿 `set_timeframe`（同步 SharedState.adjust → 无条件 fetch_bars，last request wins）；SEPA/screener 行点击经 `dispatch_symbol_fetch` 携带当前档位。

---

## 1.5 接口契约（锁定，对抗性测试 RED 依据）

- `provider.rs`：`DataProvider::fetch_bars(&self, symbol: &str, timeframe: &str, range_start: i64, range_end: i64, adjust: &str) -> Result<..., ...>`——adjust 值域 `"qfq" / "hfq" / "none"`，**未知值回退 `"qfq"`**（与 UI 默认一致）；返回类型沿现有签名。`timeframe: &str` 参数沿现有签名实际类型（实现时以现状为准，仅新增 adjust）。
- `messages.rs`：`FetchRequest { symbol, timeframe, range_start, range_end, adjust: String }`。
- `state.rs`：`SharedState.adjust: Dynamic<String>` 默认 `"qfq"`；`SharedState::new` 增加 default_adjust 参数。
- `model.rs`：`AppSection { ..., default_adjust: String }`，`#[serde(default = "default_adjust")]`，`default_adjust() -> "qfq"`。
- 数据层三态语义（日线 adjust_ohlc 路径）：
  - 每行 `ratio_i = adjclose_i / close_i`（close<=0 或 adjclose 非有限/NULL → ratio 无效）
  - `none`: `factor_i = 1.0`（全部行）
  - `hfq`: `factor_i = ratio_i`（ratio 无效行 factor=1.0）——后复权（adjclose 口径本身）
  - `qfq`: `r_anchor` = 序列中**最后一个 ratio 有效行**的 ratio；无有效行 → 全部 factor=1.0；否则 `factor_i = ratio_i / r_anchor`（ratio 无效行 factor=1.0）——前复权归一，最新有效行 factor=1.0（最新 bar=现价）
- 1w/1M 聚合路径（duckdb.rs SQL，735-807）：先缩放后聚合；scale 三档语义与日线一致（ratio 无效行 factor=1.0，**不除锚**）；`r_anchor` 在日线层（查询窗口内全部日线行）计算并传入 SQL（逐行 scale ÷ r_anchor），等效于「聚合序列最后一根有效 bar 的 close 不变」。
- `main.rs` 辅助：`adjust_value(idx: usize) -> &'static str`（0→"qfq",1→"hfq",2→"none"，越界→"qfq"）；`adjust_index_from_value(s: &str) -> usize`（"qfq"→0,"hfq"→1,"none"→2，未知→0）；`set_adjust(&mut self, idx: usize)`（同步 `self.shared_state.adjust` 后无条件 `fetch_bars()`）。

## 2. 实现步骤（阶段 + commit 边界；每个 commit 含 `ref #345` 独立成行）

### 阶段 A：数据层三态（compass-core）
1. `crates/compass-core/src/model.rs`：`AppSection` 加 `#[serde(default = "default_adjust")] pub default_adjust: String` + `default_adjust() -> "qfq"`（355-378 区域）。
2. `crates/compass-core/src/data/provider.rs`：`DataProvider::fetch_bars(&self, symbol, timeframe, range_start, range_end, adjust)` 增加 `&str`/`Adjust` 参数（58-66 行）；波及 duckdb.rs / parquet.rs / synthetic.rs。
3. `crates/compass-core/src/data/duckdb.rs`（~520 起）三态：
   - qfq：`factor_i = (adjclose_i/close_i) ÷ 最新日比率`（归一化前复权，最新 bar=现价）
   - hfq：`factor_i = adjclose_i/close_i`（现行为 = 后复权，修正口径）
   - none：factor 1.0
   - 1w/1M 聚合 SQL（735-807）：`scale` 三态；qfq 时 `scale ÷ 最新日比率`
   - adjclose NULL / 非有限 / close<=0 → factor 1.0（保留现有兜底）
4. `crates/compass-core/src/data/parquet.rs:140-211`、`synthetic.rs:28`：适配新签名（ParquetReader 仅 benches/测试用，生产无调用方）。

### 阶段 B：GUI 数据流（compass）
5. `crates/compass/src/messages.rs:16-21`：`FetchRequest` 加 `adjust: String`。
6. `crates/compass/src/backend.rs:~104`：读 `req.adjust` 传 provider.fetch_bars。
7. `crates/compass/src/state.rs:14-15/66-102`：`SharedState` 加 `adjust: Dynamic<String>`（默认 "qfq"）；`SharedState::new` 签名带 default_adjust。
8. `crates/compass/src/dispatcher.rs:98-107`：`dispatch_symbol_fetch` 构造 FetchRequest 时带 `shared_state.adjust`（SEPA/screener/market 行点击联动同档位）。

### 阶段 C：工具栏 UI + i18n（compass + compass-i18n）
9. `crates/compass-i18n/src/lib.rs:45-46`：`KEY_TOOLBAR_ADJUST_QFQ/HFQ/NONE` + ALL_KEYS（~359）。
10. `crates/compass-i18n/locales/zh.yml` / `en.yml`：`toolbar.adjust` → 三段子键（旧键删除）。
11. `crates/compass/src/main.rs`：
    - 字段 `adjust_index: usize`（838 区域）；初始化 `adjust_index_from_value(&config.app.app.default_adjust)`（仿 206）。
    - 辅助 `adjust_value(idx)` / `adjust_index_from_value(s)`（未知值回退 0/qfq）。
    - `set_adjust(&mut self, idx)`（仿 set_timeframe 1105-1116：同步 SharedState.adjust → 无条件 fetch_bars）。
    - Group B（1395-1411）Tag→Dropdown：`Dropdown::new(&tokens, [...]).id_salt("adjust").selected(self.adjust_index).width(96.0)`，`if !is_index` 守卫保留，`.show(ui)` 变化时 set_adjust。
    - main.rs:3934 zh/en 键值对照表：旧行 `("toolbar.adjust", "前复权", "Adj.")` 替换为三行。

### 阶段 D：测试（RED 先行 → GREEN）
12. **门禁 3.5 对抗性测试 RED**（`subagent_skwy_adversarial_test`）：先写失败测试。典型攻击面：
    - adjclose NULL（SZ300683 2202 行形态）→ factor 1.0；close<=0；非有限 adjclose（NaN/inf）
    - 最新日比率除零/未定义（全 NULL 序列、单 bar 序列）
    - 指数序列 ratio=1.0（SH000001）→ qfq/hfq/none 三档结果相同
    - 1w/1M 聚合：qfq 除最新日比率与 hfq 缩放差异；空序列/无行返回
    - config 非法值 `default_adjust` → 回退 qfq；下标越界（adjust_index 超范围）
    - 切换竞态：连续快速切换 last request wins（若可测）；fetch 返回后 bars 变化触发指标重算指纹
13. **门禁 4 需求验收测试 RED**（`subagent_skwy_requirement_test`）：
    - fetch_bars 三档往返正确性（SZ002832 真实数据形态：qfq 最新日=现价、hfq=adjclose/close 缩放、none=close）
    - default_adjust 透传（config=qfq 默认）；SharedState.adjust 同步
    - set_adjust 触发 fetch_bars（重载断言）
    - dispatch_symbol_fetch 带 adjust（SEPA 联动）
    - 指数/板块隐藏 Dropdown（`crates/compass/tests/requirement_index_market.rs:95-103` `toolbar_adjust_tag_has_index_hide_guard` 更新断言）
    - main.rs 测试 `render_toolbar_renders_adjusted_price_tag`（2035-2046）改三档下拉断言 + 新增切换重载测试（仿 2048+）
14. **GREEN**：实现后跑全部测试通过。

### 阶段 E：文档同步（5b）+ 决策记录（5c）
15. `.dsh/kb/design/data-providers.md`：
    - 96 行 stock_adj_factor 表说明更新（adjclose 为后复权口径）
    - 194-196 指数 adjclose=close 占位（保留说明）
    - 280-290「前复权（ref #176）」章节 → 三档复权说明
    - 决策记录表（527/529/566 行）：529 行 fetch_bars 前复权条目追加三档决策
16. `.dsh/kb/design/ui.md`：76 工具栏示意、181-182 与 240-241 前复权 Tag 两处 → Dropdown 三档、357 时间线。
17. `.dsh/kb/user/gui.md`：93-94、108 行。
18. `.dsh/kb/user/config.md`：87/103/122 行 default_timeframe 附近加 `default_adjust`。
19. `.dsh/designs/adjust-mode.md`（已含决策记录表 ✓，5c 自包含）；`.dsh/plans/adjust-mode.md`（本文件）随实现提交。

### 阶段 F：验证 → commit → review → push 门禁
20. 本地验证：`cargo check`（subagent 允许）/ `just check`（fmt+clippy+test）/ 覆盖率门槛（compass-core 95%、compass 90%）。
21. 独立 QA 复核：GREEN 后委派 `subagent_skwy_requirement_test` 独立验证（验证者≠实现者）。
22. commit→`subagent_review`→修复（最多 2 轮）。
23. **等用户 push 确认** → 加载 skwy-reflect 写反思提交 → `git fetch origin master` + rebase（若落后）→ push → issue #345 完成 comment + 关闭。

---

## 3. 关键风险与注意点

- **qfq 归一化锚点 ≡ 序列内最后一个 ratio 有效的行**（§1.5 语义；「最新交易日」即最后一个有效行，尾部 NULL adjclose 时锚点前移）：adjudicate 时以当前查询窗口内最后有效比率为准——锚点定义与 handoff 一致（全序列最新有效交易日），保持与指标计算同窗口语义；聚合路径（1w/1M）同样基于日线层最后有效比率归一，先缩放后聚合。**注意**：不得实现为「最后一行（可能无效）」——尾部 NULL（SZ300683 形态）时锚点会错。
- **接口签名变更波及面**：`DataProvider::fetch_bars` 所有实现与调用点一次性改齐（duckdb/parquet/synthetic/backend/bench 调用），遗漏会编译失败——grep 全量核对。
- **旧 i18n 键删除**：确认无外部引用（仅 main.rs:1405 与两处测试），否则 key-completeness 测试失败。
- **切标的联动**：adjust 是会话级状态（不因标的切换重置），index→stock 切换控件恢复显示且沿用原档位。
- **覆盖率**：新数据层分支（三态+归一化）需足量单测，防 compass-core 95% 门槛被拉低。

## 4. 完成定义（Done）

- [x] 三档切换在 UI 生效（qfq 最新 bar=现价 / hfq 后复权 / none 原始），SEPA 面板最新价不变（代码审查：backend.rs 未触碰；真实数据验证 SZ002832 qfq 最新=25.11）
- [x] 指数/板块控件隐藏（kittest + 源扫描测试），指数三档结果恒等（ratio=1.0 fixture 测试 + SH000001 真实数据 adjclose==close 验证）
- [x] `default_adjust` 生效（serde default + 启动链路测试）、运行中切换不持久化（set_adjust 无 save 调用，代码审查确认）
- [x] RED 测试（对抗性 24 + 需求 13）全部转 GREEN；`just check`（fmt/clippy/test）通过；覆盖率门槛（compass-core 95%/compass 90%）待 CI 验证
- [x] `.dsh/kb/` 五文件同步 + 决策记录齐备
- [ ] commit→review→（用户确认）push→issue #345 收尾（推后核验后勾选）
