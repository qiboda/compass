# 工具链问题排查卡

本文件记录执行过程中遇到并解决的问题，按**问题排查卡**格式沉淀可复用的
排查路径（而非事件流水账）。每条卡片的「排查路径」是核心价值——下次遇到
同类问题直接照做。

> 触发来源：AGENTS.md 品质准则「问题处理闭环」——执行中任何异常禁止静默
> 绕行，必须 感知 → 诊断 → 处理 → 记录（ref #159）。每条记录对应一次闭环。

## 使用方式

- 遇到新问题：先走问题处理闭环，处理完按下方格式追加卡片
- 遇到疑似已知问题：先在本文件搜索症状关键词，命中则直接照「排查路径」复现诊断
- 卡片按类别分组；同类问题多了再考虑归纳为 skill/脚本（内容多了再升级位置）

## 卡片格式

```
### [类别] 一句话问题摘要
- **症状**: 现象（报错信息/异常行为）
- **根因**: 定位到的根本原因（含 why）
- **排查路径**: 可复用的诊断步骤（命令级，本次怎么查出来的）
- **修复**: 改了什么
- **验证**: 怎么确认修好了（复现命令/预期输出）
```

---

## 工具链（MCP / 外部服务）

### [工具链] MCP github server 401 "Requires authentication"

- **症状**: 调用 MCP github 工具（如 create_issue）报
  `GitHubAuthenticationError: Requires authentication` (status 401)；
  但 gh CLI 同操作成功
- **根因**: server-github v0.6.2（npm 2025.4.8）的 `dist/common/utils.js`
  认证逻辑**只读取 `GITHUB_PERSONAL_ACCESS_TOKEN`** 环境变量；opencode 配置
  设的是 `GITHUB_TOKEN` → Authorization header 从未注入 → GitHub 返回 401。
  stdio 握手（initialize）成功 ≠ 认证可用——401 只在真实工具调用时暴露
- **排查路径**:
  1. curl 直连验证 token 本身有效（排除 token 失效）：
     `curl -s -o /dev/null -w "%{http_code}" -H "Authorization: Bearer $(cat ~/.config/opencode/github-token)" https://api.github.com/user` → 200
  2. 检查 MCP 进程实际注入的环境变量（排除配置展开问题）：
     `cat /proc/<pid>/environ | tr '\0' '\n' | grep GITHUB` → 确认变量名与值
  3. 读 server 源码确认读取哪个变量：
     `grep -n "process.env.GITHUB" <npm-cache>/@modelcontextprotocol/server-github/dist/common/utils.js`
  4. 用正确变量名 stdio 直测 MCP 调用复现/验证（见「验证」）
- **修复**: `~/.config/opencode/opencode.json` 中 github MCP environment
  变量名 `GITHUB_TOKEN` → `GITHUB_PERSONAL_ACCESS_TOKEN`
- **验证**: `GITHUB_PERSONAL_ACCESS_TOKEN=<token> npx @modelcontextprotocol/server-github`
  后发 `tools/call create_issue` → 返回 issue 对象（而非 401 错误）。注意：
  本机 opencode 需重启才重新注入环境变量

---

## 数据管线（compass-data）

### [数据管线] `import --since` 是过滤子集 + 覆盖，不是增量追加

- **症状**: `cargo run --bin compass-data -- import --since 20260801` 后，
  `stock_daily.parquet` 从 689MB/1829 万行缩为 237KB/5534 行——历史行情
  全部丢失，只剩 2026-08-03 一天
- **根因**: `import`（stock_daily 路径，`import_dolt.rs::run`）**没有 merge
  逻辑**。`--since` 只是给 SQL 加 `tradedate >= '...'` WHERE 过滤，然后
  `write` + `rename` **原子覆盖**整个 parquet 文件。与 `import-compass`
  （`import_compass.rs`，有 `since && !overwrite && path.exists()` 的 merge
  路径）行为不同——两者都叫"增量"，语义迥异。AGENTS.md/文档中
  "`import --since` 增量"的注释是误导（2026-08-03 事故直接诱因）
- **排查路径**:
  1. import 后立即检查文件大小：`ls -lh parquet_data/stock_daily.parquet`
     （721MB→237KB 即事故信号）
  2. 验证数据范围：duckdb 查询
     `SELECT MIN(tradedate), MAX(tradedate), COUNT(*) FROM read_parquet(...)`
  3. 确认根因：读 `crates/compass-data/src/import_dolt.rs::run`——
     找 `std::fs::rename(&tmp_path, &final_path)`（原子覆盖），对比
     `import_compass.rs` 的 merge 分支
