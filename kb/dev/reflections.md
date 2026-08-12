# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

**归档机制**：教训已融入流程（AGENTS.md 规则、skill 步骤、hook、回归测试、CI 门禁）
的条目不再具活性参考价值，归档至 `kb/dev/reflections-archive.md`（历史可查）。
主文件仅保留仍具活性参考价值的条目（最近 + 教训未完全固化者）。

**自动归档（skwy-reflect 第 5 步，ref #238）**：本文件超过 500 行时自动归档一次
——值得处理的条目建 issue 后归档、已处理的直接归档、剩余的保留待下次归档时
重新检阅；归档后仍超 500 行则交用户判断。



## 2026-08-06 — ref #184/#182 CI hooks：pre-commit fmt 落地 + temp 竞争根治（#189 收尾）

**What was done**: fix/ci-hooks worktree 处理三个 CI issues：#189（GIT_EDITOR=true 排查卡）确认已由 master 直推 `523e615` 完成、补收尾（comment + close）；#184 temp CSV 竞争修复（test-first RED → 唯一后缀 GREEN，后续 review 驱动扩展为共享 `stage_csv` helper + O_EXCL 防 symlink + import 后 `remove_file` 清理，sepa.rs 同类竞争一并修复）；#182 pre-commit 追加 `cargo fmt --check`（#171 反思排期项落地）。5 commits，2 轮 review（10 次审查）全 PASS 无 blocking。

**User corrections**（逐字引用对话记录）:
1. "本 PR 顺带修复 (Recommended)" —— 用户确认 Context review 发现的 sepa.rs 同类竞争纳入本 PR（超出原 plan 范围，用户批准扩展）。
2. "一并清理 + 重构测试" —— 用户未采纳我的"保持现状"推荐，选择同时清理 temp 文件并重构回归测试（接受 Goal/CodeQuality 的 MINOR 发现）。

**What went wrong**:
1. **commit-msg 拒绝：正文引用已关闭 issue 需用 #N 不带 ref 前缀**（ref #119/#172 教训再犯）：`7084d16` 首次 commit 时正文叙述性提及已关闭的 #154 却写了 `ref #154`，commit-msg hook 当场拒绝。AGENTS.md 已有明文规则（"叙述性提及已关闭/合并 issue 时，用 #N 不带 ref 前缀"），执行时未遵守——「文档已固化但未遵守」模式第五次出现（#96 → #104 → #171 → #190 → 本次）。
2. **review 的修复建议存在隐藏冲突**：Goal/CodeQuality agent 建议 `dolt_import` 后 `remove_file` 清理 temp 文件，但未发现该建议会破坏回归测试 `files.len()==2` 断言（数残留文件验证唯一性）——两 agent 均未察觉，我在实施前识别出该冲突并向用户提出。若盲目采纳会引入测试空洞。
3. **security review 的 create_new 建议与回归测试交互**：O_EXCL 修复与 `files.len()==2` 断言兼容，但共享 helper 提取后两处调用（backtest/sepa）语义需一致——通过统一 `stage_csv` + 显式契约文档化解。

**Lessons learned**:
1. **commit message 正文引用已关闭 issue 时一律 `#N` 不带 `ref` 前缀**——commit-msg hook 是硬钩子但只拦截"提交后"，写 message 时就要遵守；引用历史 issue 讲解背景时先确认其状态再决定前缀（#119 正文示例误判、#172 正文引用已合并、本次 #154 三重教训）。
2. **采纳 review agent 建议前先核对与既有测试断言的交互**：remove_file 建议与 files.len()==2 断言的冲突是隐藏依赖，agent 不会自动察觉——实现者必须验证"建议的修复是否破坏现有测试的观测点"，必要时向用户提出而非盲目执行。
3. **共享 helper 提取的时机**：sepa.rs 同类竞争（Context agent 发现）与 backtest.rs 原 bug 完全同根因，一次性提取 `stage_csv` 解决两处——review 发现同类问题时，优先评估"共享修复"而非"逐点补丁"。

**Process improvements**:
- 已落实：#182 pre-commit `cargo fmt --check`（#171 反思排期项，本 session 落地为 hook 硬钩子）；`stage_csv` helper + O_EXCL + remove_file（代码级，含唯一性直接单测）
- proposed（代码类，未排期）：`stage_csv` 的 `f.write_all` 失败时清理已创建文件（ENOSPC 边界）；目录级 symlink 变体（攻击者预建 `compass_sepa_writeback` 为 symlink）加固——单用户开发机可接受，如需要走 gate 建 issue

### Trends (last 10)
- **「文档已固化但未遵守」模式第五次出现**（ref #96 → #104 → #171 fmt → #190 Dolt → 本次 commit-msg ref 前缀）：AGENTS.md 明文规则（叙述性提及已关闭 issue 用 #N）执行时未遵守，被 commit-msg hook 硬拦截——文档规则必须配套执行侧硬钩子（本次 #182 的 fmt hook 正是该模式的正向固化：从文档规则升级为 pre-commit 硬钩子）
- **review 驱动的同类问题发现 → 共享修复**（ref #139 六轮、#171 两轮、本次两轮）：Context agent 发现的 sepa.rs 竞争与 backtest.rs 原 bug 同根因，一次提取共享 helper 解决两处——review 的价值不只是抓 bug，还在于发现"修一个留一个"的同类风险

## 2026-08-08 — ref #181 symbol 前缀规范化全面修复

**What was done**: 修复 issue #181（import 剥 SH/SZ/BJ 前缀导致指数 SH000905 与股票 SZ000905 汇合为裸码 000905，stock_daily.parquet 出现 (symbol, date) 重复行）。方案：恢复 Dolt-native 前缀符号全链路（五 crate），废弃裸码+exchange 分离设计（D9 输入层禁裸码）、D10 旧 config 自动迁移、D11 搜索语义。14 commits（fec8781..1948a0c），含 10 轮 plan 双审（momus+Oracle r1-r10）、F2/F3 验证波、两轮 review-work（5-lane 首轮 FAIL → 修复轮 PASS）。

**User corrections**（逐字引用对话记录）:
1. "不允许输入层便利。" —— 对 D7 的重大修正：输入层（CLI --symbols、GUI 过滤/fetch、config 默认值）也不接受裸码，不是"输入层便利 + 数据层规范化"。直接改变 plan 范围（D9 由此诞生）。
2. "Scope OUT（已确认不做的） 这部分里面有前缀和symbol分离的吗？" —— 追问 scope 边界，促使 Scope OUT 补充"禁止新代码以裸码作为存储/查询/返回值格式"显式条目。
3. "压缩一部分上下文，把plan修改的具体过程压缩掉。" —— 用户主动要求压缩会话以继续工作。
4. "1 ，并minor也一并修复" —— review 报告选项选择：收口 2 个 IMPORTANT 项 + 一并修复全部 MINOR 项（而非只做 IMPORTANT）。
5. "push" —— 明确推送指令（此前所有 review 轮次均未授权 push，符合 HARD BLOCK）。

**What went wrong**:
1. **F1 evidence 早期声称"9 commits"**：plan 完成审计（F1）在修复轮中途写就，声称 9 commits 全部含 ref #181——实际完成时 14 commits。上下文挖掘 lane 抓出"evidence 过期声称"，违反 AGENTS.md ref #174"完成声明前必须核实、禁止过度声称"。教训：F-wave evidence 应在全部实现完成后一次性写，中途写必然过期。
2. **MINOR 修复引入 doc drift（43→BJ 未同步 kb/）**：c79564c 给 `infer_exchange_prefix` 加 43→BJ 分支（对齐官方采集器），但 kb/design/symbols.md L57/L211 + kb/user/config.md D10 迁移规则仍是"8/92→BJ、其余→SZ"——三方 review（Goal/CodeQuality/Context）同时抓出。违反 plan 自身 success criterion "kb/ 文档与代码一致"。教训：行为变更（尤其规则/启发式改动）必须同 commit 同步文档，不能等"文档任务"。
3. **Security lane 抓到 --start-date/--end-date 注入未封**：首轮修复只封了 --symbols 注入，日期参数仍是原始插值——修复不完整导致第二轮 FAIL。教训：安全修复要覆盖同一漏洞类的全部实例（--since 已有校验，start/end 应同构处理），review 通过后修复必须逐条验证而非"修了主要的那条"。

