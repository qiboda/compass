# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

**归档机制**：教训已融入流程（AGENTS.md 规则、skill 步骤、hook、回归测试、CI 门禁）
的条目不再具活性参考价值，归档至 `.dsh/kb/dev/reflections-archive.md`（历史可查）。
主文件仅保留仍具活性参考价值的条目（最近 + 教训未完全固化者）。

**自动归档（skwy-reflect 第 5 步，ref #238）**：本文件超过 500 行时自动归档一次
——值得处理的条目建 issue 后归档、已处理的直接归档、剩余的保留待下次归档时
重新检阅；归档后仍超 500 行则交用户判断。

## 2026-08-15 — ref #265 justfile：便捷启动与常用命令集

**What was done**: 根目录新增 `justfile`（9 recipes：run/build/test/fmt/clippy/check/import/export/backup，run 为默认 recipe，`just` 即启动 GUI）+ 两批回归测试（需求 22 断言 `justfile-test.sh` + 对抗 18 项 `justfile-adversarial-test.sh`）+ 文档同步（AGENTS.md Commands、gui.md 启动、index.md 快速开始）。2 实现 commit（351c19a / 3a52eba），RED→GREEN 全流程，subagent_review 无 blocking（P1×1 已修）。

**User corrections**（逐字引用对话记录）:
1. "算了那就不修复了。 改为安装just工具，让我能方便的启动项目" —— 否决我推荐的 `default-members` 修复方案（cargo run 默认 compass），转向 just 方案。grill 确认环节正常运转：用户对推荐方案行使否决权并给出新方向。
2. "push" —— 授权推送。

**What went wrong**:
1. **对抗测试 agent 的 self-GREEN 自验 fixture 与需求契约不一致**：其 check 顺序断言 `grep -nFx 'cargo fmt'`（-x 全行匹配）在契约行 `cargo fmt -- --check` 下永远落空；其临时自验 fixture 用的是 `cargo fmt`（无 `--check`）所以"ALL 18 PASSED"。真实 justfile 落地后该断言失效，脚本在 set -e 下静默崩退（rc=1 零诊断）。与 ref #235 "RED 因错误原因失败"同族：测试 agent 自述的验证结论（RED/GREEN）都必须独立复核，不能直接采信。
2. **主 agent 修复测试 bug 时只修了第一处**：首修仅改了 fmt 行的断言（`grep -nFx 'cargo fmt -- --check'`），review P1 抓出 clippy/test 两行**同类问题**（同模式 grep 无匹配即 set -e 崩退）——"修一处"在重复模式断言前是陷阱，应 grep 同模式全部出现点一次修全。
3. **review 抓出 requirement 脚本 tempdir 无 trap**（P3）：set -e 中断会残留临时目录，已补 `trap 'rm -rf "$TMPDIR_X"' EXIT`。
4. 工具层摩擦（子代理侧已自行规避）：bash 工具用 heredoc 写多行 justfile 时换行偶被吞，改用 printf 逐行/拷贝现成文件可靠。

**Lessons learned**:
1. 测试 agent 的 self-GREEN 模拟必须使用与需求契约**逐字一致**的 fixture，并对关键断言做 mutation 负面验证（削弱实现 → 断言必须 FAIL）；主 agent 收到自验报告后在真实实现上重跑两批测试再采信。
2. 修复测试/代码中的重复模式断言 bug 时，先 grep 同模式全部出现点再统一修复（本次 `grep -nFx` 三连只修一处，review 第二轮抓出同病两处）。
3. 对"测试脚本在 set -e 下静默崩退"的防御：所有命令替换的 grep 管线加 `|| true`，让空结果走到清晰 FAIL verdict 而非无诊断退出。

**Process improvements**:
- .dsh/kb/dev/testing.md「脚本自测」章节新增：两个 justfile 回归测试脚本记录 + 「委派测试 agent 的自验可信度」条款（自验 fixture 与契约逐字一致 + mutation 负面验证 + 主 agent 重跑采信），已随本次反思 commit 落地

### Trends (last 10)
- **测试 agent 自验结论失真变体再现**（ref #235 RED 断言目标语义错误 → 本次 #265 self-GREEN fixture 与契约不一致）：测试 agent 的 RED/GREEN 结论都不能直接采信——主 agent 独立复核（真实实现重跑 / 逐断言核对语义）应成为固定步骤；testing.md 已固化条款
- **子代理交付/自验可靠性系列第 5 次**（ref #244 零交付 → #245 只分析 → #255 截断零落盘 → #235 断言失真 → 本次自验 fixture 失真）："委派后核验"持续靠主 agent 手动补救，至今未固化为 skill/hook——建议正式写入 skwy-workflow 委派协议（proposed，连续 5 批未落实）
- **独立 review 持续抓出主 agent 盲区**（#255 → #264 → 本次测试脚本静默崩退 P1 + trap P3）：审查-修复闭环价值稳定实证，保持强制
## 2026-08-15 — ref #273 指数采集真实运行：CSV 缺 update_date 列修复 + 首次采集暴露反爬封禁

**What was done**: 首次真实运行 epic #255 指数采集管线（1000 板块，3.5h），暴露 `_kline_records()` 缺 `update_date` 键导致 CSV 缺列、Dolt 导入必败（测试用手工 header 掩蔽契约断裂）；修复 + 新增 e2e/对抗测试走真实 run()→CSV→import_to_dolt() 链路。采集后期被东财 push2his 反爬封禁（HTTP 000 全镜像），仅 45 概念板块入库（index_daily 2759 行 + index_basic 1000 行），官方指数 30/行业 496/概念 459 待解封后续采（记录在 .dsh/evidence/index-fetch-resume-2026-08-15.md）。

**User corrections**: 无纠正类消息。用户追加要求："push2his 反爬拉黑，什么时候能好呢？先记录下当前的拉取范围，方便下次继续拉取"——已将续采指引与缺失清单落盘 .dsh/evidence/。

**What went wrong**:
1. 采集 3.5h 全程盲等：后台任务用 `| tail -40` 管道缓冲输出，48 次 job_output 轮询看不到进度——应先探测板块总数（504+496=1000）估算时长，或用 `> logfile 2>&1` 直写日志按需 tail
2. 提交被 pre-commit hook 拦截一次（ruff 4 错误：测试文件 F401/I001）——本地应先自跑 `uv run ruff check *.py tests/` 再提交
3. edit 一次失败（file changed since read，需 re-read 重试）；compress 一次失败（seq 不在 surface）
4. 提交 2585829 落 master（存在活跃 worktree collector-progress/data-name-i18n，但均为他任务）：判定 #273 为单模块 Python bugfix（collectors 目录内，不产出 plans/designs），按 0.5 步规则无需 worktree——判断依据记录在案

**Lessons learned**:
1. 网络采集类后台任务：先算请求总数（clist 探测）预估时长，日志直写文件而非 tail 管道，避免盲等
2. commit 前自跑项目 lint（`uv run ruff check *.py tests/`）——hook 是最后防线不是第一道
3. 测试子代理产出后主 agent 先过一遍 lint 再提交，避免 hook 拦截返工

**Process improvements**:
- toolchain.md 追加排查卡（采集器测试必须覆盖真实 run()→write_csv→import 链路；手工 CSV 掩蔽列契约断裂）——已随 2585829 提交
- 续采记录 .dsh/evidence/index-fetch-resume-2026-08-15.md + 两份缺失清单——已落盘（待提交）

### Trends (last 10)
- 数据级 bug 反复由真实数据首次暴露（#181 stock_daily 混源、#273 update_date 缺列）：fixture 测试覆盖不到，epic #255 的"真实数据冒烟"步骤在实现时未执行真实采集，本次 3.5h 采集才撞出契约断裂——真实冒烟不应滞后到交付后
- 子代理交付后主 agent 未立即验证（本次 ruff 由 review 子代理发现而非提交前）：与 #244/#255"子代理交付验证"模式同类，commit 前 lint 自跑应成为固定动作

