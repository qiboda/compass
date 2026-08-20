# 工具链问题排查卡

> **历史注记（ref #264）**：本文件含 OpenCode 时代排查卡（如 GitHub MCP 401 的
> `~/.config/opencode/github-token` 配置、`opencode.json` MCP environment、opencode
> bash 工具无 TTY 等），OpenCode 已退役、DSH 下不适用；卡原文保留可追溯。新排查卡
> 按 DSH 语境追加。

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

### [Git] rebase 冲突手动解决后残留冲突标记（`>>>>>>>` 行）

- **症状**: rebase 冲突手动编辑解决后，某文件残留
  `>>>>>>> <sha>` 冲突结束标记行（手动合并时漏删），后续 commit 把
  垃圾行带进仓库；`git status` 已显示 resolved 不报错，无自动拦截
- **根因**: 手动解决冲突时逐块删除 `<<<<<<<`/`=======`/`>>>>>>>`，
  结束标记漏删；`git add` 后 git 不再检查冲突标记
- **排查路径**: 解决后 `grep -rn "^<<<<<<<\|^=======\|^>>>>>>>" --include="*.rs" --include="*.py" --include="*.md" .` 全仓扫描
- **修复**: `sed -i '/^>>>>>>> /d' <file>` 删除残留行；再 grep 校验零残留
- **验证**: grep 扫描零输出；`git diff` 确认删除正确
- **教训**: 手动解决 rebase 冲突后，`git add` 前必须 grep 全仓冲突标记
  残留（ref #266）

---

## 进程（检测 / 结束）

### [进程] `pgrep -f` 自匹配导致 PID 持续变化假象（两次复发）

- **症状**: `pgrep -f "target/debug/compass"` 返回的 PID **每次执行都不同**
  （或反复出现）；多轮 kill 后仍"检测到进程"，误判"有进程在重启"；
  极端情况下 `pkill -f` 杀掉**执行 shell 自身**，命令/会话挂死
- **根因**: `pgrep -f <pattern>` / `pkill -f <pattern>` 按**完整命令行**
  匹配所有进程——执行该命令的 bash（含 bash 工具持久会话的 `bash -c`
  包装）命令行本身就含 pattern 字样，**每次 pgrep 都匹配到自己新起的
  bash** → PID 持续变化假象。`pkill -f` 则可能把执行 shell 卷进 kill 范围
- **排查路径**:
  1. 先怀疑自匹配：`pgrep -f "target/debug/compass"` 连跑两次，
     PID 变化即自匹配特征
  2. 用 `-x` 精确进程名复核：`pgrep -x compass` —— 只匹配进程名恰为
     compass 的进程，**不匹配命令行**，无自匹配问题
  3. 或 `ps aux | grep "[t]arget/debug/compass"`（`[t]` 破坏自匹配）
- **修复**: 检测一律 `pgrep -x <binary>`（精确进程名）或 `[x]` 技巧；
  `pkill` 前先 `pgrep -x` 列出核对，`pgrep -x compass | xargs -r kill`
  （`-r` 无匹配时不报错）；禁用 `pkill -f "<路径>"` 形式
- **验证**: `pgrep -x compass` 无输出 = 进程确已干净退出（不再是
  PID 变化假象）；窗口可见性另以 `wmctrl -l` / `xdotool search` 为准
  （进程存在 ≠ 窗口可见，ref #105）
- **教训**: `pgrep -f` 自匹配已两次复发（ref #105、ref #226）——若在
  process.md 纪律章节已写"检测/结束 GUI 进程的正确姿势"却仍走 `-f`，
  先回查该章节（`.dsh/kb/dev/process.md` 调试技巧）再动手；长链命令
  （pkill;sleep;build;tmux;sleep;pgrep）分步执行，避免 bash 工具超时
  留下半启动状态

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

### [测试] rust-i18n v4.2.1 `[package.metadata.i18n] default-locale` 是 no-op（#222 i18n epic）

- **症状**: compass-ui / compass 单测里 `t!()` 解析到英文而非设计默认中文——
  例如 `t!("sepa.table.rank")` 得 `"Rank"` 而非 `"排名"`，zh-literal 断言
  （`get_by_label("排名")`/`"自选股为空"`/`"共 3 行"`）全部失败
- **根因**: rust-i18n 的 `CURRENT_LOCALE` 初始硬编码 `"en"`
  （`rust-i18n-4.2.1/src/lib.rs:15`）。`i18n!` 宏按 `default-locale` 生成的
  初始化分支是
  ```rust
  if "zh" != rust_i18n::locale().deref() {
      rust_i18n::set_locale(rust_i18n::locale().deref()); // 保持当前
  } else {
      rust_i18n::set_locale("zh");
  }
  ```
  当 current="en"、default="zh" 时走 if 分支 `set_locale("en")` → 默认值
  **永不生效**（两分支都保持现状）。`[package.metadata.i18n]` 官方文档标注
  仅服务于 `cargo i18n` CLI，宏虽读它生成此分支但行为如上——不是我们配置错