**Lessons learned**:
1. **F-wave evidence 只在全部实现完成后写**——中途写必然"过期声称"；如不得已中途写，完成后必须补正（本次已补正为 11→14 commits 但应避免再犯）。"声明完成前逐条核实"是 ref #174 的强制要求，evidence 产物本身也必须真实。
2. **规则/启发式改动 = 文档同 commit 同步**——行为变更的 commit 必须包含其文档同步，不能依赖独立的"文档任务"兜底；doc-drift 会被 review 抓出但已在 review 后才暴露（成本更高）。可固化：commit 自检增加"改了规则/常量 → 检查 kb/ 是否有对应文字"。
3. **安全修复按"漏洞类"而非"单点"闭合**——--symbols 与日期参数是同类注入面，修复必须枚举全部入口；review 通过 ≠ 无遗漏，复审要验证修复覆盖了 finding 描述的全部范围。

**Process improvements**:
- 已落实：无机制变更（本次为 review 修复闭环 + evidence 补正，规则已在 AGENTS.md/KB 中；43→BJ 规则已同步三处文档，F1 evidence 已补正）
- 建议（可检测失误）：plan 的 Final verification wave F1 增加"evidence 文件日期/commit 计数与 HEAD 一致"自检项——proposed，走 gate 建 issue 时评估
- 建议（行为类）：commit-msg 前自检清单增加"规则/启发式改动 → 同 commit 检查 kb/ 对应文字"——proposed

### Trends (last 10)
- **"完成声明先于验证/声称过期"模式延续**（ref #160 → #174 → 本次 F1 "9 commits" 过期声称）：声明 plan 完成前的证据核实是反复被"学到"但未固化的教训——F1 evidence 应在实现收尾后统一写，且 evidence 本身内容要可复核（commit 计数、grep 结果）
- **doc-drift 反复出现**（ref #171 陈旧文档、ref #139 决策记录同步、本次 43→BJ 未同步 kb/）：行为变更（规则/启发式/默认值）与 kb/ 文档必须同 commit 提交——"文档任务"兜底模式已被证实两次失败，应固化为 commit 自检
- **安全/质量修复不完整导致复审 FAIL**（ref #154 两轮修复、本次 security lane 抓到日期注入）：review 发现的修复必须逐条验证覆盖 finding 全部范围——"修了主要实例"不等于"闭合漏洞类"


## 2026-08-08 — ref #203 创建独立 test agent（认知独立 vs 权限隔离）

**What was done**: 创建 `.opencode/agent/test.md`（独立 QA subagent）+ AGENTS.md 登记。核心设计决策：test agent 与 `qa` skill 并存——skill 注入方法论（怎么测），agent 提供认知独立（谁来独立判断测什么）；edit 权限放开到 `crates/**/src/**/*.rs` + `collectors/**/*.py`，用三层兜底（指令约束 + bash 收紧 + 无提交权）保证独立验证。

**User corrections**（逐字引用对话记录）:
1. "把 agent 的价值错误限定为'权限隔离'，忽略了 AI 系统里更重要的'上下文隔离和认知独立'" —— 纠正我对 agent/skill 差异的理解：agent 的价值核心是独立上下文与认知独立，不是权限隔离；测试 skill 适合执行流程，test agent 扮演独立 QA 角色
2. "2 有点问题，单元测试在文件的内容。。。" —— 纠正我 Q4 权限设计：Rust 单测 `#[cfg(test)]` 内嵌源文件，路径级权限（只允许测试文件路径）无法区分"文件里的测试部分"与"生产部分"，必须放开 edit 到整个 src 并用三层兜底替代硬隔离

**What went wrong**: 
- 初始权限方案犯了"路径隔离迷信"：设计了 `**/*test*.rs`、`**/tests/**` 的 edit 白名单，没有先想清楚 Rust 单测内嵌源文件的现实——正是用户纠正点 1 批评的"把 agent 价值限定为权限隔离"的实例。被用户点破后才改成三层兜底。
- git 流程验证：本次 commit `97a78e9` 在 master，但当时已有两个活跃 worktree（sepa-unit、financial-f10）。这是**工具链配置变更**（.opencode/ 下，非产品代码），按"master 只允许 docs/lint/typo/反思类直推"判定为可接受的直推——但未向用户明示该判定，应在交付时声明。

**Lessons learned**:
1. 设计 agent 权限时，先想清楚**代码现实**（Rust 单测内嵌 = 路径隔离不可行），再设计隔离机制；硬权限做不到时，用"指令约束 + 工具限制 + 流程兜底（无提交权+主agent审查）"组合替代——隔离的本质是**判断独立**，不是文件物理隔离。
2. agent/skill 差异的建模：skill = 方法论（how），agent = 认知主体（who judges what）——复杂长期项目需要独立 agent 保证验证者≠实现者；简单项目 skill 足够。这是 agent 设计的原则性决策，不是工具配置细节。
3. 工具链配置变更（.opencode/ 下）在活跃 worktree 存在时直推 master，需在交付时显式声明判定依据（非产品代码 + docs 类），避免模糊。

**Process improvements**: 
- 已落实：AGENTS.md 新增「test Agent（独立 QA）」章节——路由规则（门禁第 4 步 RED 委派 + 实现后独立复核 + /test 手动触发）、职责边界（不写生产实现）、三层兜底说明。
- 建议（可检测）：`.opencode/agent/*.md` 新 agent 创建后可加"agent 文件 YAML frontmatter 合法性校验"（opencode 启动时校验，属于运行时行为，无需额外 hook）——proposed

### Trends (last 10)
- **用户纠正持续指向"原则/建模"而非"值/细节"**（ref #200 "去掉模型约束" → 本次 "认知独立 vs 权限隔离"）：AI 倾向工具化最小修正（权限白名单/改引用），用户倾向原则性建模（认知主体/配置归属）——设计新机制时应先问"这个机制在系统里的正确抽象是什么"，再谈具体参数


## 2026-08-08 — ref #201 SEPA 单位口径修复：import 侧换算 + 指数剔除

**What was done**: 修复 SEPA 评分单位 bug——`import` 侧 SELECT 对 `volume × 100`（手→股）、`amount × 1000`（千元→元），并无条件剔除 6 个指数代码（SH000300/SH000852/SH000905/SH000906/SH000985/SZ399300）于主查询与 symbols.txt 枚举；重跑全量 import + `sepa score --top 50` 冒烟验证选满 50 只、无指数、茅台通过过滤。

**User corrections**:
1. "rebase，然后review" —— 用户指定**顺序：先 rebase 再 review**。实际执行时 review（5-agent review-work）在 rebase **前**已完成，rebase 后仅重跑测试+clippy 验证、未重新触发 review-work。rebase 是干净 fast-forward 无冲突、review 结论对 rebase 后内容仍成立，但执行顺序与用户指令不完全一致——应在 rebase 后显式重跑 review 或向用户说明 review 已在前一 base 上完成且结论不变。

**What went wrong**:
- **冒烟时 matched=0（真实数据坑）**：重跑 `import` 生成前缀格式 `stock_daily.parquet` 后，`sepa score` 匹配 0 只——`stock_basic.parquet` 仍是 8-01 的旧裸码格式（`000001`），与前缀 daily 无法 join。修复：重跑 `import-compass --table stock_basic` 刷新前缀格式，matched 恢复 4703。此问题属于 handoff 契约"顺带解决旧 parquet 裸码 symbol 问题"的直接体现，但当时只计划重跑 import、未预见 stock_basic 也需刷新——真实数据冒烟（ref #154 教训）正是暴露此依赖链的唯一途径。
- **review 顺序偏差**（见 User corrections 1）：rebase 后未重跑 review-work，仅以测试+clippy 代替。

**Lessons learned**:
1. 用户给出"先 X 再 Y"的顺序指令时，严格按序执行：先 rebase 到最新 base，再在 rebase 后的内容上跑 review-work；若 review 已先行完成，必须在 rebase 后显式重跑（或向用户声明"review 在前一 base 上已通过、rebase 无冲突、结论不变"并获认可）。
2. 数据管线变更的冒烟必须是**全链路**（import → 下游消费方）：单位/格式修正会影响所有关联 parquet（stock_daily 与 stock_basic 的 symbol 格式必须一致才能 join）——刷新主表时盘点所有依赖它的副表，一并刷新。

