# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

**归档机制**：教训已融入流程（AGENTS.md 规则、skill 步骤、hook、回归测试、CI 门禁）
的条目不再具活性参考价值，归档至 `kb/dev/reflections-archive.md`（历史可查）。
主文件仅保留仍具活性参考价值的条目（最近 + 教训未完全固化者）。

**自动归档（skwy-reflect 第 5 步，ref #238）**：本文件超过 500 行时自动归档一次
——值得处理的条目建 issue 后归档、已处理的直接归档、剩余的保留待下次归档时
重新检阅；归档后仍超 500 行则交用户判断。



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

## 2026-08-12 — ref #135 collectors 财报增量修订检测：UPDATE_DATE 锚点增量 + UPSERT 覆盖

**What was done**: 实现 issue #135——fetch_fin_indicators.py 增量模式从 REPORTDATE 枚举改为 UPDATE_DATE 时间锚点（filter=(UPDATE_DATE>='{anchor}')，锚点=min(data_updates.last_updated, state.json last_update_date)），Dolt import 改 UPSERT（SELECT 别名 + ODKU 无前缀别名引用，Dolt 2.2.3 不支持限定引用/VALUES()），CSV 整文件 keep-LAST 去重，替代 #27 --refresh N。7 commits（test RED ×2 + feat ×2 + docs ×2 + fix ×1），406 tests 全绿、cov 96%、5-way review 全 PASS，双审 5 轮批准。

**User corrections**（逐字引用对话记录）:
1. "在更新表中记录的有更新日期，根据更新日期自动增量不行吗？" —— 用户点出按 UPDATE_DATE 自动增量优于我最初的 `--revision-window` 窗口重抓方案。实测确认东财 API 支持 `filter=(UPDATE_DATE>='...')`，方案大幅简化。
2. "默认的更新日期使用compass_data数据库中记录的上次日期可以吗？你知道是哪个表哪一列吗" —— 用户指定锚点用 `data_updates.last_updated`（并考察我是否真知道表列）；我澄清了 `last_report_date` 是报告期语义不能作 UPDATE_DATE 锚点的陷阱。
3. "按推荐" —— 确认锚点列 data_updates.last_updated。
4. "review B" —— C4 决策选 B（不做过渡回补）+ 要求 high-accuracy review。