- **排查路径**: 用最小 /tmp 工程复现（Cargo.toml 含
  `[package.metadata.i18n] default-locale = "zh"` + zh/en 各一 key）：
  `cargo run` 打印 `initial locale: en` / `resolved: Chart`；`RUST_I18N_DEBUG=1
  cargo check` 确认宏生成的 `set_locale` 分支文本
- **修复（当前）**: 各 crate 仍加 `[package.metadata.i18n] default-locale = "zh"`
  （compass-i18n 已有；T6 补 compass-ui + compass）——无运行时副作用、表达
  契约意图、上游修复后即刻生效。**单测要 zh 默认必须显式 `set_locale("zh")`**
  （产品路径 `main()` 已显式调用，无碍；测试需 LANG_LOCK 串行保护，见 #222
  plan T14/T15）。勿依赖 metadata 让测试静默变 zh
- **验证**: /tmp 复现工程输出 locale=en 即证；workspace 内 `cargo check` 通过
  不证明运行期 locale（宏展开产物在 RUST_I18N_DEBUG=1 下可查）
- **教训**: 第三方 i18n 库的 "default-locale" 配置不一定改运行期初始 locale——
  任何"默认 zh"的测试契约都要在测试侧显式 set_locale，不能赌配置生效

### [测试] 子分组 scope `max_rect` 高度用 INFINITY → 组内控件及卡片下方全部 NaN/inf rect（#245 交互测试发现）

- **症状**: screener builder 一旦存在子分组（`CondItem::Group`），其内部控件
  （组头 Segmented、组删除 X、组内 add menu）与**组之后的全部内容**（根 add
  menu、卡片下方的「筛选」按钮）在 egui_kittest 树中 rect 为
  `[[NaN inf] - [NaN inf]]`（空分组占位 label 高度 `inf`）。kittest 点击
  这些节点被静默丢弃（click 事件落在 NaN/inf 坐标 → 无 hit）——
  `filter_click_compresses...` 式断言 saved=None 且无任何 error，易误判为
  "点击没生效"
- **根因**: `render_group_items`（citizens/screener.rs）对子分组用
  ```rust
  ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_max(
      start, egui::pos2(start.x + row_w, start.y + f32::INFINITY))))
  ```
  scope 高度 = INFINITY；叶子卡片行用有限值
  `start.y + tokens.spacing.control_md`。wrap 布局（`horizontal_wrapped` +
  `Align::Center`）在 available height=INF 下垂直居中计算
  `(INF - h)/2` → inf/NaN → 组内控件 rect 全毁；scope 结束后外层 wrap
  cursor.y 累加 INF → 组之后所有内容（根 add menu、筛选按钮）位置也变
  NaN/inf。**生产同样受影响**（egui 布局与 harness 无关——最小复现仅
  condition_builder + 一个 Group 即出 NaN）
- **排查路径**:
  1. kittest 树查 rect：`query_all_by_label_contains("添加条件")` 两个节点
     rect 均为 `[[NaN inf] - [NaN inf]]`（无组时根菜单 rect 有限）
  2. 最小复现：仅 `panel.condition_builder(ui, &[], &[])` + 预置一个
     `CondItem::Group(CondGroup::default())`，固定窗口 1000x1400，`run()`
     后仍 NaN（排除结果表/窗口尺寸因素）
  3. 对照：无组 6 卡布局所有 rect 有限；叶卡 scope 高度有限（control_md）
  4. 验证点击被丢弃：对 NaN 节点 click + step 后状态无变化，且
     `screener_error` 也 None（无任何副作用 = 事件未命中）
- **修复**: ✅ 已修复（commit 24f70d3 后续补丁）。子分组 scope 的 max_rect
  高度改用有限值——`ui.available_rect_before_wrap()` 的底部（与叶卡一致只
  约束 x 全宽、y 用当前可见底部）而非 INFINITY。修后 rect 全部有限，
  组内与组后控件可点击
- **验证**: ✅ 已修。`cargo test -p compass citizens::screener::tests`
  28/28 全绿；`add_sub_group_and_nested_cards_compile_nested_and` 保留
  "建子组"真实交互 + 视图模型加卡断言嵌套 And AST（kittest 对嵌套 scope
  二次 popup 点击仍有限制——见下）；`restore_query...` 3 卡 restore 渲染
  断言 + 扁平形状保存 round-trip 全绿
- **遗留限制（#245）**: kittest 无法稳定点击**嵌套 scope 内 popup 的第二次
  选项点击**——第一次（根组 popup）成功，第二次（子组内 popup 打开后点
  选项）被 Dropdown 的 click-outside 关闭逻辑吞掉（Area hover 状态时序）。
  同类限制已见于 multi_select 的 Area/ScrollArea 注释。子组内加卡交互由
  `add_condition_via_root_menu_appends_card`（根组加卡）覆盖交互层，
  嵌套结构由视图模型层断言