**Process improvements**: 
- 已落实（docs）：`kb/dev/toolchain.md` 指数混源卡片补注 #201 已落地 import 侧剔除（随本 PR 提交）。
- 建议（可检测）：数据管线变更的 plan 中，冒烟步骤显式列出"全链路验证（含所有依赖副表格式一致性）"——proposed（下次 plan 模板层面落实）。

### Trends (last 10)
- **用户纠正指向"顺序/流程语义"**（ref #201 "rebase，然后review"）：AI 倾向于"先做重要的事（review）再做形式步骤（rebase）"，用户关注指令字面顺序——复合指令中的顺序词（先/然后）是硬约束，不是建议；本条与 ref #190（写库后立即 commit）同属"执行顺序纪律"类别
- **真实数据冒烟暴露格式不一致**（ref #154 → ref #201 matched=0）：跨文件依赖（daily 前缀 ↔ basic 前缀）在 fixture 测试中不可见，只有全链路真实冒烟能暴露——数据管线变更的冒烟清单应从"单命令验证"扩展为"消费链末端验证"


## 2026-08-08 — ref #205 open-worktrees.sh worktree 内 --close 失效修复

**What was done**: 修复 `scripts/open-worktrees.sh` 从 worktree 内部执行 `--close` 时输出 "not a worktree" 的 bug。根因：`PROJECT_ROOT` 用 `dirname "$0"` 相对路径解析，worktree 内调用时 `$0` 相对路径指向 worktree 自身。修复：抽出 `resolve_project_root()` 用 `git rev-parse --git-common-dir` 定位主仓库（worktree 内返回绝对路径），`SELF` 改为基于 PROJECT_ROOT；加守卫防 PROJECT_ROOT 静默为空（Security review 发现）。扩展测试 3 例 + docs 同步。

**User corrections**（逐字引用对话记录）:
1. "脚本输出'not a worktree'——检查实际状态：关闭worktree的时候发现关闭有问题？" —— 引导我实际检查而非直接假设脚本行为
2. "是的，在worktree里面执行的。" —— 澄清关键触发条件：脚本是从 worktree 内部（相对路径）执行的，这是复现 bug 的必要前提
3. "自己新建一个worktree，模拟一下这个脚本运行情况。" —— 纠正我的诊断方式：我此前只做代码分析和 dry-run 复现，用户要求新建 worktree 完整模拟真实执行路径——实际模拟立刻暴露了"测试套件用 sed 提取函数 + eval 通过、但真实执行仍失败"的差异（worktree 内执行的是旧 checkout 副本）

**What went wrong**:
- **流程违规（agent 自身，ref #206 机制后补记）**：执行 #205 修复时存在活跃 worktree `fix/financial-f10`，但实现直接在 master 提交（判定为"单文件修复跳过 worktree"）。按 AGENTS.md 规则"存在活跃 worktree 时实现类提交落在 master 即流程违规"——Context Mining lane 明确指出此偏差，当时的反思却未记录。教训：单文件修复判定也不能绕过 worktree 规则；涉及"关闭 worktree 的脚本"本身时更应谨慎（脚本修复影响 worktree 管理，属于工具链，但实现位置判定仍需显式向用户声明）。
- **效果不符预期（agent 自身）**：测试套件（sed 提取函数 + eval）全绿但真实脚本执行仍失败——因为 worktree 内的 `scripts/` 是独立 checkout 副本，测试用的 SCRIPT 是主仓库路径，而用户场景执行的是 worktree 副本。首次冒烟只做了 dry-run，未在 worktree 内相对路径模拟，直到用户要求"新建 worktree 模拟"才暴露。冒烟测试（wt-sim 新建 worktree 从内部执行）才完整验证。
- **命令/工具用错（agent 自身）**：诊断 F10 报表 filter 参数时多次尝试 `(SECUCODE="...")(REPORT_DATE='...')` 复合条件（Python urllib 编码、单/双引号变体），均失败，最后用 curl 直接测试才成功——filter 复合条件的正确编码方式（%28...%3D...%29）应在 toolchain 记录，避免重复试错。
- **效率摩擦（agent 自身）**：第二轮 Code Quality re-review 运行 19 分钟未完成（7 行 delta 正常应 1-2 分钟），延迟识别为卡住并取消重试——review lane 长时间无输出时应及早标记 inconclusive 并重派，而非被动等待。
- 修复对"已存在 worktree"（financial-f10）不生效：其脚本副本是旧版，需 merge 后重新同步。已在 docs 中注明该限制。

**Lessons learned**:
1. bash 脚本的"自包含测试"（sed 提取函数 + eval）无法覆盖顶层初始化逻辑（如 PROJECT_ROOT 解析），此类脚本必须补真实执行冒烟（新建 fixture worktree 从目标 cwd 调用）——测试套件全绿 ≠ 真实场景可用。
2. worktree 内执行脚本时 `$0` 相对路径解析的是 worktree 副本而非主仓库脚本——涉及"定位主仓库/项目根"的脚本逻辑，一律用 `git rev-parse --git-common-dir` 而非 `dirname $0`；同时注意已存在 worktree 的脚本副本需同步才生效。

**Process improvements**: 
- 已落实：`scripts/open-worktrees.sh` 抽 `resolve_project_root()`（git-common-dir 定位）+ PROJECT_ROOT 空值守卫；`kb/dev/process.md` 记录 worktree 副本同步注意点。
- 已落实：测试扩展 3 例（worktree cwd / repo root / 仓库外 fallback），其中 worktree cwd 用例正是本 bug 的回归保护。
- 建议（可检测）：`open-worktrees-test.sh` 可增加"从 fixture worktree 内部真实执行脚本"的端到端用例（当前 #22 只验证 resolve_project_root 函数，未验证顶层 PROJECT_ROOT 集成）——proposed

### Trends (last 10)
- **测试全绿 ≠ 真实可用 反复出现**（ref #139 真实数据冒烟、ref #154 冒烟证据、本次 worktree 内执行）：fixture/单测覆盖不到"真实执行路径"（副本、cwd、环境）——脚本类变更必须做真实路径冒烟，不能只看测试套件
- **用户纠正持续指向"实际验证"而非"代码推理"**（ref #200 "去掉模型约束" → 本次 "新建worktree模拟"）：AI 倾向从代码/测试推导结论，用户倾向真实场景复现——发现"测试通过但用户说不行"时应立即怀疑测试与真实路径的差异


## 2026-08-08 — ref #208 mold 链接器 + collectors CSV 输出目录统一

**What was done**: (1) 新增 `.cargo/config.toml`（参考 atom 项目布局）：Linux 启用 mold（`linker="clang"` + `-fuse-ld=/usr/bin/mold`），macOS/Windows 默认链接器占位，Nightly flags 注释保留；CI `rust`/`bench-check` job 安装 mold+clang；AGENTS.md + kb/user/index.md 补 mold 前置条件。(2) collectors 全部 11 个采集器默认 CSV 输出从 `collectors/` 相对路径统一到 `csv_dir()`（`/data/compass-data/csv`，`COMPASS_CSV_DIR` env 可覆盖），`-o/--output` 保留覆盖；`main.py` import 路径同步；conftest autouse fixture 隔离测试目录；review 修复后补 `csv_dir()` mkdir + 删 `COLLECTORS_DIR` 死代码。3 commits（735d4ea/8d7bca4/2c24f68），5-way review 两轮。

**User corrections**（逐字引用对话记录）:
1. "支持各个平台。 所有需要编译的都需要，要不然就编译不过了。。。" —— grill Q2 我推荐"只配 Linux target 段"，用户否决：要求像 atom 一样覆盖各平台段，且 CI 所有编译 job 都要装 mold（否则 rustflags 引用 mold 会编译失败）
2. "本地编译之前，先clean一下旧的编译产物。" —— 我原本计划直接 cargo build 验证，用户要求先 `cargo clean` 再编译，确保链接证据来自全新构建而非增量缓存
3. "抓取的 CSV 原始数据保存到 compass_data 目录下，这个也作为一个要求" —— 在 mold 审查进行中追加无关的新需求（CSV 输出目录），并确认"追加到 #208 验收标准"而非独立 issue