- **修复**: 数据已从 Dolt 源全量重建（`import` 不带 `--since`）。文档已更正：
  `import --since` 描述改为"过滤子集直写，会覆盖全文件"；同步流程改用
  全量 `import` 或 `import-compass --since`（后者有 merge）
- **验证**: `SELECT MIN(tradedate), MAX(tradedate), COUNT(*)` → 覆盖完整
  历史（1990-12-19..2026-08-03, 18293598 行）
- **教训**: 文档注释"增量"不等于实现语义。执行破坏性命令（覆盖/删除/重置）
  前，先读源码确认 merge/覆盖行为；Dolt 是权威源，parquet 可重建，但
  GUI 不可用期间就是损失——命令执行前应确认不会破坏现有产物

---

## 编辑器工具链（opencode / LSP）

### [编辑] edit 工具按 oldString 匹配误伤文件内重复片段

- **症状**: 用 edit 在 `fetch_stock_basic.py:167` 加 `# pragma: no cover`
  注释，结果加到了 `:145` 的正常分支 `return []` 上，导致
  `IndentationError: expected an indented block after 'if' statement on line 144`。
  另一例：子代理在 `test_concept_member.py` 末尾追加新类，oldString 匹配到
  `test_run_board_list_fetch_exception_aborts` 的重复结尾片段，新类插入到
  类中间，使既有测试 `test_run_empty_board_list_aborts` 落进新类作用域
  （AttributeError: 'TestFetchBoardMembers' object has no attribute '_make_get'）
- **根因**: edit 是字符串精确匹配，不感知代码结构。文件内重复片段
  （`return []`、`if __name__ == "__main__":`、断言+文件存在性检查的收尾块）
  会命中**第一个**出现位置，而非目标位置。LSP 的"could not be resolved"
  类报错（如 `import pytest`、curl_cffi）是 venv 环境噪音，容易让 agent
  把真实语法错误也当噪音忽略
- **排查路径**:
  1. 目标行在文件中不唯一时（短片段/重复收尾块），先用
     `grep -n "<片段>" <file>` 确认出现次数与目标行号
  2. edit 的 oldString 必须带**足够上下文**（前一行 + 目标行 + 后一行）
     使匹配唯一；或直接引用行号附近的独特文本
  3. edit 后立即 `python3 -m py_compile <file>`（Python）或 LSP diagnostics
     验证语法；再用 `grep -n "pragma\|class \|def "` 抽查结构归属
  4. 子代理完成编辑后，主 agent 应抽查文件结构（类/方法归属），
     不只信子代理自报
- **修复**: 撤销误匹配处（改回原文本），在正确位置带上下文重新 edit；
  结构错位的测试类用 edit 把方法移回原类
- **验证**: `python3 -m py_compile <file>` 通过；`grep -n` 确认目标行
  带注释、正常分支无注释；`pytest <file> -q` 全绿（结构错位时
  AttributeError 消失）
- **教训**: 编辑重复片段前先 grep 计数；oldString 带足上下文；编辑后
  立即编译/结构验证——禁止凭"看起来对了"跳过验证

---

## Git 工作流（hooks / push）

### [Git] pre-push hook 拦截"修复失败 CI 的 PR"（死锁）

- **症状**: push 报
  `ERROR: Latest CI run on master is FAILING. Fix CI before pushing.`，
  但本 PR 正是修复该 CI 失败（#169 修复 toast flaky，master CI 因 #169 红）
- **根因**: `.githooks/pre-push` 第 0 步无条件检查 master 最新 CI run，
  若 `conclusion=failure` 直接 `has_error=1`。但"master CI 失败"的原因
  往往就是某个 open flaky issue（如 #169）——修它的 PR 必然撞上该检查，
  形成"CI 红了才能修，修了不让推"的死锁
- **排查路径**:
  1. `gh run list --repo qiboda/compass --branch master --limit 1` 确认失败 run
  2. 判断失败 run 是否与本次 PR 修复的目标一致（如 #169 的 toast flaky 测试）
  3. 若一致 → 属已知问题，hook 其余门禁（fmt/clippy/doc/ref #N）已跑过则
     `git push --no-verify` 绕行；若不一致 → 先修真正的 CI 失败再推