- **教训**: kittest 节点 rect 为 NaN/inf 时点击会**静默丢弃**——先查 rect
  再查行为；UI 断言先验证"节点存在且 rect 有限"再断言交互结果。
  `fit_contents()` 会随内容测量放大此症状（INF 内容 → 窗口尺寸失真），
  交互测试用固定大窗口更稳；`Harness::run()` 对持续 repaint（popup 动画
  request_repaint_after）会在 4 帧后 panic，popup 交互用单帧 `step()`

### [测试] Python 脚本批量正则改 Rust 构造点误伤字段声明（E0573）

- **症状**: 用 Python 脚本对多个 Rust 测试 fixture 批量插入新字段
  （如 `name_en: None`）时，正则条件过宽误匹配**字段声明**行
  （`name: &'static str`），插入后变 `name: name_en: None ...` 类型错误
  E0573，多个文件连锁编译失败，多轮返工
- **根因**: 批量替换目标应是**结构体字面量**（`Foo { name: ..., }`），
  但 `name: .*,` 类模式同时命中 `pub name: Type` 字段声明行——声明与
  构造点同形，无排除条件
- **排查路径**: `cargo check` 报 E0573 的定位行 → 检查脚本替换模式是否
  覆盖声明行（`grep -n "pub.*name"` 对照）
- **修复**: 正则条件加排除（`^(?!.*pub)` / 跳过含 `:` 后接类型大写字母的
  行）；改后立即 `cargo check` 验证；失败先 `git checkout` 恢复再精确重做
- **验证**: `cargo check --tests` 全绿；`git diff` 核对仅构造点被改
- **教训**: 批量脚本改代码前先区分「字段声明」vs「构造点」两类匹配点，
  用条件排除声明行；改后立即编译验证，不反复修补

### [测试] cargo 输出 grep 计数与 llvm-cov percent 单位误解

- **症状**: ①`grep -c "test result: ok"` 返回 0 误判"测试未跑"（实际
  输出含 ANSI/换行差异）；②llvm-cov JSON 的 percent 字段解读错误
  （如 `9521.2%` 其实是 covered/count 比值放大，需自己算）
- **根因**: 对工具输出格式假设错误——grep 模式与真实输出不匹配
  （多行/前缀/转义），percent 字段非直观百分比
- **修复**: ①计数用 `grep -cE "^test result: ok"` 或直接看
  `tail -N` 原始输出；②llvm-cov 用 `covered/count` 自己算百分比
- **验证**: 与原始输出逐行对照
- **教训**: 统计类命令输出先验证格式语义再采信（ref #266）

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
  保留最后一行）——回测代码对真实数据输入防御。**数据管线侧根因**由
  [issue #181](https://github.com/qiboda/compass/issues/181) 修复：
  import 不再剥 SH/SZ/BJ 前缀（恢复 Dolt-native 前缀符号），指数 SH000905
  与股票 SZ000905 不再汇合为同一 000905，混源行随之消除。原建议方向
  （import 去重或排除指数代码）当时被前缀恢复方案取代、未采纳；
  **后续由 [issue #201](https://github.com/qiboda/compass/issues/201)
  落地 import 侧指数剔除**——`compass-data import` 无条件剔除
  SH000300/SH000852/SH000905/SH000906/SH000985/SZ399300（主查询 +
  symbols.txt 枚举），parquet 不再含任何指数行，混源彻底消除
- **验证**: 冒烟重跑——strategy -9.63%、benchmark -13.93%、excess +4.30%，
  NAV 曲线合理（0.85-1.01）；`cargo test -p compass-strategy backtest` 含
  `dedup_bars_keeps_last_row_per_symbol_date` 全绿；#181 修复后实测
  stock_daily.parquet (symbol, tradedate) 重复行 = 0

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
  数据管线根因由 issue #181 修复——import 恢复前缀，指数与股票不再混源）
- **验证**: 全窗口 benchmark 从 296% → 101.7%，单日最大跳变从 +94% → +4.8%；
  新增 `benchmark_skips_absurd_returns` 测试锁定；#181 修复后 benchmark
  单日收益区间回归 [-9.7%, +4.0%]（F3 实测），无 >100% 伪影残留

### [CI] Python Test 失败：/data 权限（GitHub runner 无 /data 写权限）

- **症状**: PR CI 的 Python Test job 失败——`tests/test_common.py::TestCsvDir::test_env_unset_returns_default`
  `PermissionError: [Errno 13] Permission denied: '/data'` + `FileNotFoundError: /data/compass-data/csv`；1 failed, 326 passed
- **根因**: `csv_dir()` 默认返回 `/data/compass-data/csv`（master commit 8d7bca4 引入统一 CSV 目录），
  该测试在 env 未设 COMPASS_CSV_DIR 时断言默认路径——**GitHub runner 无 /data 权限**（Errno 13），
  而本地（有 /data）3 个 TestCsvDir 测试全部通过。与 #210 技能迁移无关（该 PR 零 Python 变更）。
- **排查路径**: `gh pr checks 212` 看失败 job → `gh run view <id> --log-failed` 定位失败测试与
  PermissionError → 本地 `uv run pytest tests/test_common.py::TestCsvDir -q` 通过（有权限）→
  git diff 确认 PR 无 Python 变更 → 判定为预存 CI 环境问题（源自 8d7bca4）
- **修复**: 待处理——CI runner 需创建 `/data/compass-data/csv` 并授权，或测试改为 mock
  csv_dir 的默认路径（不依赖真实 /data）。**影响所有后续 PR 的 Python Test job**。
- **验证**: 本地测试通过；CI 需环境修复后重跑

### [Git/环境] pre-push hook pytest 报 `pydantic_core._pydantic_core` ModuleNotFoundError

- **症状**: `git push` 时 pre-push hook 的 `cd collectors && uv run pytest` 失败，traceback 显示导入
  `/home/skwy/.hermes/hermes-agent/venv/lib/python3.11/site-packages/langsmith/schemas.py` 后
  `No module named 'pydantic_core._pydantic_core'`。
- **根因**: hermes agent 环境设置了 `PYTHONPATH=/home/skwy/.hermes/hermes-agent/venv/lib/python3.11/site-packages`，
  git hook 子进程继承该变量 → collectors 的 py3.12 venv 启动时 PYTHONPATH 注入 py3.11 的 site-packages，
  pytest 插件发现 langsmith（py3.11 包），其 `pydantic_core` 是 cp311 二进制，py3.12 解释器无法加载。
- **排查路径**: 1) 单跑 `uv run pytest` 复现（错误同上）；2) 检查 `echo $PYTHONPATH` 发现 hermes 泄漏；
  3) `env -u PYTHONPATH uv run pytest tests/` 通过（328 passed）——隔离变量即修复。