**What went wrong**:
1. **测试全量跑污染真实数据目录**：`csv_dir()` 落地后第一次全量 pytest（326 个）中，6 个旧测试把 `RPT_DMSK_FN_*.csv` 等写入真实 `/data/compass-data/csv/`（20:05-20:07 时间戳），污染需手动 trash 清理——conftest autouse fixture 是 review 前才补的，RED 阶段契约测试与既有测试跑混合时已发生污染。测试隔离应在实现第一批代码时就位，而非等全量跑发现污染。
2. **review Round 1 FAIL（MAJOR）**：`COLLECTORS_DIR` 在 main.py 改 csv_dir() 后成死代码，且 test_main.py 两个 legacy 测试类仍 monkeypatch 它（no-op，靠 conftest autouse 恰好指向同 tmp_path 才通过）——code quality review 抓出"为错误理由通过"的误导性测试。批量替换路径引用后未清理旧常量与旧测试机制。
3. **Goal Verification oracle lane 卡死 1h10m**：5-way review 中 4 lane 正常完成，goal lane 长时间无输出，respawn 替换 lane 才拿到结论；原任务最终被系统回收（task not found）。应更早（~30min）判定 lane 失活并替换，而非等 1h+。
4. **`-o` help 文本泄漏代码标识符**（NITPICK）：`help="default: csv_dir()/stock_basic.csv"` 把 `csv_dir()` 函数名写进用户可见的 `--help`——应插值真实解析路径。

**Lessons learned**:
1. **路径/目录语义变更时，测试隔离必须与实现同批落地**：任何"默认输出目录"类改动，第一步就是 conftest autouse fixture 指向 tmp_path（或等价隔离），再写实现——顺序颠倒必然污染真实环境
2. **批量替换后必须清理死代码与失效测试机制**：改引用点后 grep 旧常量全仓（含测试 monkeypatch），并逐测试确认"断言的是真实行为而非巧合通过"——review Round 1 的 MAJOR 正是这类残留
3. **review lane 失活判定阈值**：oracle/子任务 30 分钟无输出即 respawn 小 lane（替换任务），不无限等待；原任务回收后清理
4. **用户可见文本不写代码标识符**：`--help`/报错信息展示解析后的真实值（`csv_dir() / 'x.csv'` 的实际路径），而非函数名

**Process improvements**:
- 已落实（随实现提交）：`collectors/tests/conftest.py` autouse `_isolate_csv_dir` fixture——任何新测试默认 COMPASS_CSV_DIR 指向 tmp_path，杜绝数据目录污染（本条目教训 1 的固化）
- 已落实（随实现提交）：`csv_dir()` 内 `mkdir(parents=True, exist_ok=True)`——消除首写 FileNotFoundError
- 教训 2/3/4 为一次性过程教训，写入本条目（None）

### Trends (last 10)
- **真实环境隔离反复出现**（ref #190 Dolt 工作区滞留、ref #154 冒烟证据、本次测试污染 /data/compass-data/csv）："测试/写库不得触碰真实数据环境"是反复教训——本次已固化为 conftest autouse fixture，但 Dolt 侧（ref #190）仍是人工纪律；建议将"测试隔离"检查项纳入 compass-workflow 门禁第 4 步（TESTS）的强制清单
- **review 抓出批量替换残留**（ref #202 三采集器不同构、本次 COLLECTORS_DIR 死代码）：批量/并行改同构代码后，主 agent 必须做"残留模式 grep"自查（旧常量、旧测试机制、同构字段），不能只信子任务自报全绿
- **用户对范围与顺序的追加要求**（本次 CSV 追加到 #208、ref #202 配套代码、ref #201 顺序语义）：用户在实施中追加需求/纠正顺序是常态——grill 阶段把"影响面"（含测试/文档/数据环境）问透，比实现中追加再改成本低


## 2026-08-09 — ref #230 Button 主题感知文字色 + loading 宽度观感修复

**What was done**: 修复两个 UI 问题：①Primary/Danger 亮色主题文字看不清——新增 `on_accent`/`on_error` token（两主题纯白），Primary/Danger 改用之，Default/Ghost 维持 text_primary（light 下 text_primary 深字落彩底 3.19:1 < WCAG AA，白字 4.90:1）；②SEPA 刷新按钮 loading 宽度观感——根因调查（kittest 断言）证实宽度实际跟随文本（33.7→59.0px），「未变」系 loading 遮罩观感；新增 `Button::min_width(f32)` 让两态宽度一致（SEPA 96 / Fetch 104）。双测试 agent 写 RED（需求 9 + 对抗 7+2），实现后 233 lib + 11 + 7 全绿，review-work 5/5 PASS。commit `0a06bc9`。

**User corrections**（逐字引用对话记录）:
1. "primary 按钮始终是蓝色的，这样亮色下，文本就是黑色的，看起来看不清。然后按钮的大小也没有跟随文本的改变而改变宽度（sepa的刷新按钮点击效果）。 让设计师设计一下。"——GUI 冒烟后用户报告两个新 UI 问题，要求 ui-designer 设计（#230 起点）。
2. "1. 暗主题用白色合适吗？？我不确定啊 2. 微调 3. 为什么？ 4. 需要查根本原因，也许是被其他ui遮挡了？？？？需要后续确认。"——对设计方案的 4 点质疑：dark 纯白需对比度数据支撑（最终选 A 纯白）、min_width 数值实现时实测微调、空态按钮为何不加 min_width（代码事实：无 loading 切换）、**宽度问题必须查根因（怀疑被 UI 遮挡）**。
3. "1. push 2. 在当前worktree处理。"——push #226-228 PR；#230 在当前 worktree 处理（复用分支）。
4. "流程结束，自动push，并关闭worktree"——授权自动 push + 收尾关闭 worktree。

**What went wrong**: ①**需求 agent 与对抗 agent 并行写入重复测试**——`loading_button_keeps_variant_text_color`（需求 agent 更新旧断言）与 `loading_button_keeps_on_accent_variant_text`（需求 agent 新增）内容重复，review 抓出 [MINOR]；两 agent 并行时应在委派 prompt 中明确「避免与对方重复」的协调机制。②**对抗 agent 输出被截断**——其计划（对比度功能断言、非 alias 断言）部分未落地到最终测试集，实际落地由需求 agent 的 color.rs 测试 + 对抗的集成测试覆盖；后台 agent 长输出需关注完整性。③**Fetch 按钮无精确文本宽度测试**——仅 API 层断言，验收「Fetch 两态一致（min 104）」缺「加载中…」精确文本断言（FYI，min 为下限风险低）。④LSP 诊断滞后于 cargo 编译（on_* 字段已加仍报错），以 cargo 为权威验证。

**Lessons learned**:
1. **并行双测试 agent 的协调**：两个测试 agent 同时写同模块测试时，委派 prompt 需明确划分边界（需求 agent 写 mod tests 契约测试、对抗 agent 写 tests/ 集成测试）并声明「避免重复断言同一契约」——本次 loading 文字色测试双写被抓。
2. **UI 宽度类问题根因调查先行**（用户明确要求）：kittest 断言（response.rect.width() idle vs loading）直接锁定「宽度真实跟随文本」→ 方案从「修宽度」转为「修观感」（min_width 稳定两态）；先锁定根因再选方案避免修错方向。
3. **后台 agent 长输出完整性**：对抗 agent 输出超长被截断，需在委派时要求「结论摘要放最前」或分块返回，避免计划未落地。
4. **on_* token 语义**：Material `on-*`（彩色实底上的对比前景）与 `text_primary`（普通浅底主文字）语义分离，是解决「同色值两场景对比度不同」的正确分层——ref #217 统一 text_primary 决策在 light 主题的边界条件被 #230 暴露。

**Process improvements**:
- None（一次性/已落实：设计文档 `.omo/designs/button-theme-and-width-fix.md` 已提交；kb/design/ui.md L261 决策记录已修订为 ref #230 版本；ui-widgets.md Button 条目已同步。测试 helper 重复 → proposed 提取 `tests/common/mod.rs`）。