## 2026-08-15 — ref #267 collector-progress：6 个一次性写 CSV 的 collector 抓取进度可查询

**What was done**: 为 6 个一次性写 CSV 的 SEPA collector（main_flow/block_trade/index_daily/institution_survey/concept_member/dragon）接入 Progress 进度跟踪：common.py 新增 Progress 类（原子写 `csv_dir()/<name>.progress.json`，tmp+os.replace，percent clamp [0,100]，path sanitize 防穿越，best-effort 写失败不击穿采集器）；6 个 fetch_*.py 以 `with Progress(...)` 包裹（动态 total、早退 finish、异常自动 fail）；main.py 新增 `progress [target] [--json]` 子命令（choices 收敛为 6 个接入者）。5 个 commit（3b353c5/7f449f3/797198b/127d642/1ee37f0），需求+对抗双测试 agent 共 49 项新测试（RED 7 failed→GREEN），全量 502 passed，5 角度 review 无 blocking。

**User corrections**（逐字引用对话记录）:
1. "继续plan。根据半成品的代码。也许写的方向有问题，需要重写。" —— 用户对我准备"按 handoff 直接收尾半成品"的计划提出方向质疑，要求先基于代码重新评估是否重写。我做了逐文件方向评估（方案形态/原子写/统一接入/动态 total/早退路径/测试覆盖 6 项证据），结论"方向正确无需重写、仅 2 个真实缺口"，用户确认后继续。**质疑驱动了本 PR 最关键的修复**：choices 收敛（requirement RED 5 项）正是评估中发现的缺口。

**What went wrong**:
1. **文档编辑误落 master 工作区（ref #138 教训再现）**：doc-sync 阶段我用 `/data/codes/compass/.dsh/kb/user/cli.md`（master 路径）而非 worktree 路径编辑，commit 前 `git status` 检查 master 才发现两文件改动落在主分支工作区——立即 `git checkout` 还原并在 worktree 重做。虽然发现及时未造成提交污染，但正是 ref #138 记载的"产出文件落 master"摩擦的变体（该次是 untracked 文件，本次是 tracked 文件编辑）；worktree 会话内一切文件编辑必须使用 worktree 绝对路径，AGENTS.md 虽已写"plan/design 在 worktree 内创建"，但**编辑既有文档的路径选择**没有显式规则。
2. **计划批次划分未预见测试重复**：commit 划分假设"5 个 commit"，实际 choices 修复与子命令同文件无法按文件粒度拆分（合并为 4+1 个），review 又抓出 contract 测试与 test_main 重复 5 项——批次规划应早读测试文件评估可拆性。
3. **首次 exit_plan_mode 失败**：当前 session 不在 plan mode，exit_plan_mode 报错后才改用普通消息呈现计划——工具可用性应先确认再调用。

**Lessons learned**:
1. **worktree 会话内编辑任何既有文件（含 .dsh/kb 文档）必须使用 worktree 绝对路径**（`/data/codes/compass/.worktrees/<name>/...`），禁止凭记忆/默认路径落笔；编辑后 commit 前对 master 工作区跑 `git status` 交叉验证无意外改动（本次靠 commit 前检查发现）。
2. **质疑驱动的方向评估应更早触发**：handoff 声称"主 session 已审查设计质量良好"，但用户一句质疑就导向了完整重新评估——半成品的"已审查"结论不应直接采信，首个可编译工作区内先做逐文件证据核验再规划，成本远低于事后返工。
3. 分步 commit 规划前先读测试文件确认拆分粒度可行性；工具（exit_plan_mode）可用性先验证再依赖。

**Process improvements**:
- AGENTS.md 无新增（worktree 路径规则已隐含在 worktree 章节，本条目作为 ref #138 的实践补充记录，暂不重复立规）

### Trends (last 10)
- **ref #138 教训（产出/编辑落 master 工作区）再现变体**：该次是 untracked plan/design 文件，本次是 tracked 文档编辑——worktree 会话的路径纪律仍是薄弱点，值得在 AGENTS.md worktree 章节显式补一句"所有文件操作使用 worktree 绝对路径"
- **用户方向质疑→重新评估→发现真实缺口的模式第 2 次实证**（ref #264 迁移范围 → 本次 choices 收敛）：对"已审查/已计划"结论保持怀疑、以代码证据核验的流程价值持续

## 2026-08-15 — ref #266 epic data-name-i18n：数据名称翻译全链路（B1-B5）

**What was done**: 完成 epic #266 数据名称翻译——index_basic.name_en + stock_basic.industry_en 数据层英文列（Dolt→parquet→DuckDB→GUI 全链路）；collectors/name_en_mapping.csv 静态映射表（index 30 官方译名 / industry 75 标准译名 / concept 486 直译，真实数据覆盖率行业 100%、概念 96.6%、指数 12/12）；GUI locale 渲染（i18n_name.rs display_name + CORE_INDEX_WHITELIST 三元组 + SEPA concept_names/industry_names 映射 + screener shared_en 冲突回退）；搜索三路匹配（symbol/code/name_en，股票恒 None 按 D0-B）；concept 节按名称双 JOIN + COALESCE（PR 审查 P1-1）。10 commits（rebase 后基点 b0729ad，全部在 feat/data-name-i18n），467 Python + 1320+ Rust 全绿，5 轮 subagent_review 全部处理后通过（B1 前置 drop / B2 is_missing_column 收窄 / B3 SC2 碰撞 / PR 级 concept 断裂）。

**User corrections**（逐字引用对话记录）:
1. "批准" —— plan 批准（.dsh/plans/data-name-i18n.md）。
2. D0/D1 裁决（ask_user_question 回答）："B：股票不参与英文搜索"（d0-stock-name-en）、"A：GUI 层概念名映射（推荐）"（d1-theme-translation）——验收 3 修订为 "SSE"→上证指数 可达目标（issue comment 已追加）。
3. "批准，提交 B1（推荐）"（mapping-review）—— 591 行映射表提交确认。
4. "A：concept 节按名称 JOIN（推荐）"（concept-join-fix）—— PR 级审查 P1-1（486 行 concept 死数据）修复方案裁决，用户批准方案 A。
5. "push" —— push 授权。

**What went wrong**:
1. **Python 脚本批量正则改 Rust 构造点误伤字段声明**：为测试 fixture 批量插入 `name_en: None` 时正则条件未排除字段声明行，`name_en: None` 插到 `name: &'static str` 后 → E0573 类型错误；git checkout 恢复 + 条件排除 pub/类型行精确重做，多轮返工。
2. **edit 工具误删测试体开头**：old_string 过短匹配到别处，删掉了测试函数首段 → 重读修复。
3. **测试子代理并发写文件**：多个后台测试 agent 同时改同文件 → 多次 "file changed since it was read" 重读。
4. **fixture 列数与 DDL 不同步**：test_trim_imports fixture 改 13 列后 INSERT VALUES 仍 12 值 → 20 个 trim 测试批量失败。
5. **统计输出解析误判**：`grep -c "test result: ok"` 返回 0 误判测试未跑；llvm-cov percent 字段单位误解（9521.2% → 自己算 covered/count 得 95.2%）。
6. **rebase 冲突残留标记**：master Progress 重构 vs B1 手动解决冲突后残留 `>>>>>>> 28420ce` 行（sed 删除）；rebase --continue 第一轮直接跑超时后才用 GIT_EDITOR=true 后台重跑成功——toolchain.md 已有该排查卡（ref #189），执行侧未第一时间遵守。
7. **exit_plan_mode 不在 plan mode 报错**：先调用后报错，改用普通消息呈现计划（#267 已记录同教训，第二次出现）。