- **修复**: 本次 `git push --no-verify -u origin fix/toast-flaky-test`
  （hook 的 fmt/clippy/doc/ref 检查全部通过，仅 CI 状态检查误拦；用户批准绕行）
- **验证**: `git push --no-verify` 成功推送分支，`gh pr create` 正常创建 PR #170
- **教训**: pre-push 的 master-CI 检查应区分"未知/已知问题"——已知 open issue
  的 CI 失败应放行修复 PR（或提供 `--allow-ci-failure` 白名单机制），
  否则修复 flaky 的 PR 永远无法正常推送（ref #168 #169）
- **根治（ref #172）**: 该 master-CI 检查已从 `.githooks/pre-push` **整体删除**，
  CI 门槛移交 master branch protection（9 个 required status checks, strict）
  在 merge 侧强制。`--no-verify` 绕行与 `--allow-ci-failure` 机制均不再需要
  ——修复失败 CI 的 PR 可直接正常 push，PR CI 全绿后才能 merge。

### [Git] `git rebase --continue` 在 agent 无 TTY 环境挂起（编辑器等待）

- **症状**: agent（opencode bash 工具，无 TTY）执行 `git rebase --continue`
  时**挂起直至超时**（实测 120s、60s 两次），同一操作在交互式终端正常；
  冲突已解决并 `git add` 后仍挂起
- **根因**: `git rebase --continue` 提交时会启动**默认编辑器**（`core.editor`
  未设置 → vi/nano）确认 commit message——无 TTY 环境挂起等待输入。
  与 commit-msg hook 的 gh API 调用无关（gh 实测 2s 响应）
- **排查路径**:
  1. `ps aux | grep -E "git rebase|git-commit|vi|nano"` 确认无卡死进程；
     实际是命令自身挂起（bash 工具超时杀掉）
  2. 排除 hook 慢：单独 `time gh issue view <N> ...` 测 gh API 响应
  3. **验证根因**：加 `GIT_EDITOR=true` 后 rebase 秒级完成（实证：4 个
     commit 全部顺利重放）——确认是编辑器调用
- **修复**: agent 无 TTY 环境执行 rebase 相关命令前置
  `GIT_EDITOR=true`（或 `git -c core.editor=true rebase --continue`）；
  涉及多个 commit 冲突时用 `GIT_EDITOR=true setsid nohup git rebase ... &`
  后台执行 + 日志文件轮询
- **验证**: `GIT_EDITOR=true git rebase --continue` 冲突解决后立即完成
- **教训**: 任何可能触发 git 编辑器的命令（rebase --continue、reword、
  `git commit` 交互式 message）在 agent 环境必须显式
  `GIT_EDITOR=true`——"文档已固化但未遵守"同类：规则写入（本卡）
  需执行侧习惯（每次 rebase 前想起 GIT_EDITOR=true）（ref #189）

---

## 测试（Rust / egui_kittest）

### [测试] egui_kittest 动画测试受 wall-clock 影响偶发失败（慢 CI）

- **症状**: `compass-ui widgets::toast::tests::test_render_expired_toast_closes_then_is_removed`
  在 CI 慢 runner 上间歇失败：`assert len()==2` 得到 1，报
  "expired toast is closing, not removed"（ref #155 修复后仍偶发，ref #168/#169）。
  本地快机基本复现不出——正是 flaky 特征
- **根因**: `render()` 用**真实墙钟** `Instant::now()` 驱动动画
  （`close_progress`/`is_expired`/`close()` 全基于它）。kittest `Harness::run()`
  在慢 CI 上因 `wait_for_images`（字体纹理首载）触发 `sleep(step_dt=250ms)`
  循环——一帧 sleep 即超过 CLOSE_DURATION(100ms) → toast 在断言前被移除。
  本地无 pending images → 直接 break → 通过。测试与真实时间耦合 = 天生 flaky
- **排查路径**:
  1. 确认失败模式：`gh run view <id> --log-failed` 看 `assertion left == right failed`
  2. 读 `egui_kittest` 源码确认时间语义：`RawInput` 构造 `..Default::default()`
     → `time: None`；`InputState::begin_pass` 用
     `new.time.unwrap_or(self.time + predicted_dt)` → **虚拟时间累积**，
     每 step 推进 `step_dt`（默认 0.25s）
  3. 确认产品代码时间源：`grep -n "Instant::now\|elapsed()" toast.rs` →
     render/render_toast 全用墙钟