### Trends (last 10)
- **「先猜根因再验证」返工模式持续出现**（#139/#217 布局诊断、本次 #230 宽度观感）：本次因用户明确要求「查根本原因」而走了 kittest 断言先行，直接锁定根因（宽度真实跟随、遮罩观感）——验证「先复现拿证据再二分」有效，建议在 kb/dev/process.md 调试章节固化该排查框架（proposed）。
- **ui-designer 设计委派流程已成标准路径**（#217 → 本次 #230）：design-first（产出 .omo/designs → 用户逐点确认 → 实现）两次均获认可，无偏差。
- **并行子代理测试重复**（本次 loading 文字色测试双写）：新出现模式——需在双测试 agent 委派时显式划分边界，观察后续是否再现。

## 2026-08-10 — ref #238 skwy-reflect 反思文件 >500 行自动归档规则 + 归档执行

**What was done**: 给 skwy-reflect skill 新增第 5 步「反思文件超行数自动归档」：>500 行自动触发（`wc -l` 检查），三分类处理（值得处理的列候选建 issue 后归档、已处理的直接归档、剩余的保留待下次检阅、归档后仍超 500 行交用户判断），归档沿用脚本切分 + 行级丢失校验；同步 AGENTS.md / reflections.md 头部 / kb/design/workflow-skills.md 决策记录。对当前 801 行 reflections.md 执行首次归档：23 条归档（19 已处理 + 4 建 issue 后）、7 条保留、0 行丢失，主文件降至 208 行。建 issue #239（testing.md GUI 测试方法论）/ #240（process.md 磁盘预检+大库小样本）。commit 68fa517。

**User corrections**（逐字引用对话记录）:
1. "1， 4， 5， 6 建立issue。描述清楚。2和3不处理。" —— 从反思评估候选中选定建 issue 项，否决 sepa Dolt commit 内置与后台任务失活判定两项
2. "反思agent 加一个规则，超过500行，就自动归档一次，值得处理的建立issue后归档，已经处理的直接归档，剩余的保留不归档，等待下次归档时检阅。" —— 规则需求（行数阈值 + 三分类）
3. "就是排除掉建立issue的，和已经处理的。剩下的部分。如果之后还是超过了500行，就交给用户判断" —— 纠正我的"最近 2 条+待检阅"保留标准推荐：保留 = 排除建 issue 和已处理之外的全部；归档后仍超 500 行交用户判断

**What went wrong**:
1. **归档脚本两次返工**：① UNKNOWN refs 校验把条目标题中的次要引用（#175/#179/#182/#189/#227/#228——epic 子 issue 号）误判为未知——校验逻辑假设"标题 ref ⊆ 分类集"不成立（分类用交集判断本身正确）；② 删除该检查时把 arch_refs/keep_refs 定义一并删除，后续 print NameError——第三次才成功。一次性工具缺 dry-run/自测。
2. **数量声明未经验证**：三分类检阅时声称"29 条活性条目"，实际 30 条（grep -c "^## " 确认）；"已处理 26 条"计数与最终归档数 23 不符。凭阅读印象声明数字，未先命令计数。
3. **edit oldString 换行不匹配一次失败**：reflections.md 头部自动归档说明编辑时，oldString 将原文两行合并为一行，匹配失败重试——#163 排查卡"足上下文"教训的换行变体。

**Lessons learned**:
1. 归档/数据操作脚本先 dry-run（print-only 验证分类结果：条目数、归档/保留集）再写文件；校验逻辑不假设标题 ref 全集 ∈ 分类集（标题常含 epic 子 issue 等次要 ref），用交集分类 + unclassified 检查
2. 数字声明（条目数/commit 数/行数）前一律 grep/命令计数验证——ref #181 evidence "9 commits" 过期声称同类，声明数字不凭阅读印象
3. 可复用的数据操作脚本入库（scripts/）供第 5 步复用，避免每次 /tmp 重写 + 返工

**Process improvements**:
- 已落实：skwy-reflect skill 第 5 步（>500 行自动归档 + 三分类 + 行级校验）；AGENTS.md L314 归档描述；reflections.md 头部说明；kb/design/workflow-skills.md 决策记录（commit 68fa517）
- 已建 issue：#239（testing.md GUI 测试方法论）、#240（process.md 磁盘预检+大库小样本）；上轮 #234-237
- proposed（代码类）：归档脚本入库 `scripts/archive-reflections.py` 供第 5 步复用（本次 /tmp 脚本两次返工暴露的一次性工具问题）——走 gate 建 issue 评估
- 教训 2/3 为一次性，写入本条目

### Trends (last 10)
- **"数字/状态声明不实"模式延续**（ref #181 evidence "9 commits" 过期声称 → 本次"29 条"实为 30 条）：声明数量前必须 grep/命令验证——可检测失误，建议 reflect 模板加"数字声明经命令验证"提示
- **数据操作脚本的工具链摩擦**（ref #186 脚本切分成功先例 → 本次脚本两次返工）：脚本化+校验（行级丢失）是正确方法，但脚本自身需 dry-run；可复用的数据操作（归档）应入库而非 /tmp 一次性
- **一次性临时工具导致返工**（ref #205 自包含测试不覆盖顶层 → 本次 /tmp 归档脚本）：工具质量与流程同等重要——数据操作的"工具验证"应与"数据校验"并列

## 2026-08-10 — ref #222 gui-i18n 中断恢复 + F2 review 修复 + push 前收尾

**What was done**: 恢复中断的 gui-i18n rebase（abort 过期 rebase 基点 ef0fbc8 → 重做 16 picks 到最新 origin/master），解决 3 处冲突；修复 2 个 rebase 引入的测试回归（语言下拉 harness 尺寸、dropdown hint 测试适配）；执行 plan F2 门禁（/review-work 5 lanes → 2 FAIL），修复 2 个 blocking（factor-note 精度回归、locale 顺序依赖测试）+ 增量重审双 PASS；同步 origin/master 二次推进（#238），push 前就绪核查通过。分支 feat/gui-i18n 21 commits。

**User corrections**: 无纠正型消息 — 本 session 仅"继续，之前系统重启了，被打断了"（中断恢复指令）与"push并合并pr。关闭worktree。"（明确 push 指示）。全程无方向纠偏。

**What went wrong**:
1. **git rebase --continue 挂起 120s 超时**（GIT_EDITOR=nvim 环境，git 打开编辑器等待输入）——toolchain.md L150 已有排查卡（原 session 沉淀），但首次遇到时先按"hook 慢/gh 慢"猜了 2 轮（查 hooks、查 pre-commit）才想起查工具链卡。教训：git 命令挂起/超时第一动作是查 toolchain.md 已知坑表，不是猜根因。
2. **F2 review 暴露 2 个实现期缺陷**：① factor-note 精度回归——`factor_note_text` 裸 f64 插值丢 `{:.1}`/`{:.0}`/`{:+.0}`，实现期测试全用干净值（12.3/75）掩盖；② 9 个 zh 断言测试依赖全局 locale 污染（隔离必失败），违反 toolchain.md L222 已记录的 default-locale no-op 契约。两者都是"实现时该发现的坑，靠 F2 独立 review 才暴露"。
3. **origin/master 在 push 准备期二次推进**（#238 反思归档，2 commits）——rebase 后至 push 前又落后，需二次 rebase。教训：rebase 完成 ≠ 永远同步，push 前必须重新 fetch 校验（AGENTS.md 已有规则，执行了但未预期这么快再推进）。
4. **LSP 陈旧缓存误报**：sepa.rs 编辑后 LSP 报"no such field: label/note"（旧字段名），实际是 rebase 冲突解决后的 stale 索引——重复 3 轮才确认是缓存问题，浪费诊断轮次。教训：冲突解决后的 LSP 报错先 cargo check 验证再信。

**Lessons learned**:
1. git 命令挂起/超时/非预期行为 → 第一步查 kb/dev/toolchain.md 已知坑表（GIT_EDITOR/TTY/权限卡），确认无匹配再诊断根因
2. 数值/格式化类 i18n 变更的测试必须用非干净值（小数、正负号、边界）锁定精度契约——干净值测试让精度回归静默通过（factor-note 教训）
3. 冲突解决/LSP 索引过期的报错以 cargo check 为准，不逐轮猜 LSP 输出
4. push 前就绪核查必须包含"fetch + ahead/behind 复验"，不信任早前 rebase 结果