- **修复**: push 时用 `env -u PYTHONPATH git push origin <branch>`（仅清除泄漏变量，hook 本身无问题）。
- **验证**: `env -u PYTHONPATH uv run pytest tests/ -q` → 328 passed。

### [覆盖率] cargo llvm-cov nextest 报 double-spawn "No such file or directory"（一次性构建竞态）

- **症状**: `cargo llvm-cov nextest --json --summary-only` 首次运行失败：
  `error: creating test list failed` → `[double-spawn] failed to exec ".../compass_core-<hash>" "--list" "--format" "terse"` →
  `No such file or directory (os error 2)`，cov.json 为空（0 字节）。
- **根因**: nextest 的 double-spawn 机制在 compass_core 测试二进制**尚未链接完成**时就尝试 `--list` 枚举
  测试——大型二进制（987MB）链接慢，nextest 竞态触发。二进制在错误后仍会完整生成（时间戳在错误之后）。
- **排查路径**（教训：先重跑再深挖）: 1) 检查 `target/llvm-cov-target/debug/deps/` 里 compass_core 二进制
  是否存在——错误时可能不存在、稍后出现；2) 确认无残留 cargo/nextest/llvm-cov 进程竞争 target 目录；
  3) 直接**重跑同一条命令**——竞态类错误重跑即过。
- **修复**: 无需修复——干净重跑成功（EXIT=0）。非环境/配置问题。
- **验证**: 重跑 `cargo llvm-cov nextest --json --summary-only --output-path ...` 后
  `bash scripts/check-coverage.sh` 8 项全 OK（2026-08-12，ref #250 首次遇到）。

### [数据] index_daily 导入必败：CSV 缺 update_date 列（采集器契约断裂）

- **症状**: `python main.py import index_daily` 报 `column "update_date" could not be found in any table in scope`，
  index_daily 表建好后永远 0 行；index_basic 正常（1000 行）。2026-08-15 首次真实采集（1000 板块，3.5h）暴露。
- **根因**: `fetch_index_daily.py::_kline_records()` 构造的 record 字典**不含 `update_date` 键**，而
  `common.write_csv()` 按首个 record 的 keys 推断 CSV 表头 → 真实 `index_daily.csv` 缺 `update_date` 列；
  但 `DAILY_INSERT_COLS` 与 INSERT SQL 引用该列 → 导入必败。**测试盲区**：`TestImportToDolt` 用手工
  header（含 update_date）+ `_write_csv()` 拼 CSV，绕过了 `run() → write_csv()` 真实路径，缺陷从未暴露。
- **排查路径**: 真实导入报 SQL 列缺失错误 → 对比 CSV 表头（9 列）vs `DAILY_INSERT_COLS`（10 列）→
  检查 `_kline_records` 与 `write_csv` 键推断逻辑 → 确认测试用手工 header 绕过真实路径。
- **修复**: `_kline_records()` record 补 `record["update_date"] = today_iso`（issue #273）；新增端到端
  测试 `tests/test_index_daily_e2e.py` + 对抗测试 `tests/test_index_daily_adversarial.py` 走真实
  run() → CSV → import_to_dolt() 路径（不再用手工 header）。
