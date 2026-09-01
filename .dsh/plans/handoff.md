# Handoff — adjust-mode worktree

## 用途
K 线图复权方式切换（前复权/后复权/不复权）+ 修复 adjclose 口径错误。
Issue: **https://github.com/qiboda/compass/issues/345**（C-Feature, A-GUI, A-Data）
分支：`feat/adjust-mode`（基于 master 38bd8dc）。
（注：本目录 .dsh/plans/ 中其余 .md 为 master 已提交的历史 plan 文件，随 checkout 带入，与本任务无关。）

## 背景（用户报告）
比音勒芬 SZ002832：SEPA 面板最新价 25.11（close 不复权），点击后 K 线图显示 150.97（×6.0123）。
根因：Dolt `final_a_stock_eod_price.adjclose` 实为**后复权**（Tushare 源；首日 2016-12-23
adjclose==close=37.68 锚点；adjclose/close 比率单调递增 1.0→6.0123），而 ref #176 前复权化
`adjust_ohlc`（`crates/compass-core/src/indicators.rs`）假设 adjclose 为**前复权**
（最新日 adjclose==close → factor=1.0）→ factor=6.0123 放大整序列 → 图表显示后复权价。

## grill-me 锁定决策（shared understanding）
1. **范围**：仅 K 线图表显示（主图表 + SEPA 弹出图表，均走 `fetch_bars`）跟随复权三档切换；
   SEPA 面板「最新价」保持 `close`（现实价/市值口径，backend.rs:486 `latest: last.close`）；
   选股器/回测不变（前/后复权日收益率数值相同，不受口径影响）。
2. **UI**：工具栏 Group B（周期 1d/1w/1M 旁）把静态「前复权」Tag（main.rs:1404-1409）换成
   Dropdown 三档（前复权/后复权/不复权）；组件库已有 `Dropdown`
   （`crates/compass-ui/src/widgets/dropdown.rs`）。UI 细节先委派 subagent_ui_designer。
3. **指数/板块**：沿用 `is_index` 判断隐藏切换控件（adjclose=close 占位，无复权概念），
   指数图表永远 factor=1.0（不复权）。
4. **默认与持久化**：config 新增 `default_adjust`（默认 `qfq`，与 default_timeframe 对称）；
   运行中切换不持久化（重启回默认，与现有 timeframe 行为一致）。
5. **前复权锚点**：全序列最新交易日（`factor_i' = (adjclose_i/close_i) ÷ 最新日比率`），
   最新 bar 缩放后 = 现价（修复 150.97 现象）。三档：qfq=归一化前复权、hfq=adjclose/close（当前行为）、
   none=factor=1.0（raw close）。**注意 adjclose 可能为 NULL**（factor 回落 1.0）；1w/1M 聚合路径
   （duckdb.rs SQL scale 逻辑）同样三档支持（先缩放后聚合）。

## 下一步（worktree 会话执行）
1. **同步原始分支**：先 `git fetch origin && git rebase origin/master`（工作开始前）。
2. 门禁第 1 步：委派 `subagent_ui_designer` 产出 `.dsh/designs/adjust-mode.md`（在 worktree 内），
   向用户展示要点并获确认；确认后同步最终要点到 `.dsh/kb/design/ui.md`（权威文档）。
3. 门禁第 3 步：DSH plan mode 制定计划（2+ 模块：compass-core 读取层 + compass GUI + i18n +
   config）；无 plan mode 指示时用普通消息 + ask_user_question 呈现（ref #267/#266），
   `.dsh/plans/*.md` 写入 worktree。
4. 门禁 3.5/4 步：委派 subagent_skwy_adversarial_test / subagent_skwy_requirement_test 写失败测试（RED）。
5. 门禁 5b/5c：文档同步（`.dsh/kb/design/data-providers.md` 前复权章节、`.dsh/kb/user/gui.md`、
   `.dsh/kb/user/config.md`、`.dsh/kb/design/ui.md`、决策记录）+ 对应 issue commit `ref #345`。
6. 实现 GREEN → 本地验证（cargo test/clippy/fmt+真实数据冒烟：对比 002832 三档价格）→
   commit → 五角度审查 → 反思 → 等用户 push 指令 → push 后 issue #345 收尾（comment + close）。