**Process improvements**:
- 已复用（非新增）：toolchain.md L150 GIT_EDITOR 卡、L222 default-locale no-op 卡——两条卡在 F2 review 中被独立验证准确
- 教训 2（非干净值测试）为一次性实现教训，写入本条目；可考虑后续在 skwy-requirement-test skill 补充"数值断言用非干净值"提示（proposed，未建 issue——低优先）
- 其余教训为一次性，写入本条目

### Trends (last 10)
- **"已知坑未先查"模式**（本次 GIT_EDITOR 挂起猜 2 轮才查 toolchain.md）：工具链卡存在但不作为第一查询对象——建议遇到非预期命令行为时强制先 grep toolchain.md 再诊断
- **"实现期测试干净值掩盖回归"**（本次 factor-note 精度）：review 是最终防线但成本高——数值/格式化断言应主动用非平凡值，写进测试方法论

## 2026-08-11 — ref #234/#236/#237/#239/#240 docs 批次：5 个文档 issue 批量处理

**What was done**: 批量处理 5 个 docs issue——compass 仓库 4 commits（#234 toolchain.md 进程检测排查卡、#240 process.md 磁盘预检+小样本 QA、#237 AGENTS.md F1 evidence 一致性自检、#239 testing.md GUI 测试方法论四节）+ skwy 仓库 1 commit（#236 skwy-requirement-test SKILL.md 委派 prompt 两条款）。纯文档变更，门禁例外适用；commit 后证据式核对 5 个 issue 验收标准全过。

**User corrections**（逐字引用对话记录）:
1. "236 234 239 处理。  240 237 不是很清楚是做什么的，详细介绍下。" —— 用户先锁定 3 个 issue，要求我详细解释另外 2 个再决定（范围澄清）。
2. "全部处理。然后问一下，evidence是什么" —— 用户扩展范围为全部 5 个，并追问 evidence 概念（我以 `.omo/evidence/` 实际文件佐证回答）。
3. "236 外部  237 agents.md" —— **关键落点纠正**：我推荐 #236/#237 全部落本地 AGENTS.md，用户纠正 #236 落外部（skwy-requirement-test skill）、#237 落本地 AGENTS.md。
4. "hao" / "push" —— 确认 Q5 批次分工、确认 push。

**What went wrong**:
1. **`git commit --amend` 误改 HEAD 的 message 而非目标 commit**：想改 05a5992（testing，中英混杂 message）时直接 amend，但 HEAD 实际是最后一个 commit（AGENTS.md e857b0a）——amend 把 testing 的 message 安到了 AGENTS.md commit 上，内容与 message 错配，且原中文 message 残留。需 soft reset 重来（浪费 3 轮）。教训：**amend 前先 `git log --oneline -1` 确认 HEAD 是目标 commit**。
2. **`git reset --soft` 后两个文件都 staged，首 commit 混入**：soft reset 到 process commit 后 AGENTS.md 与 testing.md 均为 staged 状态，`git add AGENTS.md && commit` 把两个文件一起提交（68 insertions = 12+56），需 `git restore --staged` 拆分重做。教训：**批量分 commit 时，reset 后先 `git restore --staged` 排除非目标文件再提交**。
3. **commit message 混入中文「撑宽」**：违反"提交信息按惯例使用英文"（AGENTS.md 明文规则），触发上述 amend 连锁。教训：commit message 写完后自查中英混排（尤其中文技术术语如"撑宽"随手带入）。
4. **commit-msg hook 误报 #237 MISSING**（gh API 瞬时故障）——toolchain.md 已有记录，诊断（手动 gh view 返回 OPEN）后重试成功，非 agent 失误，正常闭环。

**Lessons learned**:
1. **amend 前必须确认 HEAD**：`git commit --amend` 只作用于 HEAD——目标 commit 非 HEAD 时先 `git log --oneline -3` 核对，或改用 `git rebase -i` 精确 reword；commit message 写完自查（中英混排、ref 前缀）。
2. **soft reset 拆分提交前先清理 staged**：`git reset --soft <base>` 后所有变更都 staged，分批 commit 时必须先 `git restore --staged <非目标文件>`，避免首 commit 混入后续文件。
3. 批量处理多 issue 时每 issue 独立 commit 且内容/message/ref 一一对应，提交后立即 `git log --stat` 验证——发现问题当场修复，不留到 push 前。

**Process improvements**:
- 无机制变更（纯 docs 批次，不新增规则）。amend/reset 操作纪律为一次性操作性教训，写入本条目——commit-msg hook 与 pre-commit 已覆盖可检测部分，操作顺序类摩擦难以 hook 化。

### Trends (last 10)
- **commit 操作摩擦高频出现**（ref #184 commit-msg 误写 → ref #222 rebase 挂起 → 本次 amend 误操作 + staged 混入）：git 操作纪律（amend 前确认 HEAD、reset 后清 staged）建议沉淀到 kb/dev/process.md「版本控制」章节——commit 操作是比实现代码更高频的摩擦源
- **「文档已固化但未遵守」模式**（ref #184 记录第五次 → 本次 commit message 中文）：AGENTS.md 规则存在但执行时未查——commit 前自查清单（message 英文 + ref 前缀 + 叙述性提及用 #N）值得做成 pre-commit 可检测项或提交模板

## 2026-08-11 — ref #46/#132/#71 timeframe-theme-stocklist 批次：3 个 issue 批量修复

**What was done**: worktree `fix/timeframe-theme-stocklist` 批量处理 3 个 issue——#46 timeframe 聚合补端到端集成测试（聚合已在 master 2982b72b 实现，验证后关闭）、#132 主题切换写回 config.toml（save_theme_config 镜像 save_language_config）、#71 股票列表 GUI 层 delist_date 过滤。6 个 commit（#46 测试 / #132 / #71 / docs / plan / F-wave evidence），856 测试全绿，review-work 5 agent 全 PASS。

**User corrections**（逐字引用对话记录）:
1. "bug应该是点击周和月没有立即fetch吧，再看看？" —— **方向纠正**：我探索后判定 #46"聚合已实现、issue 只剩收尾"，用户指出真正的 bug 可能是"点击周/月不立即 fetch"——促使我重新审视 GUI 链（最终确认 GUI 已有 segmented_switch 测试覆盖点击触发 fetch，provider 聚合测试全绿，结论仍是无代码缺口、补集成测试收尾；但用户的质疑点提示"已实现"≠"验证过完整链路"）。
2. "没有问题就写一个集成测试，通过的话，就关闭这个issue" —— **范围裁决**：#46 处理方式定为"验证后收尾"，不扩展 ParquetReader 聚合等缺口。
3. "全部完成后push" —— **push 时机指示**：等 #132/#71 全部完成再 push，不单独 push #46。
4. "任务完成后自动push，merge pr 并关闭worktree" —— **闭环授权**：push + merge PR + 关闭 worktree 全部自动化。

**What went wrong**:
1. **#71 探索阶段误信 handoff 假设**："stock_basic 表仅含当前上市 A 股"——实测 5,888 行含 354 退市 + 21 B 股（全退市）。幸而在规划阶段用真实 parquet 数据验证（非实现后才发现），纠偏为 delist_date.is_none() 单条件。教训：**handoff/issue 里的数据面断言必须用真实数据核实，不能当作事实**——尤其涉及"表内容/数据量"的声明。
2. **两个测试 agent 并行写同一文件产生重复**：adversarial + requirement 两个 agent 并行在 main.rs 各写了一套 #71 测试（fixture ×2、filter 测试 ×2、unchanged 测试 ×2），我需手动去重（合并独特断言 + 删重复块）。教训：**同一文件的并行测试委派应明确划分区域或串行**，或委派前指定"先看对方已写内容"。
3. **git add -p 拆分 hunk 三次失败**（printf 管道提前 EOF、python 驱动卡死、e 编辑模式超时）才改用 tmux send-keys 成功。教训：**git add -p 非交互环境不可靠——分 commit 场景改用 tmux 驱动或预先规划好 hunk 应答序列**。
4. **docs 的 #132 段误入 #71 commit**：add -p 时 ui.md 主题段与股票段在同一 hunk 无法拆分，导致 #132 主题 docs 混进 #71 commit，reset --soft 重做。教训：**多 issue 单文件 docs 变更，要么接受"docs 独立 commit 引用多 issue"，要么实现前就按段拆分**（后者成本高，接受前者更务实）。
5. **save_theme_config 注释残留探索期行号**："// ← 行 1125，切换生效"（测试 agent 引入，行号已漂移失效）——hook 移除。教训：**行号注释是最容易过时的注释类型，任何代码变更后必失效**。