**Lessons learned**:
1. 批量脚本改代码：正则条件必须精确排除"字段声明/类型行"（构造点 vs 声明区分），改后立即 cargo check 验证；失败先 git checkout 恢复再精确重做，不反复修补。
2. edit 工具 old_string 必须带足够上下文锚点（唯一匹配）；编辑测试体先重读确认当前内容。
3. fixture schema 变更必须同步更新同文件所有 INSERT/DDL（列数一致），改后跑全相关测试。
4. 统计类命令输出（grep 计数、覆盖率 percent）先验证格式语义再采信，必要时直接算原始数值。
5. rebase 冲突手动解决后 grep 残留 `<<<<<<<`/`>>>>>>>` 校验；rebase --continue 直接前置 GIT_EDITOR=true（已有排查卡，执行时先想起）。
6. 呈现计划前先确认 session 是否处于 plan mode（exit_plan_mode 仅 plan mode 可用）；不在则普通消息呈现。

**Process improvements**:
- toolchain.md 测试章节追加 2 张排查卡：Python 批量正则改 Rust 构造点误伤字段声明（E0573）、cargo 输出 grep 计数与 llvm-cov percent 单位解析——已随本 commit 落地
- toolchain.md Git 章节追加 1 张卡：rebase 冲突手动解决后残留冲突标记校验——已随本 commit 落地
- AGENTS.md Step 3 补 plan mode 确认注记（exit_plan_mode 可用性）——已随本 commit 落地
- GIT_EDITOR=true 卡既有（ref #189），本次为执行侧未第一时间遵守——记录第二实例，不重复立卡

### Trends (last 10)
- **exit_plan_mode 可用性教训第二次出现**（#267 → 本次）：#267 已记录"工具可用性先验证再依赖"但未固化——本次落实为 AGENTS.md Step 3 注记（趋势报警生效：第二次出现 = 上次未固化）
- **命令输出解析类摩擦跨批再现**（#273 后台 tail 盲等 → 本次 grep 计数误判 + llvm-cov percent 单位）：对工具输出格式先验证再采信——toolchain.md 已追加解析卡
- **GIT_EDITOR=true 卡（ref #189）执行侧再现**（本次 rebase --continue 先直接跑超时）：已有排查卡但未第一时间遵守——"文档已固化未遵守"模式再现，执行侧习惯待养成

## 2026-08-15 — ref #277 collectors: 连续失败快速终止 + 全局限流调大

**What was done**: 实现 `fetch_index_daily.run()` 连续失败快速终止（连续 5 个标的失败/empty 即写已抓 CSV 后抛 RuntimeError），并将 `common.py` / `fetch_fin_indicators.py` / `fetch_stock_basic.py` 的 `EM_MIN_INTERVAL` 全部调至 2.0s；新增 17 个 RED→GREEN 测试，更新 3 个旧测试适配新语义；全套件 553 passed，coverage 98.57%。

**User corrections** (if any): 无显式纠正；用户指令为环境检查 → worktree 启动 → push。

**What went wrong**:
1. **对抗性测试子代理首次委派只返回开场白、未产出文件**：需重新委派才完成（工具/子代理异常，浪费一轮；与历史“子代理交付验证”模式同类）。
2. **完整 pytest 套件随机挂起**：`dolt send-metrics` 后台进程阻塞 `dolt sql`，先盲等 180s 超时；通过 `ps`/子集复现定位，用 `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1` 解决并记录 toolchain.md。
3. **`git commit -m` 第二次提交被 commit-msg hook 拒绝**（消息文件手工测试通过），改用 `git commit -F` 成功——shell 多行传参与 hook 读取差异未完全定位。
4. **Review 发现“全局限流调大”最初只改了 `common.py`**：`fetch_fin_indicators.py` / `fetch_stock_basic.py` 局部常量未同步，handoff“影响全部采集器”的决策最初未完全落实，review 后补改。
5. **全套件第一次超时属预判不足**：未提前想到 Dolt 遥测会在无网络环境阻塞子进程，应先加环境变量或先探测。

**Lessons learned**:
1. 涉及 Dolt 的测试/CI 命令默认加 `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1`；遇到 pytest 无输出推进先查 `dolt send-metrics` 进程。
2. “全局”类配置变更要先 grep 全仓同名常量/局部实现，确认所有调用点都覆盖，不能只改公共常量。
3. 多行 commit message 若被 hook 拒绝，先用 `git commit -F <file>` 并手工跑 hook 验证，避免在 shell 传参上反复试。
4. 子代理返回异常（只返回开场白/未落盘）时立即重新委派并确认产物存在，不把中间状态当完成。

**Process improvements**:
- toolchain.md 新增 2 张排查卡：东财反爬快速失败机制、Dolt 遥测/更新检查导致 pytest 挂起——已随本 PR commit 落地
- evidence/index-fetch-resume-2026-08-15.md 更新限流建议为全局 2.0s——已随本 PR commit 落地
- 建议后续将 `DOLT_DISABLE_TELEMETRY` / `DOLT_DISABLE_UPDATE_CHECK` 固化进项目测试命令/hook/CI 环境变量（proposed，待建 issue）

### Trends (last 10)
- **子代理交付/输出异常系列再次出现**（#244 零交付 → #245 只分析未落盘 → #255 截断零落盘 → #235/#265 自验失真 → 本次对抗子代理首轮未落盘）：委派后核验产物（git status/文件存在）仍未固化为 skill/hook，主 agent 每次手动补救——建议正式写入 skwy-workflow 委派协议（连续多批未落实）
- **“全局”/跨模块配置只改公共点漏局部实现**（本次 #277）：改公共常量前应 grep 全仓同名定义，避免 review 才抓出——与 #264 引用范围侦察不完整同族
- **测试环境隐式网络依赖导致挂起/失败**（#255 真实采集网络不可达 → 本次 Dolt 遥测挂起）：涉及外部服务/遥测的命令应先探测或显式禁用，避免盲等

## 2026-08-15 — ref #278 官方指数接入腾讯源（东财封禁替代）

**What was done**: 在 `fetch_index_daily.py` 为官方指数 30 个新增腾讯 `fqkline/get` 回退：东财失败/empty 自动切腾讯，end-date 反向分页（count=2000）拉全历史，amount 填 0，并接入 #277 快速失败计数；新增 32 个 RED→GREEN 测试（requirement 9 + adversarial 23 参数化），全套件 583+ passed，coverage ≥95%。

**User corrections** (if any): 无显式纠正；用户指令为“#278 通知（用户已确认）…继续实施”。

**What went wrong**:
1. 需求测试子代理第一次又只返回设计过程未落盘，重新委派才创建文件——与 #277 同类子代理交付异常再次出现。
2. 测试 stub 的 URL 匹配顺序错误：腾讯 URL 含子串 `kline/get`，被东财分支提前截获，导致多个 fallback 测试假失败；修正为先匹配 `ifzq.gtimg.cn`。
3. 初始分页实现按 start_date 推进，live API 实测证明腾讯返回的是“窗口内最新 count 条”，必须用 **end 日期反向推进**；已改为 `,,<end>,<count>,qfq` 并加 trade_date 去重，实测 sh000001 8531 根（1990-12-19～2026-08-14）无重复升序。
4. QA 发现 `data` 为真值非 dict 时 `_fetch_tencent_kline` 会 AttributeError 崩溃；已加类型防御并补测。

**Lessons learned**:
1. 外部 API 分页/参数语义必须先用 live 请求实证再定实现与测试（本次 start→end 修正就是 live probe 驱动的）。
2. stub URL 匹配用子串时要注意端点间包含关系（`fqkline/get` 包含 `kline/get`），先匹配更特定域名。
3. 畸形响应防御要覆盖“真值非 dict”形态，不能只测缺 key/None。
4. 子代理交付异常仍反复出现，主 agent 每次都要核验文件存在并准备重委派。

**Process improvements**:
- data-providers.md / architecture.md 已补充腾讯回退数据源说明——已随本 PR commit 落地
- 腾讯分页 live 验证结果（8531 根/无重复/升序）记录在本反思；后续可沉淀到 `.dsh/evidence/`（proposed）

