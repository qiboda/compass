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