**What went wrong**:
1. **plan 首版方案方向偏差**：v1 方案是 `--revision-window N` 窗口重抓 + UPDATE_DATE 对比——用户一句话点破"按更新日期自动增量"后才实测 API filter 能力，方案从"窗口重抓"简化为"锚点过滤"。探索阶段应更早实测 API filter 能力，而非先设计重抓窗口。
2. **双审 5 轮 CHANGES_REQUESTED**：plan 经历 momus+Oracle 5 轮审查才通过——先后发现 T5 过滤表达式漏类、T6 0 行推进歧义、F3 五粮液断言行空转、UPSERT 限定引用不可行、wave 摘要残留旧描述、T1/T2 标题行粘连等。plan 初版质量不足，多轮返工。
3. **UPSERT 写法反复实测**：最初信 Metis 的"限定源列引用可用"，实测后才发现 Dolt 2.2.3 只支持 SELECT 别名写法（限定引用对 TRIM 文本列报错、VALUES() 报 __new_ins）。"subagent 输出需独立验证"原则执行正确但耗费多轮。
4. **测试 agent 权限受限**：skwy-adversarial-test / skwy-requirement-test 无法写 `.omo/evidence/`（edit 白名单仅 tests/**），RED 证据需主 agent 代落盘（ref #250 同教训再犯）。
5. **覆盖率差 0.69pp**：首轮全量 cov 94.31% < 95% 门槛，补 11 个覆盖测试（fetch_fin_indicators.py 91%→100%）才达标——T9 应在实现时就规划覆盖补测而非验证阶段才补。
6. **security review 发现 429 陈旧 data 复用**：fetch_by_update_date 的 `data` 在页循环外初始化，某页 429 耗尽后残留上一页响应导致重复追加。修复为每页重置（一个 commit）。
7. **review 后 rebase master**：master 在开发期间前进 8 commits，rebase 无冲突但需重跑核心测试确认。

**Lessons learned**:
1. **先实测数据源能力，再定方案形态**：涉及外部 API（东财 datacenter）时，第一步应实测其 filter/sort 能力（curl 验证 UPDATE_DATE 过滤），方案方向随实测结果调整——用户的一句话往往点出更简路径，探索要覆盖"数据源本身支持什么"而非假设局限。
2. **plan 双审轮次可前置压缩**：plan 写入前的 Metis 差距分析 + 独立验证（UPSERT 写法、路径存在性）应更彻底，减少 momus/oracle 轮的 CHANGES_REQUESTED 往返（本次 5 轮）。
3. **委派测试 agent 时声明 evidence 落盘 fallback**：只允许写 tests/** 的 agent，RED 证据由 prompt 明确"输出完整记录，主 agent 代落盘"，避免交付时才发现权限边界（ref #250 同教训，本次再犯——需固化为流程）。
4. **覆盖率门槛在实现 wave 内规划**：新增代码的覆盖缺口（错误分支、fallback 路径）应在 GREEN 实现时就补测试，而非 T9 验证阶段发现 94.31% 再补。

**Process improvements**:
- 已落实（docs）：无 AGENTS.md 变更。
- 建议（可检测）：委派测试 agent 的 prompt 模板增加"evidence 落盘权限说明"——测试 agent 只能写 tests/** 时，prompt 明确"RED 证据以完整记录输出，主 agent 代写入 .omo/evidence/"（proposed，ref #250 已提类似项，本次再犯需固化）。
- 建议（可检测）：探索阶段先验证数据源能力——涉及外部 API 的 feature，plan 探索 checklist 增加"实测 API filter/sort 支持能力"项（proposed）。

### Trends (last 10)
- **测试 agent 权限/产出摩擦 3 次**（ref #203 权限设计 → ref #250 edit 权限限制 → 本次 evidence 无法落盘）：委派 prompt 必须预判 agent 权限边界并声明 fallback（evidence 主 agent 代落盘、代码主 agent 代写入），仅"注意"不固化则每次再犯
- **外部依赖能力未先实测导致方案返工**（本次 UPDATE_DATE filter 未在 v1 探索、UPSERT 写法多轮实测）：涉及外部 API/数据库方言时，第一步实测能力边界（curl/filter 验证、方言兼容性探针），再定方案形态
- **plan 双审轮次偏高**（本次 5 轮 momus/oracle）：Metis 差距分析 + 关键 claim 独立验证前置化可压缩轮次——验证过的写法/路径/过滤直接写入 plan，避免每轮评审重复发现同类问题

## 2026-08-13 — ref #245 llm-screener Batch 2：可视化条件构建器 UI（条件卡片组操作 Filter AST）

**What was done**: epic #243 Batch 2（#245）用 Metabase 范式条件卡片组（AND/OR 嵌套 ≥2 层）替换选股器固定表单——compass 新增 `screener_builder.rs` 视图模型（CondItem/CondGroup/CondLeaf + filter_to_items/leaf_to_filter/group_to_filter 双向映射，round-trip 结构等价）；UI 渲染（根组 Card + 递归子组 Frame + 添加/删除/AND-OR/清空/取反/空态）；`RunScreenerRequest` 携 Filter；legacy 保存复用引擎 `filter_to_query` 作压缩 oracle；i18n `screener.builder.*` 键。7 commits 全含独立成行 `ref #245`，1087 测试全绿、覆盖率全达标（compass 92.2% ≥ 90%），momus+oracle 双审修订计划后执行。

**User corrections**: 无明确纠正——"好"（批准计划）、"review"（要求双审）、"开始"（批准执行）、"push"（确认推送）均为流程推进决策非纠偏。6 项设计分叉（预置 6 卡/允许构建运行报错/提供 Not/Count 延后/legacy 保存/就地编辑）经 question 工具全部采纳推荐。

**What went wrong**:
1. **子代理两次"只分析未落盘"（Todo 3 unspecified-high、Todo 5 quick）**：两个 agent 都输出了完整设计与代码思路但未写文件（git status 干净），各浪费一次完整委派周期（5m + 21m）。原因：委派 prompt 强调"分析/设计"多于"必须落盘"，且未强制交付前 git status 验证。教训：#244 同类（worker 超时零交付）已出现——子代理交付验证必须前置为 prompt 硬性要求。
2. **kittest label 多匹配踩坑 3 次**（en_builder_technical "MA"、show_renders "排除退市"/"全部"）：`get_by_label_contains` 遇多匹配直接 panic，需 `query_all_by_label_contains` + 下标。每次都是运行失败后才发现，浪费 3 轮测试-修复循环。
3. **自建 panel 测试未显式 set_locale 单跑失败**（Todo 4 两个 filter_click 测试）：绕过 `panel_with_form()` 自建 panel 时未设 `rust_i18n::set_locale("zh")`，单跑默认 en locale 下 `get_by_label("筛选")` 找不到按钮；完整跑时被其他测试的 zh 遗留掩盖。多轮调试才定位（"测试隔离问题"）。教训：#244 同批已强调"默认 zh 测试契约要显式 set_locale"（toolchain 卡），此处重复——自建 panel 的测试构造必须显式 set_locale。
4. **unsupported_save 提示被 `screener_error.set(None)` 顺序覆盖**：Todo 4 初版先压缩失败设提示、后无条件清 error，导致提示消失。真实逻辑缺陷（非测试问题），调试 4-5 轮定位（含探索 toast 机制、backend result_slot 语义）。教训：交互路径中"设置状态 A → 清状态 B"的顺序依赖应在实现时一次想清，测试驱动发现成本高。
5. **测试 5 子组内二次 popup 点击死磕后确认 kittest 限制**：为驱动"子组内加第二张卡"交互投入 ~15 轮调试（多 step、rect 有限断言、click 时序、clicked_outside 分析、kittest click_at 探索），最终确认 kittest 无法点击嵌套 scope 内 popup 的第二次选项（Area hover 时序），降级为视图模型加卡 + 保留建组真实交互。教训：遇到"交互测试驱动不了"应先查 toolchain 排查卡同类限制（multi_select Area/ScrollArea 限制已记录），避免长时间死磕。
6. **测试驱动的生产 bug：子组 scope max_rect 高度 INFINITY → 组内/组后控件 rect NaN**：交互测试发现生产布局缺陷（INFINITY 使 wrap 垂直居中算 NaN），修复为有限底部并沉淀 toolchain 卡。此为正面收获（测试发现生产 bug），非摩擦——记录以示测试驱动的价值。

**Lessons learned**:
1. 委派 prompt 必须写"交付前 git status 确认改动落盘"+"未落盘视为失败"（子代理两次只分析未写入）。
2. 所有 kittest label 查询默认 `query_all_by_label_contains` + 下标，`get_by_label_contains` 仅在已知唯一时用——多匹配 panic 是默认陷阱。
3. 自建 panel/harness 的测试构造器必须自带 `set_locale("zh")`（不依赖 panel_with_form 的副作用）。
4. 交互路径状态设置顺序（set A → clear B）实现时先画时序，测试驱动定位成本高。
5. 交互测试驱动失败先查 toolchain 排查卡同类限制，再决定死磕或降级（kittest popup 时序限制已有先例）。

**Process improvements**:
- toolchain.md 新增 2 张排查卡：`子分组 scope INFINITY 布局`（已修复 + 遗留 kittest 二次 popup 限制）、kittest NaN/inf rect 点击静默丢弃 + popup 单帧 step 时序教训
- kb/dev/testing.md 未更新——kittest 多匹配/locale 教训已在 toolchain 卡覆盖，待下次测试文档修订时并入（proposed）

### Trends (last 10)
- **子代理交付验证重复 2 次**（ref #244 worker 超时零交付 → ref #245 两次只分析未落盘）：委派 prompt 必须内置"交付前 git status 验证 + 未落盘视为失败"的硬性检查，纯"注意"不固化则每批再犯——本批已通过 prompt 显式要求缓解，但尚未固化为 skill/AGENTS.md 规则
- **kittest 测试时序/查询摩擦跨批重复**（ref #244/#245 均出现 locale 与 label 查询踩坑）：同一类"测试契约未显式化"反复——set_locale 与 query_all 应固化为项目测试基建（如 panel_with_form 自带 locale、helper 封装多匹配查询），而非每个测试手写易错
- **测试驱动发现生产 bug 是稳定收益**（ref #244 AST 形状 C2 修订 → ref #245 INFINITY 布局）：RED 测试在实现前暴露契约/布局缺陷——test-first 的价值已两次实证，继续保持

## 2026-08-13 — ref #246 llm-screener Batch 3：通用 Filter AST 求值器 + 序列条件 + 持久化

**What was done**: epic #243 Batch 3（#246）把选股器从"受限反向转换 filter_to_query + 硬编码 screen_symbol"升级为通用递归求值器 `screener_eval.rs`（evaluate 递归：Meta/Series/And/Or/Not，UpDays/Count/VolumeSurge 真实过滤，NDayHigh 前 N 根不含最新等语义复刻），删除 filter_to_query 全套机制 + UnsupportedFilter；持久化双格式（[screener] filter JSON key + legacy 11 键兼容读取 + 坏 JSON 回退）；GUI on_save/restore 迁移 &Filter、oracle/i18n 清理；criterion bench 性能对比（同量级，-0.25%/-2.84%）。5 commits 全含独立成行 `ref #246`，34 个门禁 RED 测试（9 需求 + 25 对抗）转 GREEN，21 语义测试断言不变，覆盖率 strategy 97.48% ≥ 95%，真实数据冒烟（6126 标的）全部形状验证正确，5-way review 全 PASS。

**User corrections**: 无明确纠偏——"开始"（启动 worktree 工作）、"批准，并跑高精度审查"（要求双审）、"开始"（批准 plan 执行）、"完成后自动push，合并pr，并关闭worktree"（授权收尾）均为流程推进决策。1 项分叉（是否跑高精度审查）经 question 工具用户选择跑双审（非推荐项），采纳执行。

**What went wrong**:
1. **task() 委派首调缺 subagent_type 参数失败 2 次**：门禁 3.5/4 步委派 skwy-adversarial-test / skwy-requirement-test 时第一次调用漏传 `subagent_type` 被拒（missing_category_or_agent），重试补齐后成功。教训：使用 task() 委派专用 agent 时 subagent_type 是必填，与 category 二选一——调用前核对参数。
2. **bench 数据规模与 criterion 默认采样冲突**：screener_eval bench 首跑 6000×400 在默认 criterion 采样（100 samples）下编译+运行超时（900s 截断）。教训：数据密集型 bench 首次验证先用 `--sample-size 10 --warm-up-time 1 --measurement-time 3` 快速确认可跑，再决定是否全量。
3. **filter JSON 写入 TOML 转义踩坑 2 次**：持久化测试手写 `filter = "{json}"` 未转义 JSON 内引号导致解析失败；改用 `toml::to_string(&toml::Value::String)` 又遇 toml 0.8 不支持顶层 String 值（UnsupportedType）。最终手工 `replace('"', "\\\"")` 转义。教训：JSON 字符串嵌入 TOML basic string 必须转义内层引号，且 toml crate 的 to_string 不支持顶层裸 String——先查 toml 序列化能力再写。
4. **NDayHigh 在 Count 内的语义先写错后修正**：初版把 Count 内 NDayHigh 写成"含当日"，与顶层 Cmp 的"前 N 根不含最新"不一致，Oracle 审查前自我修正为统一 `series[i-n..i]`。教训：factor 语义在求值器内必须单一来源（顶层与逐日求值共用 factor_at 定义），避免两处漂移。
5. **Review 5-agent 中 2 个 oracle lane 各耗时 30m/8m**：goal 验证 lane 运行 30m53s（大量读文件+跑测试），与 QA lane 部分工作重叠（都跑了 workspace 测试）。教训：oracle lane 的 prompt 应更聚焦（给足 file_contents 减少读文件），避免与 QA lane 重复执行重型验证。

**Lessons learned**:
1. task() 委派专用 agent（skwy-*）必须显式传 subagent_type；category/subagent_type 二选一必填。
2. criterion bench 首次验证用缩减采样参数（--sample-size 10 --warm-up-time 1 --measurement-time 3）快速冒烟，全量采样留给正式记录。
3. JSON 嵌入 TOML 字符串必须手工转义内层引号（`\` → `\\`，`"` → `\"`）；toml crate 顶层 String 序列化不可依赖（UnsupportedType）。
4. 递归求值器内 factor 语义单一来源（顶层 Cmp 与 Count 逐日求值共用同一 factor_at），任何"上下文相关"的语义差异须显式注明并测试钉住。
5. 多 agent review 时给 oracle lane 足量 file_contents 内联（减少其读文件耗时），并把重型验证（workspace 测试/冒烟）收敛到 QA lane 单点执行，避免重复。

**Process improvements**:
- 无 AGENTS.md/hook/skill 变更——4 条教训（task 参数、bench 采样、TOML 转义、factor 单一来源）均为一次性技术摩擦，已沉淀本条目；toolchain 排查卡可考虑补"toml 内嵌 JSON 转义"卡（proposed，待下次遇到同类序列化问题时固化）

### Trends (last 10)
- **子代理交付验证**（ref #244/#245 零交付/只分析 → 本批 3.5/4 步测试 agent 均正常落盘）：本批委派 prompt 显式要求"写文件 + 跑验证 + 报告"，效果良好——交付验证前置为 prompt 硬性要求已见效，趋势缓解
- **测试驱动价值持续实证**（ref #244 C2 → #245 INFINITY → 本批 34 RED→GREEN + 真实数据冒烟）：test-first 连续三批捕获契约/语义问题，继续维持
- **epic 批次间编译级联动**（#244 run_screener 签名 → #245 GUI 调用点 → #246 filter_to_query 删除破坏 screener.rs:208）：跨 crate 删除公共函数必须同步扫描全部调用方（grep 前置），本批因 filter_to_query 删除与 GUI oracle 的编译耦合被迫联动 Todo 2/4——plan 依赖矩阵应预判跨 crate 删除的联动

## 2026-08-13 — ref #255 epic index-data：指数采集/导出/BK 符号/大盘 tab

**What was done**: 完成 epic #255 全链路——东财指数采集器（官方 30 白名单 + 概念/行业板块 clist + push2his kline + last_report_date 增量）+ Dolt index_daily/index_basic 双表 + import-compass 导出（index_daily 增量 merge / index_basic 全量覆盖）+ BK 前缀符号体系（6 消费点扩展）+ GUI 大盘 tab（6 白名单 Card + 板块轮动列表 + 双 parquet 路由 + 第四快照通道）。4 子 issue（#256-259）+ review-fix 共 5 commits，全部测试绿（Rust 全 workspace + Python 442 passed 95.74%）。

**User corrections**: 用户仅发三次指令——"开始"（执行 handoff 流程）、"按推荐"（批准 grill-me 推荐）、"完成后自动 push 合并 PR 关闭 worktree"（收尾授权）。全程批准推荐，无纠正。

**What went wrong**:
1. **C1 实现子代理输出截断、零落盘**（13:32→15:12 浪费约 40 分钟）：bg_b12ab50f 首次委派在分析阶段被截断，fetch_index_daily.py 未创建、main.py 未改——必须 task_id 续会话重做才落地。与 ref #244/#245 trends 的"子代理交付验证"模式**第三次重复**（worker 超时零交付 → 两次只分析未落盘 → 本次分析截断零落盘）。
2. **pre-commit hook 多轮拒绝**（C1 commit 时）：ruff SIM105（try-except-pass ×2）+ SIM117（嵌套 with ×2）共 4 处修复才通过。hook 规则明确但实现 agent 未预检 ruff。
3. **FIX-3 抽样核对测试数据设计错误**：3001 vs 2990 差 0.37% 在 0.5% 容差内——测试断言"必须报警"但数据未超容差，多轮调试后定位是测试数据问题而非实现缺陷。
4. **FIX-4 真实数据冒烟被网络阻塞**：东财 push2his 全部 host（主域 + 91./79./17./7./80./29. 镜像）HTTP 000 不可达，真实采集无法执行；仅 quote.eastmoney.com 首页可达。按问题闭环记录根因（环境网络策略），降级为 tempdir 真实形态数据验证管线，真实采集待网络恢复。
5. **review-work 5-lane 的 Goal/Security FAIL 暴露的缺口**：T8 文档同步缺失（kb/ 零更新）、决策 6 抽样核对未实现、--since 注入校验缺失——均为实现 agent 的 scope 遗漏，review 独立发现（体现 review 价值）。

**Lessons learned**:
1. **子代理委派必须内置"落盘验证"硬性检查**：prompt 加"交付前 git status 确认改动落盘，未落盘视为失败并立即报告"——本次虽已写但 C1 agent 仍截断零落盘，说明对**后台长任务**还需加"完成后主 agent 必须 git status 核验落盘"（本批主 agent 已核验，但应固化为流程）。
2. **委派实现 agent 前预跑静态检查**：提交前对 Python 改动跑 `ruff check`、Rust 跑 `cargo fmt --check`——hook 拒绝是既知摩擦，agent 应预检而非等 hook 弹回。
3. **测试数据必须与断言同量级**：容差类断言（0.5%）的测试数据要设计成明显超差（>2%），不能贴着阈值（0.37% 看似该报警实则不超）。
4. **网络依赖的真实冒烟需前置探测**：FIX-4 若在实现前先 `curl -s -o /dev/null -w "%{http_code}" push2his` 探测网络，可提前规划"网络不可达 → 直接安排 tempdir 冒烟"而非实现后才发现。

**Process improvements**:
- AGENTS.md/委派惯例：本批主 agent 已对后台任务执行"完成后 git status 核验落盘"（C1 截断即由此发现）——建议固化为 skwy-workflow 的委派后核验步骤（proposed，待建 issue）
- toolchain.md 新增排查卡：`push2his 行情 API 网络不可达（HTTP 000，仅 quote 首页可通）→ 真实数据冒烟前置探测`（proposed）
- 反思条目第 3/4 条为一次性教训，无法固化为机制（None）

### Trends (last 10)
- **子代理交付验证第四次出现**（ref #244 超时零交付 → #245 两次只分析 → #255 C1 截断零落盘）："委派后核验落盘"至今是主 agent 手动行为，未固化为 skill/hook——本次已通过主 agent 核验补救，建议正式写入 skwy-workflow 委派协议（proposed）
- **hook 静态检查预检缺失跨批重复**（#245 pre-commit 拒绝 → #255 ruff SIM 4 处）：实现 agent 提交前不预检 lint/fmt，靠 hook 弹回——建议委派 prompt 内置"提交前 ruff/fmt 预检"条款
- **review-work 独立发现 scope 遗漏是稳定收益**（#255 Goal/Security FAIL 暴露 T8 文档缺失 + 决策 6 未实现 + 注入校验缺失）：独立审查 agent 发现主 agent 盲区——5-lane review 价值持续实证，保持强制
## 2026-08-14 — ref #247 llm-screener Batch 4：内嵌 LLM 生成选股 Filter AST（epic #243 收尾）

**What was done**: epic #243 最终批次——LLM 客户端（compass-core::llm，OpenAI 兼容 chat completions）、validate_filter 语义校验（compass-types）、prompt/parse 业务层（compass::llm_screener）、[llm] config 节、backend 第 5 AsyncDispatcher 通道（seq 守卫）、ScreenerPanel 自然语言输入区 + i18n + docs。4 commit + 2 fix commit，36 测试套件全绿，coverage 全达标（core 96.5%/types 99.4%/compass 90.5%）。

**User corrections**: 无（用户仅"开始"+"完成后自动 push 并合并 PR 关闭 worktree，有问题自行解决"——全程自主推进）。

**What went wrong**:
1. **设计偏离后中途改判**：实现前裁决"消息无 seq、不做 Esc 取消（轻量原则）"，与用户确认的设计文件 `.omo/designs/llm-screener-llm.md` §3/§5（seq 守卫 + Esc 取消）冲突——直到实现 Todo 5 才细读设计文件发现，改判为按设计实现。根因：plan 摘要未含 seq 细节，实现前未完整读设计文件契约。
2. **review 抓出 4 个 blocking（契约落实缺口）**：① AC3 模板外形状（Count/单边 Cmp）静默丢失——Unknown 卡在 `leaf_to_filter` 被转 `And(vec![])`，与设计"可随筛选发送"承诺矛盾；② llm_error→Error toast 未实现（设计 §5 双通道，只做了内联）；③ 后端 LLM 通道零测试（plan Todo 5 验收"backend 测试新增 roundtrip/未配置/5xx"未落实）；④ llm_merge_into_root 与 seq 守卫 drop 路径零测试（设计 §7 测试锚点）。全部是"plan/设计声明的验收标准在实现阶段未逐条核对"，靠 5-agent review 才暴露，返工 2 轮。
3. **测试契约冲突**：requirement-test agent 按 plan（无 seq）写测试并明确标注 plan vs 设计文件冲突待裁决；我裁决"以设计为准（带 seq）"后，其代落盘的 backend 测试需调整——契约冲突未在实现前统一裁决。
4. **sed 按行号批量修改多次失效**：edit 插入行后行号 +1 偏移，后续 sed 用旧行号未命中；多次 grep 重定位重跑（效率摩擦）。

**Lessons learned**:
1. 实现前必须完整读用户确认的设计文件（`.omo/designs/*.md`）的契约细节（消息字段/交互/测试锚点），不能只看 plan 摘要——设计文件是权威，plan 是执行摘要，两者冲突时以设计为准且需记录裁决。
2. 宣称"plan 完成"前逐条核对 plan 的 Todo acceptance criteria 与设计 §7 测试锚点（本项目 review 是门禁，但自查在先可省 2 轮返工）——特别是"测试新增"类验收（如 Todo 5 的 backend roundtrip 测试）必须在实现 commit 中落地，不能只靠 review 抓。
3. sed 按行号修改后必须 grep 重验命中（行号偏移是常态）；批量调用点修改优先用模式匹配（replaceAll）而非行号。

**Process improvements**: None（一次性教训——plan 的 Todo acceptance 已写明"实现+测试=ONE todo"、设计文件路径已在 plan 引用；本轮为执行未落实，非流程缺失）。

### Trends (last 10)
- Batch 2/3/4（#245/#246/#247）均为"plan 声明的验收标准 → 实现 → review 验证"模式，仅本批出现契约缺口返工——前两批的 review 未抓出同类问题，本批 4 个 blocking 集中在"设计承诺 vs 实现行为"差异（Unknown 卡丢弃/toast 缺失），提示实现后自查应比对设计文件的用户可见承诺（gui.md"与手动卡片完全同构"等措辞）。
- 无其他显著重复模式。