### Trends (last 10)
- **子代理交付/输出异常系列第 N 次**（#244/#245/#255/#235/#265/#273/#277 → 本次 #278 需求子代理首轮未落盘）：委派后核验产物仍未固化为 skill/hook，建议正式写入 skwy-workflow 委派协议
- **外部 API 语义假设需 live 实证**（#255 真实采集网络不可达 → 本次腾讯分页方向 start/end 修正）：先探测再实现，避免测试桩与真实行为脱节
- **URL 子串匹配/引用范围侦察类摩擦**（#264 引用范围漏目录 → 本次 stub 子串误匹配）：匹配条件应使用更精确的标识（完整域名/URL），避免包含关系误伤
## 2026-08-15 — ref #281 官方指数经腾讯源全量入库 + 过程问题排查卡

**What was done**: 东财 push2his 封禁未解期间，用一次性脚本经腾讯源拉全 30 个官方指数（145,215 行全历史，上证自 1990-12-19）入 Dolt（commit chmn88d）+ index_basic 补 30 official 条目 + name_en 列 ALTER 迁移；三个过程问题沉淀 toolchain 排查卡（随 e1365ce，ref #281）。

**User corrections**: ①"反爬被封大概是爬取了多久后发生的事情"→ 分析确认封禁触发点 = 45 请求/2 分钟内（前 45 板块成功、第 46 个起全败），非指纹问题；②"修改爬取的程序，一旦爬取失败，立即结束"→ 快速失败需求 #277；③"或者试一下其他网站？例如腾讯的？"→ 腾讯源调研 #278；④"export duckdb的流程先不用管"→ 我对 export duckdb 未镜像 index 表深挖多轮（job_output ×6 + 多次重跑），用户叫停——GUI 有 parquet fallback，该问题非阻塞。

**What went wrong**:
1. export duckdb 未镜像问题过度深挖：多轮重跑 export（bash-97/98/99/101/102）、RUST_LOG=debug 排查，直到用户"先不用管"才停——应更早识别非阻塞（GUI 大盘 tab 走 ParquetReader 直读，不依赖 DuckDB）
2. 快速失败机制设计盲区：run() 板块段先跑（无 fallback）打满 5 连败 abort，官方段（有腾讯 fallback）轮不到——fallback 形同虚设，只能靠一次性脚本绕过
3. name_en 列缺失：Dolt 旧表无此列，CREATE TABLE IF NOT EXISTS 对已有表不生效 → ALTER TABLE 补齐
4. 一次性脚本只写 index_daily.csv，官方 basic 条目需手动合并进 index_basic.csv

**Lessons learned**:
1. 快速失败计数器应**段间重置**或 fallback 段前置——无 fallback 段在前会误伤有 fallback 的段（已入排查卡，建议后续修 run()）
2. 用户叫停前应主动识别非阻塞项：GUI 有 parquet fallback 时，DuckDB 镜像缺失不阻塞功能——深挖前先判断阻塞性
3. Dolt DDL 列增变更需显式 ALTER TABLE 迁移，CREATE IF NOT EXISTS 不覆盖已有表

**Process improvements**: toolchain.md 排查卡（快速失败误伤 fallback 段 + name_en 列迁移，ref #281）；官方指数数据已入库推送（Dolt chmn88d）

### Trends (last 10)
- 数据级问题反复由真实数据首次暴露（#181 混源、#273 update_date、本次 name_en 列/快速失败误伤）：fixture 覆盖不到数据链路的真实集成，真实冒烟/真实爬取仍是唯一暴露途径（#273、#277 反思同模式）
- 非阻塞问题过度深挖被用户叫停（本次 export duckdb）——与"真实冒烟滞后"同属范围判断：工作前先标定阻塞性

## 2026-08-16 — ref #283 板块数据源战略调整：THS 行业 90 + 概念全链路移除 + 真实采集暴露 CSV 复活事故

**What was done**: 完成 issue #283 全链路（8 实现 commit + 2 修复 commit）：THS 90 申万一级行业替代东财 496（列表实时抓 + 按年分页 K 线）；concept 全链路移除（表/模型/reader/GUI/SEPA）；SEPA 题材改按 stock_basic.industry 聚合；final_score 删 theme_score；BK 4-6 位符号；清理脚本执行（Dolt commit pvah87l0）；真实采集 512,995 行入 Dolt（aq7b7sk）。

**User corrections**: 本 worktree 会话无用户纠正（auto 模式）。D3 决策"先保留后定稿删除东财 BK 行"两次确认记录于 handoff（主会话）。

**What went wrong**:
1. **CSV 复活事故（数据一致性）**：B1 清理只删 Dolt 行、未同步清理 csv/index_basic.csv 镜像；`_persist_outputs` 的增量门禁（`not last` 时不重建 basic CSV）让旧 CSV 残留；import（INSERT IGNORE merge）把 1,000 个已删行（496 东财 BK + 504 concept）全部复活。import 后查 index_type 分布才发现。
2. **Dolt 批量导入性能盲区**：INSERT IGNORE 512,995 行 41 分钟未完成（600s timeout 先超时，3600s 也悬），换 dolt table import 批量路径 18 秒完成——差 2 个数量级。
3. 工具摩擦：多次 edit "file changed since it was read" 重试（改前未 read）；bash-137 会话重启后 job 丢失需重新核验；dolt log --stat 参数语法与 AS OF 'HEAD' 行为与预期不符（改用具体 hash 验证）。

**Lessons learned**:
1. 数据清理（Dolt DELETE/drop 表）必须同步清理/重建派生产物（CSV 镜像、Parquet），否则下次导入用 INSERT IGNORE merge 复活已删行——CSV 与 Dolt 的一致性靠"每次 run 全量重建镜像"保证（merge 永不丢行，重建才安全）。
2. Dolt 大表（>10 万行）导入用 dolt table import 批量路径（CSV→临时表→原子 RENAME 交换），避免 SQL INSERT IGNORE（慢 2 个数量级）。
3. import 后应自动断言表状态（index_type 分布/行数），当场拦截"复活"类数据事故，而非事后人工查。

**Process improvements**:
- 已落实（代码+回归测试，d7f10f1）：_persist_outputs 每次非短路 run 重建 basic CSV；增量 abort 测试契约反转
- 已落实（脚本，3464736）：cleanup-concept-data.sh 同步删除 csv/index_basic.csv 镜像
- 已落实（代码，3464736）：import 插入 timeout 600→3600
- proposed：Dolt 大表导入性能（merge 模式 INSERT IGNORE → dolt table import 路径）与 import 后自动断言——待建 issue 排期

### Trends (last 10)
- 数据链路问题仍反复由真实运行首次暴露（#273 update_date、#281 name_en/快速失败误伤、本次 CSV 复活/INSERT 超时）："真实运行验证"应进入数据管线变更的验收定义，import 后自动断言（行数/类型分布）可当场拦截复活类事故
- Dolt 批量操作性能需预先标定（本次 INSERT IGNORE 41min→table import 18s）：大表导入路径应在实现前选定，避免真实运行才发现

## 2026-08-16 — ref #286 腾讯回退官方指数补齐成交额（newfqkline）

**What was done**: 将官方指数腾讯回退从 `fqkline/get` 切换为 `newfqkline/get`，解析 day 行 index 8 成交额（万元→元）并写入 `index_daily`；更新需求/对抗测试 RED→GREEN；真实重抓 30 个官方指数并替换 Dolt/Parquet（official 160,254 行全部非 0）。

**User corrections**: 用户明确“只需要成交额，换手率用途不大。开始改。”——范围收窄，不引入换手率字段。

**What went wrong**:
1. 完整 collectors pytest 第一次前台运行 180s 超时无输出；改用 `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1` 后台运行后才拿到 614/620 passed。
2. 多次重复 `list_agents` 轮询子代理状态，被系统提示“重复调用未推进”；应等待结算通知而非反复轮询。
3. 初始实现把 `"0"` 万元转成 `"0.0"`，未通过测试的精确字符串断言；改为整数格式化后修复。
4. 安全复审发现 `1e308` 万元在 ×10000 后溢出为 `inf`，初始只在乘前判 `isfinite` 不够；补乘后判有限 + 负值降级。
5. 数据修正是对已有 Dolt 行改 amount，`INSERT IGNORE` merge 不会更新旧行；改用 `merge=False` 全表替换（合并 industry + 新 official CSV）才落地。
6. 分支基于本地 master 多出的一个未推送 commit（f781000），push 前 rebase `--onto origin/master` 排除后才得到干净 3-commit 分支。