- **验证**: 451 passed / cov 95%；真实数据导入成功（2759 行，update_date 无 NULL）。
- **教训**: 采集器测试必须覆盖 `run() → write_csv() → import_to_dolt()` 真实链路，手工构造 CSV
  的 import 测试会掩蔽列契约断裂——与 stock_daily 混源教训同理：数据级 bug 只能靠真实链路暴露。

### [数据] 东财反爬封禁导致采集器空转数小时（快速失败机制）

- **症状**: 2026-08-15 首次真实采集指数数据时东财 push2his 反爬封禁（45 请求/2 分钟触发，
  IP 级 HTTP 000 全镜像封锁）；`fetch_index_daily.py::run()` 对失败标的仅打印 FAILED/empty 后
  `continue`，955 板块 + 30 官方指数 × 6 次尝试 ≈ 3.5 小时空转后才结束。
- **根因**: `run()` 没有连续失败计数器——封禁后所有标的必然连续失败，但采集器仍逐个重试全部标的。
- **排查路径**: 真实采集日志显示大量 `FAILED`/`empty (skipped)` 后仍继续请求；对照
  `.dsh/evidence/index-fetch-resume-2026-08-15.md` 确认封禁阈值与影响范围。
- **修复**: issue #277 —— `run()` 维护跨 board/official 循环共享的连续失败计数器：
  - 失败定义 = `_get_json` 返回 None（所有 host×attempt 用尽）或 empty klines；
  - 连续 **5 个**标的失败立即终止，不再请求剩余标的；
  - 终止前把已抓 daily/basic 记录写入 CSV（保留可续采），再抛 `RuntimeError` 提示
    “连续 N 个标的失败（疑似反爬或接口故障）”；
  - 成功即清零；official code-mismatch skip 既不计数也不清零；
  - `common.py::EM_MIN_INTERVAL` 0.5s → **2.0s**，并同步
    `fetch_fin_indicators.py` / `fetch_stock_basic.py` 的局部限流常量 → **2.0s**
    （全局限流调大，覆盖全部 EastMoney 采集器）。
- **验证**: `test_fast_fail_requirement.py` + `test_fast_fail_adversarial.py` 17 用例覆盖
  连续终止、CSV 保留、交错不误杀、4/5 边界、跨循环计数、skip 语义、Progress failed 状态；
  全套件 `pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` 全绿。
- **教训**: 采集器对“连续失败”必须有止损机制；反爬场景下空转比失败本身更昂贵。

### [测试] Dolt 遥测/更新检查导致 pytest 在无网络环境挂起

- **症状**: 在 worktree 里跑完整 `pytest collectors/tests/` 时随机卡死（无输出推进），
  `ps` 可见 `dolt send-metrics` 后台进程和一个 `dolt --data-dir ... sql` 子进程长期 `Sl`；
  单独跑单个测试文件正常。
- **根因**: Dolt CLI 会触发遥测/更新检查，在无外网或网络受限环境可能阻塞子进程，
  导致 `subprocess.run(...)` 不返回；测试顺序/次数不同表现为随机挂起。
- **排查路径**: `timeout 60 pytest ... -v` 定位最后一个未完成测试；`ps` 看到
  `dolt send-metrics` + 卡住的 `dolt sql`；用 `DOLT_DISABLE_TELEMETRY=1
  DOLT_DISABLE_UPDATE_CHECK=1` 重跑即通过。
- **修复**: 测试/CI 环境设置 `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1`
  再跑 Dolt 相关测试。
- **验证**: 禁用后完整前缀 140 用例 60.9s 通过，全套件 + coverage 全绿。
- **教训**: 本地/CI 无外网时，Dolt 遥测是隐藏的挂起源；涉及 Dolt 的测试命令应显式禁用遥测。
- **完整 collectors 套件约 3 分钟（620 tests）**：优先后台运行（`run_in_background`）并带
  `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1`，避免前台超时/遥测挂起（ref #286 摩擦）。

### [数据] 东财封禁下官方指数爬取：快速失败机制误伤 + name_en 列缺失

- **症状**: ① `python main.py fetch index_daily` 在东财封禁下跑：板块段（无 fallback）连续 5 个失败即触发
  #277 快速失败终止，**官方指数段（有腾讯 fallback）根本轮不到**——fallback 形同虚设；② `import index_daily`
  时报 `Unknown column 'name_en' in 'index_basic'`。
- **根因**: ① run() 执行顺序为 boards → official，快速失败计数器跨两段累计，封禁下板块段必然先打满 5 连败；
  ② #266（data-name-i18n）合并后 `BASIC_DDL` 加了 `name_en` 列，但 Dolt 里 `index_basic` 是**旧 schema**——
  `CREATE TABLE IF NOT EXISTS` 不会改已有表，导入 SQL（`LEFT JOIN _tmp_name_en` + `name_en` 列）报列缺失。
- **排查路径**: ① 看 run() 日志尾部 `RuntimeError: 连续 5 个标的失败` → 读 run() 确认 boards 先于 official →
  判断快速失败在板块段即触发；② import 报错定位到 `_import_index_basic` 的 INSERT 引用了 `name_en` →
  `SHOW CREATE TABLE index_basic` 确认 Dolt 旧表无此列。