- **修复**: 动画改由 egui 虚拟时间驱动——`render()` 取
  `ctx.input(|i| i.time)`（f64 秒，每帧按 predicted_dt 推进，kittest 下确定），
  `Toast.created_at`/`close_started` 改 f64；`ToastManager` 缓存
  `last_frame_time`（push 在 ctx 外，用它作 created_at）；测试 harness 用
  `Harness::builder().with_step_dt(0.01)` 细粒度推进，`run_steps(11)` 精确
  越过 100ms 关闭动画
- **验证**: `cargo nextest run -p compass-ui widgets::toast::tests::test_render_expired_toast_closes_then_is_removed`
  连续 20 次无失败；`cargo test` 全量通过。正确性由虚拟时间构造保证，
  与机器负载无关
- **教训**: kittest 测试断言跨帧动画状态时，**绝不用真实墙钟**（`Instant::now()`、
  `elapsed()`）——必须用 egui 虚拟时间（`ctx.input(|i| i.time)`）或注入时钟。
  慢 CI 上任何"重置时间戳再 run()"的 workaround 都有残留竞态


- **第二实例（ref #171，已根治）**: `compass-ui widgets::modal` 与 toast 同根因——
  产品代码 `open()`/`close()`/`show()` 用 `Instant::now()` 驱动动画，测试
  4+4 处"重置时间戳"workaround（modal.rs 402/588/622/651 + main.rs 1746-47/
  1960-61/2005-06/2021-22）。#168 排查路径逐字复用：modal 动画改 egui 虚拟
  时间（`open(now: f64)`/`close(now: f64)` 显式收参），测试 harness 改
  `with_step_dt(0.01)` + `run_steps(11)`（modal.rs）或直接删 workaround

  依赖默认 `step_dt=0.25` 一 step 跨过动画（main.rs）。根治后动画路径已无墙钟
  残留（compass-data/strategy 的 `Instant` 仅剩性能计时，与动画无关）。教训同
  toast：#171 验证后"重置时间戳"模式在库内已无实例

---

## GitHub CLI / Hook

### [GitHub] commit-msg hook 误报 "issue #N is MISSING"（gh API 瞬时故障）

- **症状**: commit 时 commit-msg hook 报
  `ERROR: commit rejected — issue #154 is MISSING (must be OPEN)`；立即
  重试同一 commit 即成功；手动 `gh issue view 154 --repo qiboda/compass
  --json state --jq '.state'` 返回 OPEN
- **根因**: hook 内 `gh issue view ... 2>/dev/null || echo "MISSING"` 中 gh
  API 调用瞬时失败（网络抖动/限流），stderr 被吞 → 误判 MISSING。非 issue
  状态问题、非认证问题（`unset GITHUB_TOKEN` 后 gh 用 hosts.yml 凭据正常）
- **排查路径**:
  1. 手动跑 hook 同款命令确认 issue 真实状态：
     `unset GITHUB_TOKEN; gh issue view <N> --repo qiboda/compass --json state --jq '.state'`
  2. 若返回 OPEN → 判定为瞬时故障，直接重试 commit
- **修复**: 无需代码修复——瞬时故障，重试即过。hook 设计上 2>/dev/null
  吞错误导致误报误导，但重试成本低可接受
- **验证**: 同 commit 重试成功；连续多次 commit 无复发

## 数据（Dolt / Parquet）

### [数据] stock_daily.parquet 同 symbol 同日重复行（指数代码混源）

- **症状**: `sepa backtest` 冒烟输出荒谬指标——strategy 累计 -100%、
  benchmark 累计 1.48e15%、benchmark 日收益 +41895%。逐日调试发现
  symbol 000905/000852/000906（中证500/1000/800 指数代码）的 series 中
  **同一天有两行**：如 000905 在 2026-06-30 同时有 adjclose=9031.38
  （指数点位）与 21.5（另一数据源净值）
- **根因**: stock_daily.parquet 中部分指数代码混入两套数据（指数点位 +
  另一源），同一 (symbol, date) 出现多行。回测的 day-over-day 收益计算
  跨数据源比较（9028.93/21.5−1 = +41895%），daily_returns 的 insert 覆盖
  又产生 -99.75% 假收益 → 策略/基准 NAV 失真