**Lessons learned**:
1. 解析外部 API 数值并做单位换算时，必须在换算后再次校验有限性/范围（乘后溢出、负值），不能只校验输入。
2. 完整 Python 测试套件应一开始就用后台运行 + `DOLT_DISABLE_TELEMETRY=1 DOLT_DISABLE_UPDATE_CHECK=1`，避免前台超时和遥测挂起。
3. 修正已存在 Dolt 行的数据时，merge/INSERT IGNORE 不更新旧行；需要 replace 或 delete+insert 语义，并在方案阶段确认。
4. 推送前检查分支相对 origin/master 的基底，本地 master 独有 commit 用 `rebase --onto` 排除，避免无关 commit 混入 PR。

**Process improvements**:
- toolchain.md 补一条：完整 collectors 套件约 3 分钟，优先后台运行并带 Dolt 遥测禁用环境变量。
- 其余为一次性教训，暂不新建 issue。

## 2026-08-16 — ref #287 proxy_pool 试用：独立验证 harness + 免费代理 HTTPS 验证失败

**What was done**: 新增 `scripts/proxy_pool/docker-compose.yml`（proxy_pool 2.4.2 + Redis，回环绑定 + healthcheck + 镜像缺 bash workaround）与 `collectors/check_proxy_pool.py` 独立验证脚本（curl_cffi/chrome142，30 次 THS 探测，输出成功率/平均耗时/失败原因/判定）。RED→GREEN 测试 63 个，全套件 683 passed、覆盖率 98.25%。真实验证结果：30/30 失败（0%），免费代理全部 HTTP-only，HTTPS CONNECT 被拒，按锁定标准判定 FAIL。

**User corrections**: 无显式纠正；用户批准计划与 push/PR。

**What went wrong**:
1. 多次 edit 工具摩擦：2 次 "read the file first"、1 次 "file changed since it was read"，均因改前未 read/文件被 ruff format 改动；增加返工。
2. 测试子代理交付的 requirement 测试存在 Python 闭包 bug（`status_code = status_code` 在 class body 中 NameError）与契约不一致（`run_trial` 缺 count、`main()` 在 pytest 下被 sys.argv 干扰），实现后首次运行才暴露，需主 agent 修测试。
3. `get_proxies` 初版按假设的 `{"proxies": [...]}` 解析，真实 proxy_pool `/all/` 返回 JSON 数组对象且 `/all` 会 302；真实运行后才发现，补了 list 形态兼容与 `/all/` 尾斜杠。
4. Review 发现 `main` 文档/测试声称「API 不可达 → rc=1」，但 `get_proxies` 吞掉所有异常返回空列表，真实路径不可达；需统一契约（不可达 → rc=0 + FAIL，rc=1 仅真实 validation 错误）。
5. Docker 沙箱 bridge 网络创建 veth 失败（operation not supported），改用临时 host-network override 完成验证；官方 proxy_pool 镜像缺 bash 默认 ENTRYPOINT 崩溃，compose 用 `sh` 直启 server/schedule 绕过。
6. 本地 master 存在未推送 commit f781000 混入 worktree 分支，push 前用 `git rebase --onto origin/master f781000` 排除，得到干净 4-commit PR 分支（与 #286 同型问题）。

**Lessons learned**:
1. 委派测试子代理后，主 agent 应在实现后第一时间跑新测试并审查测试代码本身（Python class body 闭包/签名/argv 等 latent bug），不能只信子代理自报 RED/GREEN。
2. 对接外部 API 前先 curl 真实响应确认形态与重定向，再定解析契约；不要按文档/假设写死响应结构。
3. 对外暴露的退出码/错误语义必须有真实可达路径；仅靠 monkeypatch 让测试通过等于虚假契约，review 应专门检查「文档承诺的路径是否真能发生」。
4. 沙箱 Docker bridge 不可用时用 host-network override 做验证，交付 compose 保持标准 bridge；环境 workaround 写入 toolchain 排查卡。
5. worktree 分支 push 前检查 `origin/master..HEAD`，若含本地 master 独有 commit 用 `rebase --onto` 排除，避免无关 commit 进 PR。

**Process improvements**:
- 已落实：`.dsh/kb/dev/toolchain.md` 新增两条容器排查卡（bridge veth 不支持、proxy_pool 镜像缺 bash）。
- proposed：skwy-requirement-test 委派 prompt 增加「用临时参考实现自检测试可运行」要求（本次 requirement 子代理未自检，adversarial 子代理自检通过）；待建 issue 排期。

### Trends (last 10)
- 本地 master 独有未推送 commit 混入 worktree 分支已第二次出现（#286、#287）：push 前应默认执行 `git fetch origin master && git log HEAD..origin/master`，发现本地独有 commit 用 `rebase --onto origin/master <本地基点>` 排除。
- 外部 API 真实形态与文档/假设不符多次由真实运行暴露（#283 JSONP/列序、#287 `/all/` list-of-dicts）：新数据源/API 对接应先抓真实样例再写解析。
- 子代理产出测试存在 latent bug 需主 agent 复核（本次 requirement 测试闭包/契约问题）：测试子代理交付前应自检可运行性。

## 2026-08-17 — ref #290 proxy_pool HTTPS 校验补丁 + freeproxy 集成

**What was done**: 修正 proxy_pool `httpsTimeOutValidator` 的 https 代理 scheme（`https://` → `http://`），用多阶段 Dockerfile 打补丁镜像并让 compose 走 `build: .`；新增 `collectors/fetch_freeproxy.py` 把 freeproxy（`proxies.json` 快照 + `pyfreeproxy` 实时）灌入 proxy_pool Redis；补测试、文档、证据；真实验证 freeproxy 代理源下 THS 成功率 60%。

**User corrections**:
- “pyfreeproxy 还是必须的安装依赖拉。” —— 用户否决“可选依赖”方案，要求 pyfreeproxy 作为正式依赖。
- “再考虑更架构一点。” / “我们是不是需要一个更好的爬虫工具库？？” —— 要求先做架构选型（采集层/池管理层/桥接层），并评估爬虫库。
- “运维流程上呢？” —— 要求补充运维 Runbook（启动/刷新/监控/异常/安全/回滚）。
- “之后合并pr并关闭worktree” —— 最终明确 push→PR→merge→关闭 worktree。

**What went wrong**:
1. 真实免费代理池初始 0% 成功率，容易误判为补丁无效；实际是免费代理不支持 CONNECT。需用受控 CONNECT 代理证明机制，再用 freeproxy 广撒网解决数量。
2. 沙箱有 Clash 代理（`127.0.0.1:7897`），本地直连型 CONNECT 代理超时；必须把本地代理链到上游 Clash 才能访问外网。
3. 上游 `jhao104/proxy_pool:2.4.2` 缺 `patch`，Docker build 失败；加 `apk add patch` 并改多阶段构建，最终镜像不携带 patch/补丁文件。
4. Review 发现 realtime 模式两处功能 bug：`ProxyInfo` 字段是 `country_code` 不是 `country`；`.proxy` 带 `http://` scheme，直接写入会导致 proxy_pool 拼出 `http://http://ip:port`。因 realtime 路径最初无测试而漏网。
5. 子代理/初始测试存在多处 latent bug：tautological 断言、正则 `+++` 未转义、函数名 N802、过时 “RED now” docstring、`mod.curl_requests` 导出问题；均需主 agent 修复。
6. 沙箱默认 `HTTPS_URL=https://www.qq.com` HEAD 返回 501，会误伤可用代理；验证时需改用 `https://example.com` 等 HEAD 200 目标。