**Lessons learned**:
1. **数据面假设必须先实测**：涉及"表包含什么/数量多少/哪些被过滤"的断言（handoff、issue、plan 里），实现前用真实数据（DuckDB 查询、parquet 读取）验证，不把文档陈述当事实——#71 若按 handoff 假设实现会漏掉退市/B 股。
2. **并行测试委派避免同文件竞争**：同一文件（main.rs tests）的两个测试 agent 并行会产生重复 fixture/测试——委派时划分区域（如"你写 save_theme_config 区 4087+，她写 load_stock_list 区 1478+"）或告知"检查文件已有内容避免重复"。
3. **git add -p 在非交互 shell 不可靠**：用 tmux send-keys 或直接接受"同文件多 issue 用独立 docs commit 引用多 ref"的务实方案。
4. 用户"已实现"≠"已验证"：issue 状态 OPEN + 代码存在 + 用户观察到行为异常三者并存时，用户的问题描述（"点击周/月不立即 fetch"）比 issue 文字更接近真实 bug——先复现用户场景再下结论。

**Process improvements**:
- **None（本次无机制变更）**：数据面实测教训（#71）与并行委派教训为操作性经验，写入条目。考虑将"plan 中数据面声明必须标注验证来源"建议给 ulw-plan 流程（proposed，待评估）。

### Trends (last 10)
- **"文档/假设未实测"模式**（ref #46 探索纠偏 → 本次 #71 handoff 假设实测推翻）：涉及数据面断言（表内容、行数、过滤语义）的规划假设多次与真实数据不符——建议 AGENTS.md 或 ulw-plan 增加"数据面声明必须附实测来源"的规划约束
- **并行 agent 同文件写冲突重复出现**（本次 #71 测试重复 fixture）：多 agent 委派到同一文件时产出重复——委派 prompt 强制"检查对方已写内容"或划分区域

## 2026-08-12 — ref #235/#213 data-trim-hook-batch：collectors TRIM + hook 批量查询

**What was done**: 一个 PR 内完成两个修复——#235 collectors 写 Dolt 的文本列 SQL 层统一 TRIM（stock_basic/fin_indicators/财务三表/institution_survey/block_trade，U+3000 盲区锁定），#213 commit-msg/pre-push 的 gh issue 校验改单次批量查询（fail-closed）。7 commits（rebase 后），双通道审查（momus+oracle）3 轮批准，门禁 3.5/4 RED 测试（test_trim_imports.py 23 用例 + gh-issue-list-test.sh），351 Python 测试 + 4 hook 脚本全绿，F1-F4 验证通过。

**User corrections**（逐字引用对话记录）:
1. "F1 按推荐 F2 B" —— 用户确认 F1=推荐 A（纳入 institution_survey/block_trade）、F2=B（hook 内联重复不提取共享脚本）——两个 fork 的决策。
2. "开始，后面直到push，都自动执行" —— 授权门禁 3.5→push 前全程自动执行（这是范围授权，不是纠正）。
3. "任务完成后自动push，merge pr 并关闭worktree" —— 扩展授权到 push + merge + 关闭 worktree（HARD BLOCK 的显式解除）。

**What went wrong**:
1. **F10 TRIM 测试断言目标错误（RED 阶段未暴露）**：对抗测试 agent 的 `TRIM_EXPECTED` 字典值既是输入（带空格，正确）又是断言目标（也带空格，错误）——断言 `_hex(self.TRIM_EXPECTED[c])` 期望带空格值，但 TRIM 实现后落库无空格 → 三表 GREEN 时 3 测试 FAIL。门禁 4 的 requirement agent 复核时只看到"9 RED 合理"就放行，未逐断言核对断言目标与输入的语义关系。RED 测试可以"为错误原因失败"——这正是 TDD 红绿灯陷阱。
2. **门禁 4 复核未抓 F10 断言 bug（承接 1）**：requirement-test agent 复核了契约覆盖率（补了 fin_indicators 8 列 + j2）但没验证既有断言目标逻辑——"验证 RED 存在" ≠ "验证 RED 正确"。
3. **F1 发现 mirror-drift guard 未实现**：plan todo 4 明确要求扩展 hook-standalone-ref-test.sh 的 guard 循环，实现时遗漏（只加了 gh-issue-list-test.sh 的 guard），F1 compliance audit 抓到后补提交。
4. **首次委派 C2 对抗测试结果截断**：agent 输出在规划中途截断，gh-issue-list-test.sh 未创建——续接 session 后完成（浪费一轮）。

**Lessons learned**:
1. **RED 测试必须验证"因正确原因失败"**：断言目标与输入值的关系必须语义自洽（padded 输入 → 期望 trimmed 输出），不能只确认"测试失败数量合理"。门禁 4 复核清单增加"逐断言核对输入/期望关系"步骤。
2. **实现时逐 todo 对照 plan 的 What to do 全项**：验收标准通过 ≠ What to do 全落实（mirror-drift guard 属 What to do ① 而非 acceptance）——F-wave 前自查 plan 每 todo 的 What to do 逐条核对。
3. **委派后验证产出完整性**：agent 声称完成但文件缺失时，续接同一 session 而非重新委派（保持上下文），并验证产出物存在 + 证据落盘。

**Process improvements**:
- 门禁 4 复核增强：requirement-test agent 的 DELIVERABLE 增加"逐断言核对输入/期望语义自洽性"（拟写入 skwy-requirement-test skill 的复核清单，proposed）
- F-wave 前自查：主 agent 在委派 F1 前先逐 todo 对照 plan What to do 全项核对（可固化为 skwy-workflow skill 的 F 波次前自查步骤，proposed）

### Trends (last 10)
- **"RED 因错误原因失败"模式（新，本次 #235）**：对抗测试断言目标语义错误未被门禁 4 复核抓出，直到 GREEN 阶段才暴露——建议 skwy-requirement-test 复核清单增加"断言目标与输入语义自洽"检查（本条目 Process improvements 已提出）
- **plan What to do 未全落实（新，本次 #235）**：验收标准通过但 What to do 细节遗漏（mirror-drift guard），F1 兜底抓出——建议实现完成后、F 波次前逐 todo 对照 plan What to do 全项自查
- **"文档/假设未实测"模式（ref #46 → #71 → 本次 GUI 冒烟）**：plan 假设"GUI 冒烟可验证"但实测发现 X server dead + Parquet 为旧快照（concept_member 11 行脏数据根因在 Parquet 而非 Dolt）——数据面/环境面声明必须实测

## 2026-08-12 — ref #250 per-crate 覆盖率阈值按可测试性调整 + 补测

**What was done**: 调整 per-crate 覆盖率门槛（types/i18n/strategy/ui 80→95%、compass 80→90%、workspace 80→93%，core/data 保持 95%）+ 补测 compass-types（14 个对抗性测试，89.58→100%）与 compass-i18n（提取白名单谓词 + 表驱动测试，93.94→99.14%）。6 commits，5-way review 全 PASS（含 goal/QA/code-quality/security/context），872 测试全绿，8 项门槛实测全超阈值。

**User corrections**（逐字引用对话记录）:
1. "workspace的也改一下，到93%" —— 用户决策 workspace 阈值 80%→93%（原 issue 正文写"workspace=80% 保持"），plan 批准时追加。实测 96.10% 有余量；issue 正文未同步（收尾 comment 已计划注明此偏差）。