- **排查路径**:
  1. 冒烟输出异常 → 写临时 `#[ignore]` 测试加载真实 parquet，打印
     `fetch_cross_section` 返回的 sample symbols/adjclose，定位异常 symbol
  2. 打印该 symbol 完整 series，发现同日多行（两套数据并存）
  3. 对比 Dolt `final_a_stock_eod_price` 正常数据，确认 parquet 侧污染
- **修复**: 回测入口 `run_backtest` 增加 `dedup_bars`（同 (symbol, date)
  保留最后一行）——回测代码对真实数据输入防御。**数据管线侧根因未修**：
  stock_daily.parquet 生成（import）应去重或排除指数代码，跟踪于
  [issue #181](https://github.com/qiboda/compass/issues/181)
- **验证**: 冒烟重跑——strategy -9.63%、benchmark -13.93%、excess +4.30%，
  NAV 曲线合理（0.85-1.01）；`cargo test -p compass-strategy backtest` 含
  `dedup_bars_keeps_last_row_per_symbol_date` 全绿

### [性能] 回测逐日 run_sepa 重复读取 7 份数据（全窗口 40+ 分钟）

- **症状**: `sepa backtest` 全窗口（385 天）40+ 分钟未完成；单日 ~3.1s。
  `RUST_LOG=debug` 显示每日期 `fetch_ms≈3000`、`compute_ms≈210`（fetch 占 93%）
- **根因**: `run_backtest` 逐日调用 `run_sepa`，而 `run_sepa` 每次独立 fetch
  7 份数据（550 日 cross-section + stock_basic + concept_member +
  capital_main_flow + dragon_list + block_trade + institution_survey）。
  385 天重复读取 380 次（累计 rchar 255GB）——IO 是瓶颈，compute 只占 7%
- **排查路径**:
  1. 加 tracing 量化：`scoring.rs` 各 fetch 单独计时 + `backtest.rs` 每日/阶段
     计时（`RUST_LOG=debug cargo run ... sepa backtest`）
  2. 日志显示 `fetch_ms / compute_ms` 占比 → 确认 IO 主导
  3. `/proc/<pid>/io` 看 rchar 累计（255GB）证实重复读取
- **修复**: 拆分 `run_sepa` 为 `fetch_sepa_window`（预取 `[start-1-550, end]`
  全窗口一次）+ `score_sepa`（逐日从内存切片 `[now-550, now]` + 原 compute）。
  `run_sepa` 公共 API 不变（fetch + score 组合）。`run_backtest` 用预取 +
  逐日 `score_sepa` → 全窗口 40+ 分钟 → ~95 秒（提速 ~25 倍）
- **验证**: 24 天窗口优化前后结果一致（-9.63%）；全窗口 385 天 94-97 秒；
  `cargo test -p compass-strategy` 60 用例全绿；覆盖率 compass-data 95.2%
- **教训**: 性能优化前先量化（tracing），确认瓶颈是 I/O 而非 compute；
  逐日调用"重计算引擎"时，数据预取是收益最大的优化点（消除重复读取）

### [数据] 指数混源导致 benchmark 单日 +94%（日期交错，dedup 无法处理）

- **症状**: 回测全窗口 benchmark 单日 +94.3%（2025-03-11）；strategy 无异常
- **根因**: 000905/000852 等指数 symbol 的两套数据（点位 ~9000 与净值 ~21.5）
  在**相邻日期交错**（非同日重复）——`dedup_bars` 只去重同 (symbol, date)，
  无法处理跨日期交错 → `day_return_on` 算出 cur/prev = 9000/21.5（+41895%）
- **排查路径**: 打印 benchmark 各日收益排序，定位异常日期 → 检查该日 top 300
  成分的 series 是否有价格跳变
- **修复**: `compute_benchmark_returns` 加收益合理性守卫——跳过
  `|ret| ≥ 100%` 的成分（A 股单日涨跌停 ≤±30%，>100% 必为数据伪影；
  数据管线根因仍由 issue #181 跟踪）
- **验证**: 全窗口 benchmark 从 296% → 101.7%，单日最大跳变从 +94% → +4.8%；
  新增 `benchmark_skips_absurd_returns` 测试锁定