- **修复**: ① 封禁期间不跑完整 run()，用一次性脚本只拉官方指数段（复用 `_fetch_tencent_kline` +
  `_kline_records`，产出同格式 CSV）——不改生产代码；② `dolt sql -q "ALTER TABLE index_basic ADD COLUMN name_en VARCHAR(100)"` 补列后重导。
- **验证**: 30 个官方指数全量入库（145,215 行，上证 8531 根自 1990-12-19），index_basic 1030 条（含 30 official），
  update_date 无空值；Dolt commit chmn88d 已推送。
- **教训**: ① 快速失败计数器跨段累计时，fallback 段若排在无 fallback 段之后会被误伤——段间应重置计数器或
  官方段前置；② Dolt schema 变更（DDL 加列）对已存在的表不生效，`CREATE TABLE IF NOT EXISTS` 只建新表——
  列增变更需显式 `ALTER TABLE` 迁移或 DROP 重建。

### [数据] 同花顺行业板块采集：JSONP 包装 + 7 字段列序与东财不同

- **症状**: 按 plan「与东财同构，复用 `_kline_records`」直接解析同花顺 K 线，
  high/close 被静默互换（数据损坏）；JSONP body 直接 `json.loads` 失败。
- **根因**: ① 同花顺 `d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js` 返回
  **JSONP**（`quotebridge_v4_line_bk_881121_01_2024({...})`），需剥函数包装后
  `json.loads`；② `data` 字段是 `;` 分隔的 11 字段 CSV 行，**列序为
  `日期,开,高,低,收,量,额`**，与东财 `日期,开,收,高,低,量,额` 不同——
  直接复用 `_kline_records`（东财序）会把 high/close 互换。
- **排查路径**: curl 实测 2024 年数据（20240102: 开7035.381/高7035.458/低6925.206/
  收6927.422）→ 与东财 11 字段样例逐列对比 → 确认列序差异。
- **修复**: `fetch_ths_kline` 剥 JSONP + `json.loads`（兼容裸 CSV 测试形态），
  每行取前 7 字段并重排为东财序（`[0,1,4,2,3,5,6]`）后交给 `_kline_records`。
- **验证**: 90 行业按年分页全量拉取（2026→2007，空年提前终止）入库，数值合理性
  抽样核对。
- **教训**: 「同构」类复用必须先验证**列序**而非仅字段集合；JSONP/裸 JSON 双形态
  解析要兼容（真实 API 与测试 fixture 可能不同）。

---

## 容器 / Docker

### [容器] Docker bridge 网络创建 veth 失败 "operation not supported"

- **症状**: `docker compose up -d` 创建默认 bridge 网络成功，但启动容器时报
  `failed to add the host (veth...) <=> sandbox (veth...) pair interfaces: operation not supported`；容器反复 Restarting。
- **根因**: 当前 DSH sandbox/运行环境不允许创建 bridge veth 对（缺少相应
  CAP_NET_ADMIN 或沙箱网络限制）；host 网络可正常创建容器。
- **排查路径**:
  1. `docker info` 看网络驱动（bridge/host 均列示）——驱动存在不代表可用
  2. `docker network ls` 确认网络已创建但容器无法挂接
  3. `docker run --rm --network host <image> ...` 验证 host 网络可用
- **修复**: 本次验证使用临时 host-network compose override
  （`/tmp/proxy_pool-host-compose.yml`），提交的 compose 保留标准 bridge 配置
  以兼容正常 Docker 主机；在受限环境验证时用 override。
- **验证**: host override 下 `docker compose up -d` 容器 Up，API 可访问。
- **教训**: 沙箱环境网络能力与用户真实主机可能不同；环境相关 workaround 不要
  静默写进交付物，用临时 override 并记录。

### [容器] jhao104/proxy_pool 官方镜像缺少 bash，默认 ENTRYPOINT 崩溃

- **症状**: `docker compose up` 后 proxy_pool 容器反复
  `[FATAL tini (7)] exec bash failed: No such file or directory`。
- **根因**: `jhao104/proxy_pool:latest` 是 Alpine 镜像，未安装 bash，但镜像
  Dockerfile 的 ENTRYPOINT 是 `tini -- bash proxy_pool.sh ...`；官方镜像开箱即坏。
- **排查路径**:
  1. `docker logs <container>` 看到 `exec bash failed`
  2. `docker run --rm --network host --entrypoint sh jhao104/proxy_pool:latest -c 'which bash || echo no-bash; cat /etc/os-release'`
     确认 Alpine 且无 bash
  3. 读官方仓库 `Dockerfile` / `proxy_pool.sh` 确认 ENTRYPOINT 与脚本依赖 bash
- **修复**: compose 中覆盖 entrypoint 为 `["/bin/sh","-c"]`，直接启动
  `python proxyPool.py server & python proxyPool.py schedule & wait`（绕过 bash 脚本）。