**Lessons learned**:
1. 集成第三方代理源时，先验证其真实数据结构（属性名、是否带 scheme），再写 normalizer；realtime 路径必须有真实单元测试。
2. 写 Redis 前对不可信代理字符串做公网 IP/端口/控制字符校验，避免脏数据进入 proxy_pool。
3. 沙箱网络有上游代理时，本地代理验证要链到上游 Clash；环境相关 workaround 记入 toolchain。
4. 免费代理池的“可用性”必须区分“代理本身是否支持 CONNECT”和“目标站点是否放行”；用受控代理证明机制，用大规模源（freeproxy）解决数量。
5. 新功能从第一版就应包含运维 Runbook（启动/刷新/监控/异常/安全），不能等用户追问再补。

**Process improvements**:
- 已落实：`collectors/fetch_freeproxy.py` + 安全校验 + realtime 测试；`process.md` 增加 freeproxy 集成与运维注意；`toolchain.md` 增加缺 patch/多阶段构建排查卡。
- proposed：给 CI 增加依赖审计（`uv audit`/osv-scanner）以覆盖 pyfreeproxy 引入的传递依赖；待建 issue 排期。

## 2026-08-18 — ref #292 index_daily 真增量同步

**What was done**: 将 `collectors/fetch_index_daily.py` 从“全量拉取 + INSERT IGNORE”改为按 symbol `MAX(trade_date)` 的真增量：THS 行业只拉 MAX 年份→今年并过滤旧行、官方指数东财 `beg=MAX+1`、腾讯增量翻页遇边界停止；新 symbol 全量回填，周末/停牌空增量按成功 no-op。补需求/对抗测试、data-providers.md 决策记录与真实冒烟证据。

**User corrections**:
- “不是增量更新吗？？？？？” —— 用户指出当前 sync 实际是全量回拉，不是增量；这是本次 issue #292 的起点。
- “是，push 并创建 PR” —— 最终确认 push 并创建 PR（流程决策，非纠偏）。

**What went wrong**:
1. **腾讯分页行序假设错误**：初版按“页内 newest-first”实现增量边界，但真实 `newfqkline/get` day 行是 ascending（oldest first），导致 last_date 前有旧行时直接 break、丢掉同一页后面的新行；真实冒烟发现后修复并补升序页测试。
2. **增量 no-op 语义三度返工**：第一轮 review 发现 THS 空年 break 会漏 MAX 年数据；第二轮又发现“最新年失败被旧年有效响应掩盖”和“畸形 JSONP/全畸形 Tencent 行被当作有效空增量”；第三轮发现“部分年份失败但其他年有新行”会推进 MAX 造成永久空洞。每轮都补了回归测试后才收敛。
3. **手动冒烟脚本污染真实 CSV 目录**：写复现脚本时未设 `COMPASS_CSV_DIR`，直接覆盖了 `/data/compass-data/csv/index_basic.csv` 并写入 `index_daily.progress.json`；review agent 发现后从 Dolt 恢复/删除。教训：任何采集器冒烟必须先设临时 CSV/数据目录。
4. **全量 `pytest tests/` 反复超时/拖慢 review**：本地工具 60s 上限跑不完全量，review agent 又各自跑全量，导致多轮长时间等待；应只跑相关测试文件并在委派时明确指定命令。
5. **commit-msg 偶发 `gh issue list` 失败**：两次 commit 被 hook 拒绝，手动验证 `gh issue list` 正常后重试成功；属于环境瞬时故障，非代码问题。

**Lessons learned**:
1. 对接外部 API 分页前，先用真实响应确认行序/字段顺序，再写边界逻辑（本次 Tencent ascending 是决定性事实）。
2. 采集器冒烟/复现脚本必须显式 `COMPASS_CSV_DIR` 和 `COMPASS_DATA_DIR` 指向临时目录，禁止触碰真实数据文件。
3. 增量 no-op 必须区分“合法空响应”与“畸形/失败响应”：`[]` 只能来自确认无新数据，任何结构异常应返回 `None` 并计入失败，避免绕过 fast-fail。
4. 增量窗口内“部分年份失败”不能写入部分行后推进 MAX，否则失败年份的缺失数据永远不会再被拉取；应丢弃部分行并让下次重试整个窗口。

**Process improvements**:
- 已随本 PR 固化回归测试：THS 空年 continue、部分失败丢弃、Tencent 升序页、畸形 payload 返回 None、官方空增量 no-op。
- proposed：在 `.dsh/kb/dev/testing.md` 或 `toolchain.md` 增加“采集器冒烟必须隔离 COMPASS_CSV_DIR/COMPASS_DATA_DIR”的强制检查项，并考虑给 review 委派模板固定“只跑相关测试文件”命令，避免全量超时。

## 2026-08-19 — ref #294 collectors 接入 proxy_pool 代理层 + keepalive

**What was done**: 为全部 Python collectors 接入 proxy_pool 代理层（proxy-first、池空降级、坏代理轮换）并新增 keepalive 双源喂源脚本；RED 测试、文档、evidence 同批提交（7b041f9）。

**User corrections** (if any): 无显式纠正。用户两次批准 fallback（RED 测试与 review 均因 DSH 子代理基础设施不可用改为主 agent 自写/自审）；用户询问 trash-put 原因（环境 rm 安全包装，非流程纠正）。

**What went wrong**:
- DSH 子代理工具整体不可用（subagent run failed），两处门禁（RED 测试、review）被迫走用户批准的 fallback，失去认知独立性（已记录 toolchain.md）。
- `str_replace_editor` 在本环境多次“成功”但吞掉替换内容：fetch_freeproxy 函数整体消失、proxy_pool_client.get_proxy 方法体变空、fetch_bse 测试体被清空——均通过 `write` 全量重写或 `edit` 修复；造成大量返工（edit ×69 / read ×41）。
- 多次 `edit requires reading ... first` 报错：本环境 edit/str_replace_editor 强制先 read，未先 read 直接编辑会失败。
- `skill` 工具首次调用漏传 `name` 参数（ToolArgsError）。
- 覆盖率首轮 94.01% 未达标，补 25 个覆盖测试后 96.60%。
- keepalive 冒烟首轮因 realtime 源过慢超时；改用 `--realtime-sources ""` 验证 json 路径。
- fetch_main_flow 冒烟在沙箱 push2 直连超时（exit 124），仅验证了降级路径；真实代理成功路径需生产 VPS 验证。

**Lessons learned**:
1. 本环境 `str_replace_editor` 不可信：大段替换优先用 `write` 全量重写或 `edit`（精确唯一 old_string）；每次编辑后立即 grep 验证函数体存在。
2. 编辑工具要求先 read：新文件/未读文件先用 `read` 标记再 edit，避免报错返工。
3. 子代理不可用时不要反复重试：先最小任务确认系统性故障，再向用户申请 fallback 并记录 toolchain。
4. 覆盖率门禁应在提交前跑完整 `--cov-fail-under=95`，新增代码即时补覆盖，避免提交后返工。

**Process improvements**:
- 已新增 `.dsh/kb/dev/toolchain.md` 问题卡：DSH 子代理工具不可用（实现 commit 已含）。
- 新增 `.dsh/kb/dev/toolchain.md` 问题卡：`str_replace_editor` 内容吞噬（本反思 commit 一并提交）。
- 其余为一次性教训，未固化为机制（None）。

### Trends (last 10)
- 多次真实冒烟暴露单测盲区（#283 CSV 复活、#286 成交额、#292 增量、#294 沙箱 push2 超时）→ 保持“真实数据冒烟 + evidence 落盘”强制步骤，并在 evidence 中显式记录环境限制。
- proxy_pool 系列（#287/#290/#294）反复受“沙箱无 proxy_pool/Redis”限制 → evidence 模板应固定“生产 VPS 最终验证清单”，避免每次重新摸索。
- 采集器网络/反爬主题高频出现（#277/#278/#283/#286/#287/#290/#292/#294）→ 该领域值得沉淀为 skill/checklist（如“反爬/网络故障排查卡”）。