**What went wrong**:
1. **llvm-cov double-spawn 竞态诊断过长**：首次 `cargo llvm-cov nextest` 失败（exit 104，nextest 在 compass_core 二进制链接完成前尝试 --list），多轮排查（toolchain 卡、二进制存在性、CI 配置、残留进程）后才确认是一次性构建竞态、重跑即过。应"先重跑验证是否竞态，再深挖根因"。
2. **测试 agent 权限受限改变交付落点**：skwy-adversarial-test 仅可写 `**/tests/**`，compass-types 测试落到 `tests/adversarial_serde.rs` 而非 lib.rs mod tests；skwy-requirement-test 无法 edit `crates/compass-i18n/src/lib.rs`（`**/src/**/*.rs` glob 未匹配），代码由主 agent 代落盘。委派 prompt 未预判 agent edit 权限边界（ref #203 三层兜底的外沿情况）。
3. **pre-commit fmt 拦截首次 commit**：i18n assert 合并 + types 断言展开未先 `cargo fmt`，hook 拦截后格式化重提（hook 正常工作，效率摩擦）。
4. **llvm-cov JSON segments 过滤条件两次用错**：`[3]==0` → `[2]==0` 才正确（格式 `[line, col, count, hasCount, ...]`，count 在 index 2）。
5. **F1 evidence 初始不完整**：先只落 task-5 + task-evidence，task-1/2 测试证据后补（task-1-2-tests.md）——违反"F-wave evidence 一次性写全"（ref #174/#181 教训相关）。
6. **review NITPICK 修复产生 2 个额外 commit**（unwrap→expect、裸 assert 加消息）：计划 4 commit，实际 6（含 evidence 补齐）。

**Lessons learned**:
1. **工具链报错先重跑/复现拿证据，再诊断**：llvm-cov double-spawn 类竞态错误，第一步是重跑验证是否一次性，命中再深挖——避免多轮静态排查（本次 4+ 轮 bash 排查后才重跑即过）。
2. **委派测试 agent 前预判其 edit 权限**：只允许 `tests/` 的 agent 会改变测试落点（集成测试 vs mod tests）；委派 prompt 显式声明"权限受限时落 tests/ 或交主 agent 落盘"，避免交付形态偏差与返工。
3. **F-wave evidence 一次性写全所有 task 证据**：task-1/2 测试证据与 task-5 验证证据同批落盘，不先写部分再补（本次后补暴露的完整性缺口）。

**Process improvements**:
- 已落实（docs）：`kb/dev/toolchain.md` 新增 llvm-cov double-spawn 排查卡（先重跑验证竞态）。
- 建议（可检测）：skwy-adversarial-test / skwy-requirement-test 的 agent edit 权限 glob 核查——`**/src/**/*.rs` 未匹配 `crates/*/src/lib.rs`，验证 agent 权限配置是否覆盖工作区全部目标文件（proposed，走 gate 建 issue 评估）。

### Trends (last 10)
- **F-wave evidence 完整性反复**（ref #181 F1 "9 commits" 过期声称 → 本次 task-1/2 证据后补）：evidence 必须实现收尾后一次性写全并自检（commit 计数、task 覆盖），中途写/部分写必然需要补正
- **测试 agent 权限/产出摩擦**（ref #203 权限设计 → 本次 edit 权限限制改变落点）：委派前明确 agent 权限边界与产出落点，权限受限时由 prompt 声明 fallback 路径，避免交付形态偏差
- **"先复现/重跑拿证据再深挖"模式**（ref #205 worktree 模拟 → 本次 llvm-cov 重跑即过）：工具链报错与可疑行为的第一动作是复现/重跑验证是否一次性，而非静态多轮排查

## 2026-08-12 — ref #244 llm-screener Batch 1：AST 类型系统 + 序列函数 + run_screener 兼容层

**What was done**: epic #243 Batch 1（#244）选股器表达式 AST 类型系统——compass-types 新增 Filter/MetaCond/SeriesFactor/SeriesCond/CmpOp/FactorRef 六组 enum（serde + and/or/negate + &|~ 运算符 + From<ScreenerQuery> 11 类编译映射）；compass-strategy 新增 up_days/count_in_window/volume_surge 序列函数 + run_screener(&Filter) 受限反向转换（8-shape accept-grammar + SeenFields 重复拒绝）+ ScreenerError::UnsupportedFilter。5 commits（44affdb 覆盖率配置 commit 因 master #251 supersede 在 rebase 中丢弃），966 测试全绿，compass-types 覆盖率 99.51%，5-way review 全 PASS。

**User corrections**: 无明确纠正——三项设计分叉（覆盖率 95%、C1 FactorRef、C3 三函数）均经 question 工具采纳推荐选项，plan 批准（"好"）与 review 请求（"review"）均为流程决策非纠偏。

**What went wrong**:
1. **Todo 5 worker 超时（30 分钟无活动）零交付**：委派 `run_screener` 大任务（签名变更 + 反向转换 + 21 测试迁移 + backend 迁移合一）后轮询超时，工作树仅残留 Todo 4 的 module 声明，核心工作全未开始。教训：超时后应先查工作树残留再重派，且大任务应拆分为 5A/5B 两个小任务（拆分后各 7m/1m 完成）——超时即任务过大信号。
2. **Todo 2/3 并行编辑同一文件冲突**：Todo 2（运算符）与 Todo 3（From impl）均改 screener.rs 并行执行，Todo 3 编辑覆盖了 Todo 2 的 flatten 实现，导致 `bitor_builds_or_chain` 运行时失败（Or([Or([a,b]),c]) 而非拍平）。教训：同文件改动不可并行委派——依赖矩阵需考虑文件级冲突而非仅类型级。
3. **Goal Verification lane 输出截断无 verdict**：review-work 中 oracle lane 分析被截断（仅 1 条消息无 <verdict>），首次 background_output 未见完整结论，需重跑才得 PASS。教训：review lane 无 terminal verdict 时应立即按 inconclusive 重派（ref #205 同模式再犯——"review lane 长时间无输出及早标记"）。
4. **F1 evidence 初始为 1 行 stub**：7 个 evidence 文件先写占位符，F1 审计标记"证据文件缺失实质内容"，后补实测数据。违反 ref #174/#181"evidence 一次性写全"（趋势第 3 次出现）。
5. **rebase 冲突处理依赖手动判断**：master #251 前进导致 4 文件冲突，识别为"被 supersede"后 `rebase --skip` 丢弃 44affdb——正确处理但需人工核对 master 是否已含等价变更。

**Lessons learned**:
1. **委派粒度与超时预案**：跨模块大任务（签名变更 + 测试迁移）拆分为核心/迁移两个小任务；worker 超时先 `git status` 查残留再决策重派，不盲目续会话。
2. **文件级依赖检查**：并行委派前用 `git diff` 确认任务不触碰同一文件；同文件任务强制串行（本次 Todo 2/3 同改 screener.rs 是冲突根因）。
3. **review lane 无 terminal verdict 即重派**：输出截断/仅 1 条消息/无 <verdict> 标签的 lane 立即标记 inconclusive 并重跑（允许读文件版本），不因"看起来快完成"等待。
4. **push 前 rebase 检查 base 前进**：fetch 后若 origin/master 前进且改动同域文件（覆盖率配置），先查 master 是否已 supersede 本分支变更再决定 skip/rebase。

**Process improvements**:
- 已落实（docs）：无 AGENTS.md 变更——委派粒度与 lane 重派规则已存在于 start-work/review-work skill 文档，本次为执行层未遵守。
- 建议（可检测）：并行委派前的文件级冲突预检——在 start-work 流程或委派 prompt 模板中强制"任务涉及文件清单 + 冲突矩阵"步骤（proposed，走 gate 建 issue 评估）。

### Trends (last 10)
- **F-wave evidence 完整性反复 3 次**（ref #181 过期声称 → ref #250 task-1/2 后补 → 本次 stub 占位）：evidence 必须实现收尾后一次性写全并自检，占位符/部分写/中途写都需要补正，已成为最高频流程摩擦
- **review lane 卡住/无输出处理不当 2 次**（ref #205 19 分钟未识别 → 本次 oracle 截断先等待后重派）：lane 无 terminal 状态（超时/截断/单消息）即标记 inconclusive 重派，不被动等待
- **并行委派同文件冲突**（本次 Todo 2/3 同改 screener.rs）：委派前做文件级冲突矩阵，同文件任务串行化