- **验证**: 容器 Up，`curl http://127.0.0.1:5010/all/` 返回 JSON 代理列表。
- **教训**: 第三方镜像的 ENTRYPOINT 可能与其实际镜像内容不一致；交付 compose 时
  应在目标镜像上实测，不能只照官方 docker-compose 抄。

### [容器] jhao104/proxy_pool:2.4.2 缺少 patch；沙箱 build RUN 需 --network=host

- **症状**: 本地 Dockerfile `RUN patch -p1 < validator.patch` 构建失败：先报
  `failed to set up container networking: ... operation not supported`（bridge veth），
  改用 `docker build --network=host` 后报 `/bin/sh: patch: not found`。
- **根因**: ① 沙箱不允许 bridge 网络创建 veth，build 的 RUN 阶段默认 bridge 也失败；
  ② 上游 `jhao104/proxy_pool:2.4.2`（Alpine）未安装 `patch`，补丁镜像需先
  `apk add --no-cache patch`。
- **排查路径**:
  1. `docker build -t ... scripts/proxy_pool` → 看到 bridge veth 错误（与运行容器同因）
  2. `docker build --network=host -t ... scripts/proxy_pool` → 网络错误消失，暴露 `patch: not found`
  3. `docker run --rm --network host --entrypoint sh <image> -c 'which patch || echo no-patch; cat /etc/os-release'`
     确认 Alpine 且无 patch
- **修复**: Dockerfile 采用多阶段构建——build 阶段在 `RUN patch` 前增加
  `RUN apk add --no-cache patch` 并应用补丁；final 阶段从上游基础镜像复制补丁后的
  `/app/helper/validator.py`，运行时镜像不包含 patch 二进制与补丁文件。受限沙箱
  构建/运行使用 `--network=host`（交付的 compose 保留标准配置，受限环境用临时
  host override）。
- **验证**: `docker build --network=host -t proxy_pool_https_validator:local scripts/proxy_pool`
  成功（多阶段 build ID `ad3cc044c1d0`）；final 镜像内 `which patch` 不存在、
  `/app/validator.patch` 不存在，`sed -n '71,77p' /app/helper/validator.py` 显示
  https key 已改为 `http://`。
- **教训**: 对第三方 Alpine 镜像打补丁前先确认基础工具是否安装；沙箱网络限制同时
  影响 build RUN 与容器运行，统一用 host network 验证。

### [DSH] 子代理工具整体不可用 "subagent run failed"

- **症状**: 本 session 委派 `subagent_skwy_adversarial_test` / `subagent_skwy_requirement_test` 后台运行立即失败（"failed before it finished. It left no closing message."）；恢复重试同样失败；前台通用 `subagent` 最小任务报 `Error: subagent run failed`；`dev_mode_subagent` 返回空结果。
- **根因**: 子代理运行基础设施故障。用最小任务排除任务内容/权限因素；session 日志（`session.jsonl.zstd`）中工具结果仅 `Error: subagent run failed`，无更深层错误；DSH 服务日志不可读，无法从 session 内修复。
- **排查路径**:
  1. 委派一个最小任务（"回复一句话"）到 `subagent` 前台 → 同样失败，排除任务复杂性
  2. 同时启动两个 `subagent_skwy_*` 后台 → 均无产出无 closing message
  3. 用 `send_message` 恢复子代理 → 再次立即失败
  4. `dev_mode_subagent` 对照 → 返回空
  5. 解压 `~/.dsh/sessions/<session>/session.jsonl.zstd`，grep `subagent run failed` 定位 tool/result，确认无堆栈
- **修复**: 本次无法在 session 内修复；经用户批准采用 fallback——由主 agent 按 `skwy-adversarial-test` / `skwy-requirement-test` 方法论亲自编写 RED 测试（记录失去认知独立性）。
- **验证**: fallback 后测试文件落盘 `collectors/tests/test_collectors_proxy_{adversarial,requirement}.py`，运行 pytest 得到预期 RED（导入失败）。
- **教训**: 子代理委派是门禁硬性要求，但基础设施不可用时必须显式报告根因 + 用户批准 fallback，不得静默绕行；后续若 DSH 子代理恢复，应重新委派独立 QA 复核。
- **2026-08-19 复发/根因**：本次前台+后台子代理全失败，孩子日志（`~/.dsh/sessions/<workspace>/<child-id>/session.jsonl.zstd`）定位 `429 GoUsageLimitError: Weekly usage limit reached...`（OpenCode Go 周配额）。根因是子代理继承父 agent 创建时旧 `AgentOptions`，session 中途换模型不生效；已修 deepseek-harness `resolveChildAgentOptions`（优先读 `parent.session.requestHeader()?.config`，commit fbd193a），DSH 重启后子代理恢复。排查时务必先解压孩子会话日志看 `turn/end` 的 `reason.error`。

### [DSH] str_replace_editor 大段替换会“成功”但吞掉内容