## 2026-08-19 — ref #296 expose proxy_redis host port in compose for keepalive

**What was done**: 修复 compose 版 `proxy_redis` 未向宿主机暴露 6379 导致 keepalive / fetch_freeproxy 默认 redis URL 连不上（Error 111）；新增回归测试与 docs（6ab22e1、38fc95f）。

**User corrections**:
- “所有子代理审查工具连续失败 这是为什么” —— 用户要求追查子代理审查失败根因，不接受仅 fallback。
- “查一下为什么会失败，修复它” —— 要求修复根因而非绕行。
- “这个session之前是使用opencode go，我把session的模型换了，但是子模型没有自动切换到新的，这个bug吧。” —— 用户准确指出 DSH bug：子代理继承旧启动模型，session 中途换模型不生效。

**What went wrong**:
- 子代理审查工具（前台+后台）全失败，最初按已知 toolchain 卡直接走人工复核 fallback；用户坚持追根因后才发现真因是 OpenCode Go 周配额 + 子代理继承旧模型（并非“基础设施不可知”）。
- `pkill -f 'proxy_keepalive.py --interval 300'` 匹配到调用 shell 自身，把当前 bash 杀掉（SIGTERM），导致 keepalive 重启命令未执行；教训：清理后台进程用精确 PID，避免 broad pkill 匹配自身命令行。
- write/edit 工具强制先 read，多次因未先 read 报错返工。
- compose 端口暴露缺陷是切换 host-network→bridge 后才暴露的部署缝隙，首次 compose 启动即遇到。

**Lessons learned**:
1. 子代理失败先解压孩子会话日志看 `turn/end` 的 `reason.error`——能直接定位 provider 配额/模型错误，而不是停在“基础设施故障”卡片。
2. 子代理应跟随父 session 当前模型，而非创建时 `AgentOptions`（已修 deepseek-harness fbd193a）。
3. 杀后台进程用精确 PID（`pgrep -f` 后取 PID 再 kill），不要把 `pkill -f` 模式写进会同时匹配自身 shell 的命令里。
4. 部署形态变更（host-network→compose）后必须重新验证“宿主机可达性”类假设（端口映射）。

**Process improvements**:
- toolchain.md「DSH 子代理工具整体不可用」卡片追加 2026-08-19 复发/根因（OpenCode Go 配额 + 旧模型继承 + 修复 commit fbd193a）。
- 回归测试 `collectors/tests/test_proxy_pool_compose.py` 固化 compose 端口与 keepalive 默认 URL 绑定（ref #296）。

### Trends (last 10)
- 子代理交付/基础设施异常再次出现（#244/#245/#255/#278/#294 → 本次 #296 追到真实根因）→ 反思已从“记录故障”升级为“读孩子日志定位真因”，可固化为子代理故障排查脚本。
- 采集器网络/反爬主题继续高频（#287/#290/#292/#294/#296）→ 建议沉淀反爬/网络故障排查 checklist。
- “部署形态变更后验证可达性”是新出现的模式，值得在 process.md 部署章节加检查项。

## 2026-08-20 — ref #299 财务三表 UPDATE_DATE 增量抓取 + merge/ODKU

**What was done**: 实现 issue #299：balance_sheet/income/cash_flow 三表从 REPORT_DATE 报告期窗口改为 UPDATE_DATE 时间锚点增量抓取，导入改 merge=True + INSERT ... ON DUPLICATE KEY UPDATE，无 anchor 时固定 2020-01-01 走全历史 UPDATE_DATE；共享增量 helper 移入 common.py，main.py sync/fetch 接 --incremental。

**User corrections**: 无明确纠正；用户批准 plan、批准 push+PR。

**What went wrong**:
- 三次委派 `subagent_skwy_adversarial_test`（含一次 resume）均因 token/context 上限在写文件前中断，最终由主 agent 补写对抗性测试并记录在 `.dsh/evidence/f10-update-date-incremental-red-adversarial-tests.md`。
- 初版实现把约 90 行增量 fetch/state 块在三个 F10 模块中逐字复制，review P1 指出重复与 mypy strict 错误；随后重构为 `common.fetch_incremental`。
- 重构后测试长时间挂起：helper 直接使用 `common.AsyncSession`/`update_date_anchor`/`fetch_by_update_date`，而测试 monkeypatch 的是模块级同名属性；通过 `session_factory`/`anchor_resolver`/`fetch_fn` 注入修复。
- `fetch_fin_indicators.py` 本地 `Throttle` 与 `common.Throttle` 类型不匹配导致 mypy arg-type 错误，改用 `common.Throttle` 解决。

**Lessons learned**:
1. 把模块逻辑抽到共享 helper 时，必须保留测试注入点：把模块级 `AsyncSession`/`update_date_anchor`/`fetch_by_update_date` 作为参数传入，而不是在 helper 内硬编码 common 全局——否则既有 monkeypatch 测试会打真实网络或挂起。
2. 大段逻辑在多个模块间复制前先考虑共享函数；提交前跑 `mypy`（即使 CI 不强制）能提前暴露 `str|int|float` 与 Throttle 类型问题。
3. 子代理反复因 token 上限中断时，记录委派失败并主 agent 补写测试（附 evidence）比无限重试更高效。

**Process improvements**: None（一次性教训；对抗性测试 fallback 已记录在 evidence 文件）。

## 2026-08-22 — ref #301 docs: 移除截图修 bug 禁令，适配多模态图像输入

**What was done**: 删除 AGENTS.md 中「禁止依赖视觉表现来 debug」硬禁令，改为允许截图/多模态视觉检查辅助 UI 调试；同步更新 `.dsh/kb/dev/testing.md` 三处旧口径。创建 issue #301，commit `4700d56`。

**User corrections** (if any): 无。用户最初要求修改 AGENTS.md/项目书，确认推荐方案（同步 testing.md）后按推荐执行。

**What went wrong**: ①使用 `edit` 工具前未先通过 `read` 工具读取文件，4 次编辑调用被拒绝（工具要求先 read）；改用 `read` 后成功。②`reflect-audit.sh` 默认 `find -maxdepth 2` 找不到当前 session trace（实际嵌套在 workspace slug 目录下第 3 层），改为手动 `zstd -dc` 读取。③`gh issue list --search "..." in:title,body` 参数语法错误，换用 `gh issue list --search` 成功（小摩擦）。

**Lessons learned**:
1. 对 `edit` 工具编辑任何文件前，先通过 `read` 工具读取该文件，避免工具拒绝往返。
2. 运行 `reflect-audit.sh` 失败时，先定位实际 trace 路径（`find ... -maxdepth 3 -name 'session-*'`），再手动解压读取，不跳过第 0 步。
3. 使用 `gh issue search` 语法前参考 `gh issue list --help`；简单搜索用 `--search` 单参数即可。

**Process improvements**: 本次已完成 AGENTS.md + `.dsh/kb/dev/testing.md` 文档同步（commit `4700d56`）；无新增 hook/脚本/自动化机制。

### Trends (last 10)
- No significant patterns observed.

## 2026-08-25 — ref #298 import-compass merge key mismatch + fallback history loss

**What was done**: 修复 `import-compass` append 表增量 merge 丢行：`block_trade.partition_cols` 扩为生产 Dolt 全主键，`import_append_table`/`import_fin_indicators` fallback 改为不带 `--since` 的真全量导出并保留旧 parquet 备份；新增全部 append/import-compass 表生产 PK 防漂移测试、block_trade RED→GREEN 测试与 fallback 历史测试；同步 data-providers/toolchain/testing/cli/architecture 文档与 real smoke evidence。

**User corrections**: 无显式纠正。初始消息为 worktree 启动指令；末尾用户仅确认允许 push（流程批准，非纠偏）。