- **症状**: `str_replace_editor` 对较大 old_str/new_str 返回 "edited successfully"，但实际结果里 new 内容缺失、old 内容也被删（如 fetch_freeproxy 的 `fetch_json_proxies` 整体消失、`proxy_pool_client.get_proxy` 方法体变空、测试函数体被清空）；grep 验证才发现。
- **根因**: 本环境 `str_replace_editor` 在长替换（几十行以上）时不可靠，疑似工具内部截断/替换 bug；`edit` 工具（普通）未出现此问题。
- **排查路径**:
  1. 替换后立即 `grep -n "def ..."` 或 `cat` 目标函数确认存在
  2. 发现内容缺失时查看 git diff / 文件行号定位被吞范围
- **修复**: 大段/整函数替换改用 `write` 全量重写文件，或 `edit` 工具（精确唯一 old_string）；避免对关键代码用 str_replace_editor 做大段替换。
- **验证**: 重写后运行 pytest/语法检查通过。
- **教训**: 工具返回成功≠内容正确；编辑后必须验证函数体/关键行存在（尤其涉及多行替换时）。

### [环境] investment_data 路径硬编码 PROJECT_ROOT/investment_data 与真实 Dolt 仓库不一致

- **症状**: `scripts/sync-investment-data.sh` 与 `collectors/main.py::sync_investment_data()` 硬编码
  `PROJECT_ROOT/investment_data`（即 `/data/codes/compass/investment_data`），但实际 Dolt 仓库在
  `/data/compass-data/investment_data`，导致同步脚本报仓库不存在/未初始化。
- **根因**: 脚本/代码以项目目录下 `investment_data` 为假设路径，而本机数据仓库统一放在
  `/data/compass-data/` 下。
- **修复**: 创建符号链接
  `ln -s /data/compass-data/investment_data /data/codes/compass/investment_data`；
  `.gitignore` 原规则 `investment_data/` 不匹配符号链接，需改为 `investment_data`（无斜杠）才能忽略。
- **验证**: 符号链接后 `scripts/sync-investment-data.sh` 重跑成功，fast-forward 到
  `nta67cibl6412uhg3oo5dmeffcf1775e`。
- **教训**: 环境路径差异不要硬编码到脚本；若保留现有硬编码，需保证符号链接/统一数据目录约定并记入 toolchain。

### [collectors] stock_basic 导入后残留 `_tmp_name_en` 临时表

- **症状**: `collectors sync` 后 Dolt `compass_data` status 出现 untracked `_tmp_name_en`（105 行）；
  手动 `dolt sql -q "DROP TABLE IF EXISTS _tmp_name_en"` 后消失。
- **根因（初步）**: `collectors/common.py:361` 定义 `NAME_EN_MAPPING_TMP="_tmp_name_en"`，
  `main.py::import_stock_basic` 的 `finally` 调 `drop_name_en_mapping()`，但该清理未实际生效；
  且 `dolt_sql` 调用不检查 returncode，drop 失败被静默吞掉。根因未完全定位。
- **修复**: 本次手动 DROP 清理；代码修复待后续（检查 drop_name_en_mapping 是否在正确 finally 路径/是否正确提交事务）。
- **教训**: Dolt SQL 工具调用应检查 returncode；临时表清理失败不能静默。

### [compass-data] import-compass 增量 merge fallback 用 since 过滤数据覆盖 parquet，导致历史丢失（issue #298）

- **症状**: `import-compass --table capital_main_flow --since 2026-08-20` 增量 merge 报
  `DuckDB merge failed: Binder Error: Set operations can only apply to expressions with the same number of result columns, falling back to full export`；
  随后 `capital_main_flow.parquet` 只剩 2026-08-20 一天 5544 行，Dolt 表共 27706 行。
  进一步检查发现 dragon_list（parquet 278 / Dolt 6143）、block_trade（118 / 19654）、
  institution_survey（478 / 304848）、index_daily（528116 / 528476）也均少于 Dolt，疑为历史 fallback 覆盖累积。
- **根因**: `crates/compass-data/src/import_compass.rs` `import_append_table` fallback 分支（约 497-503 行）
  在 merge 失败时执行 `std::fs::write(&path, &new_data)`，而 `new_data` 是 `WHERE date_col >= since`
  的查询结果，并非日志声称的 “full export”——增量数据覆盖全文件导致历史丢失。
  merge 失败的具体 schema 不匹配原因未能在复现中定位（parquet 已被覆盖后无法重现，需保留旧文件才能诊断）。
- **修复（本次数据恢复）**: 对 5 个 append 表（capital_main_flow, dragon_list, block_trade, institution_survey, index_daily）
  执行无 `--since` 的 `import-compass` 全量重导，parquet 行数与 Dolt 一致后重跑 SEPA。
  代码修复（fallback 改为真正全量导出或报错退出、保留现场日志）见 issue #298。
- **验证**: 5 表 parquet 行数 = Dolt 行数（27706 / 6143 / 19654 / 304848 / 528476）。
- **教训**: 任何 fallback 若覆盖数据文件，必须保证覆盖内容与声明一致（全量）；数据文件写操作前后应校验行数，
  不能静默用增量数据覆盖历史。