**What went wrong**:
1. 修复共享 `import_append_table` fallback 后，`fin_indicators_merge_failure_falls_back_to_full_export` 仍失败，才暴露 `import_fin_indicators` 自带一份相同的 merge/fallback 副本——实现前没有先 grep 所有 `falling back to full export` 拷贝，导致第一轮修复漏掉一个路径。
2. `edit` 工具多次报错（file changed since it was read / requires reading first / old_string not found / matched 2 times），与 #286/#287/#294/#299/#301 同型摩擦。
3. 子代理完成前反复 list_agents 轮询（本 session 31 次），应等结算通知而非主动轮询。
4. 首轮 `cargo check --tests` / `cargo test --lib` 全绿，但 review 指出集成测试 fixture `data_quality_adversarial.rs` 的 FIN_SCHEMA 未同步生产 DDL；push 前补跑完整 `cargo test -p compass-data` 才覆盖到。

**Lessons learned**:
1. 修复“重复实现”型 bug 前，先 grep 全部同模式拷贝（如 `falling back to full export` / `std::fs::write(&path, &new_data)`），确保所有路径一次修完，不能依赖单个单元测试暴露遗漏。
2. 对本项目大 Rust 文件编辑前先 read；如果 edit 报 stale，重新 read 再改，避免往返。
3. push 前应跑 `cargo test -p <crate>`（含 integration tests），不只跑 `--lib`；集成 fixture 漂移只能由全 target 测试暴露。
4. 子代理后台任务等待结算通知，不反复 list_agents 轮询。

**Process improvements**: toolchain.md #298 卡已记录 duplicate fallback 副本与“grep 所有拷贝”教训；其余为一次性执行摩擦，无新增机制。

## 2026-08-25 — ref #303 sepa_daily.sh 每日流程纳入 index_daily/index_basic

**What was done**: 将 `index_daily` 及伴生 `index_basic` 纳入 `scripts/sepa_daily.sh` 每日流程：step2 fetch+import、`COLLECTOR_TABLES` allowlist、step4 per-table 增量锚点 + `index_basic` 全量覆盖；`dolt sql` 锚点查询失败改为 loud abort；并修复 `import_append_table` 首导出忽略 `--since`，补 shell/Rust 回归测试与文档同步（3 commits：5ecdf8e/276a70d/8a27017）。

**User corrections** (if any):
- 用户通过澄清问题选择：“纳入 COLLECTOR_TABLES + step4 导入” （`index_basic` 范围）。
- 用户通过澄清问题选择：“两项都加固”（per-table 锚点 + dolt sql 失败 loud abort）。
- 用户早期选择“只做本 PR，等 #298 PR 先合并”，随后告知“issue #298 已关闭，合并到主干了，可以拉取一下”，并批准 push。
- 上下文来自父会话：用户观察到“指数的数据没有更新？”，并确认“每日的数据收集也是需要的”“按推荐”。

**What went wrong**:
1. 初版 commit `1c6a033` 漏掉 `collectors/main.py import index_daily` 的伴生副作用 `index_basic`，且 step4 用全局 MAX 锚点会在“新表有 Dolt anchor 但 Parquet 不存在”时造成首导出截断；review P1 发现后追加 2 个 fix commit。
2. `edit` 工具多次报 “requires reading file first”（database.md/cli.md/data-providers.md/import_compass.rs），与 #301 同型摩擦。
3. commit-msg hook 首次拒绝：`could not verify issue states (gh issue list failed or returned empty)`；诊断 issue #303 确实 OPEN 后原样重试成功（瞬时环境问题）。
4. `cargo check -p compass-data` 120s 超时被 kill；改用定向 `cargo test -p compass-data import_compass::tests` 完成验证。
5. `git rebase --continue` 因 dumb terminal 报错，用 `GIT_EDITOR=true` 解决；handoff.md 冲突用 `--theirs` 保留本 worktree 版。
6. `reflect-audit.sh` 因嵌套 session 目录（`find -maxdepth 2`）和 `session-` 前缀双重问题找不到 trace；本次已 patch 全局脚本。
7. 子代理完成后仍多次 `list_agents` 轮询，未等结算通知（与 #298 教训重复）。

**Lessons learned**:
1. 新增采集源时必须 grep 其 import 侧的所有伴生写表/副作用（如 `import_to_dolt` 同时写 `index_basic`），否则 Dolt 脏工作区或 Parquet 过期。
2. 流水线增量锚点应 per-table；且“有 Dolt anchor 但无 parquet”首导出必须在 Rust 侧忽略 `--since`，否则历史被截断。
3. review P1 必须在同轮补测试（12b/12c/12d + Rust 首导出测试），不要留下欠账。
4. 编辑仓库文件前先 `read`，避免 edit 工具拒绝对话。
5. commit hook 瞬时失败先 `gh issue view` 诊断，再重试；不静默绕过。
6. reflect-audit 的 session trace 定位已自动化修复；下次遇到 trace 找不到先检查脚本而非手工解压。

**Process improvements**:
- 已 patch `/home/skwy/.dsh/skills/skwy-reflect/resources/reflect-audit.sh`：`find -maxdepth 2 → 3`，并在查找前 normalize `session-` prefix；嵌套 worktree 会话 trace 可被脚本直接定位。
- 已新增 Rust 回归测试 `append_table_first_export_with_since_imports_full_history`，把“首导出不得截断”固化为测试。
- 其余为一次性执行摩擦，无新增 repo hook/流程规则。

### Trends (last 10)
- `reflect-audit.sh` 找不到嵌套 session trace 在 #301 与本次重复出现；本次已通过 maxdepth 3 + prefix normalize 固化。
- `edit` 工具未先 read 的摩擦在 #298/#301 与本次多次出现；尚未固化为自动检查。
- 子代理完成前主动 `list_agents` 轮询在 #298 与本次重复出现；应改为等结算通知，避免无效轮询。

## 2026-08-27 — ref #306 sepa_daily.sh 完整 compass_data 每日刷新 + sync 硬化

**What was done**: 将 `scripts/sepa_daily.sh` 从 6 表 SEPA-only 扩展为 11 表完整 `compass_data` 每日刷新入口（step 2 改用 `collectors/main.py sync`，step 4 覆盖 stock_basic/财务四表/SEPA/指数）；同时强化 `main.py` sync/import 失败即中止、`fetch_stock_basic_official` 空/部分数据拒绝覆盖、`fetch_index_daily` index_basic 失败传播、`_import_stock_basic` 原子替换与恢复；真实数据冒烟已跑并推送 Dolt。

**User corrections**:
- 「运行完，自动完成后面的流程。我去睡觉了」——授权 auto 模式，push/PR 自动推进。

**What went wrong**:
1. 两次 `edit` 因未先 read 文件被拒绝（`.dsh/plans/complete-daily-compass-refresh.md`、`collectors/fetch_index_daily.py`）。
2. 安全硬化在 review 中连续暴露多个 P1：部分 stock_basic 可能清库、sync 内部 import 失败不传播、原子替换边界不完整、测试未同步；最终多轮 review 后才全部关闭。说明这类数据完整性改动应先在实现前把失败路径与测试矩阵设计完整。

**Lessons learned**:
1. 对会影响权威数据表的采集/导入改动，先列出所有失败路径（fetch 空/部分、import 0、备份恢复失败、final count 异常）并写测试再实现。
2. 修改模块行为时，同步 grep 所有调用方测试（`test_csv_output_dir.py`/`test_f10_incremental_requirement.py`/`test_index_main_cli.py`），避免旧断言成为 P1。
3. 自动模式下 push 前仍必须完成 review 无 P0/P1 与 rebase/反思 commit。

**Process improvements**: None（本次未固化新 hook/脚本；建议后续把“修改 import 行为必须同步相关测试”纳入流程，暂未建 issue）。

### Trends (last 10)
- 近 10 条反思多次围绕数据管线安全性（ref #292/#294/#298/#299/#303/#306），集中在“增量/每日流程边界必须 fail-loud、防止历史丢失”主题；建议后续在 collectors 测试中固化“增量空 CSV/部分覆盖”回归套件。
