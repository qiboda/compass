# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

**归档机制**：教训已融入流程（AGENTS.md 规则、skill 步骤、hook、回归测试、CI 门禁）
的条目不再具活性参考价值，归档至 `.dsh/kb/dev/reflections-archive.md`（历史可查）。
主文件仅保留仍具活性参考价值的条目（最近 + 教训未完全固化者）。

**自动归档（skwy-reflect 第 5 步，ref #238）**：本文件超过 500 行时自动归档一次
——值得处理的条目建 issue 后归档、已处理的直接归档、剩余的保留待下次归档时
重新检阅；归档后仍超 500 行则交用户判断。



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
- None（一次性/已落实：设计文档 `.dsh/designs/button-theme-and-width-fix.md` 已提交；.dsh/kb/design/ui.md L261 决策记录已修订为 ref #230 版本；ui-widgets.md Button 条目已同步。测试 helper 重复 → proposed 提取 `tests/common/mod.rs`）。

### Trends (last 10)
- **「先猜根因再验证」返工模式持续出现**（#139/#217 布局诊断、本次 #230 宽度观感）：本次因用户明确要求「查根本原因」而走了 kittest 断言先行，直接锁定根因（宽度真实跟随、遮罩观感）——验证「先复现拿证据再二分」有效，建议在 .dsh/kb/dev/process.md 调试章节固化该排查框架（proposed）。
- **ui-designer 设计委派流程已成标准路径**（#217 → 本次 #230）：design-first（产出 .omo/designs → 用户逐点确认 → 实现）两次均获认可，无偏差。
- **并行子代理测试重复**（本次 loading 文字色测试双写）：新出现模式——需在双测试 agent 委派时显式划分边界，观察后续是否再现。

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
- .dsh/kb/dev/testing.md 未更新——kittest 多匹配/locale 教训已在 toolchain 卡覆盖，待下次测试文档修订时并入（proposed）

### Trends (last 10)
- **子代理交付验证重复 2 次**（ref #244 worker 超时零交付 → ref #245 两次只分析未落盘）：委派 prompt 必须内置"交付前 git status 验证 + 未落盘视为失败"的硬性检查，纯"注意"不固化则每批再犯——本批已通过 prompt 显式要求缓解，但尚未固化为 skill/AGENTS.md 规则
- **kittest 测试时序/查询摩擦跨批重复**（ref #244/#245 均出现 locale 与 label 查询踩坑）：同一类"测试契约未显式化"反复——set_locale 与 query_all 应固化为项目测试基建（如 panel_with_form 自带 locale、helper 封装多匹配查询），而非每个测试手写易错
- **测试驱动发现生产 bug 是稳定收益**（ref #244 AST 形状 C2 修订 → ref #245 INFINITY 布局）：RED 测试在实现前暴露契约/布局缺陷——test-first 的价值已两次实证，继续保持

## 2026-08-13 — ref #255 epic index-data：指数采集/导出/BK 符号/大盘 tab

**What was done**: 完成 epic #255 全链路——东财指数采集器（官方 30 白名单 + 概念/行业板块 clist + push2his kline + last_report_date 增量）+ Dolt index_daily/index_basic 双表 + import-compass 导出（index_daily 增量 merge / index_basic 全量覆盖）+ BK 前缀符号体系（6 消费点扩展）+ GUI 大盘 tab（6 白名单 Card + 板块轮动列表 + 双 parquet 路由 + 第四快照通道）。4 子 issue（#256-259）+ review-fix 共 5 commits，全部测试绿（Rust 全 workspace + Python 442 passed 95.74%）。

**User corrections**: 用户仅发三次指令——"开始"（执行 handoff 流程）、"按推荐"（批准 grill-me 推荐）、"完成后自动 push 合并 PR 关闭 worktree"（收尾授权）。全程批准推荐，无纠正。

**What went wrong**:
1. **C1 实现子代理输出截断、零落盘**（13:32→15:12 浪费约 40 分钟）：bg_b12ab50f 首次委派在分析阶段被截断，fetch_index_daily.py 未创建、main.py 未改——必须 task_id 续会话重做才落地。与 ref #244/#245 trends 的"子代理交付验证"模式**第三次重复**（worker 超时零交付 → 两次只分析未落盘 → 本次分析截断零落盘）。
2. **pre-commit hook 多轮拒绝**（C1 commit 时）：ruff SIM105（try-except-pass ×2）+ SIM117（嵌套 with ×2）共 4 处修复才通过。hook 规则明确但实现 agent 未预检 ruff。
3. **FIX-3 抽样核对测试数据设计错误**：3001 vs 2990 差 0.37% 在 0.5% 容差内——测试断言"必须报警"但数据未超容差，多轮调试后定位是测试数据问题而非实现缺陷。
4. **FIX-4 真实数据冒烟被网络阻塞**：东财 push2his 全部 host（主域 + 91./79./17./7./80./29. 镜像）HTTP 000 不可达，真实采集无法执行；仅 quote.eastmoney.com 首页可达。按问题闭环记录根因（环境网络策略），降级为 tempdir 真实形态数据验证管线，真实采集待网络恢复。
5. **review-work 5-lane 的 Goal/Security FAIL 暴露的缺口**：T8 文档同步缺失（.dsh/kb/ 零更新）、决策 6 抽样核对未实现、--since 注入校验缺失——均为实现 agent 的 scope 遗漏，review 独立发现（体现 review 价值）。

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

## 2026-08-15 — ref #264 kb/ 迁移到 .dsh/kb/ + OpenCode 工作流语义重写为 DSH 版

**What was done**: kb/（24 文件）git mv 至 .dsh/kb/，全仓 91 处 kb/ 引用替换；opencode-ci-fix.yml 改名 ci-fix.yml、opencode.yml（GitHub 评论 bot）删除；AGENTS.md（15 处）+ process.md 的 OpenCode 机制重写为 DSH 语义（skill 工具、subagent_ui_designer / subagent_skwy_requirement_test / subagent_skwy_adversarial_test、subagent_review、GitHub MCP、plan mode、/home/skwy/.dsh/skills/ 路径、~/.dsh/.agent-presets/ 模型规则）；toolchain.md / workflow-skills.md 头部注记。3 实现 commit（4148c0d/6017ed8/935c5c2）+ 本反思 commit；subagent_review 独立审查（无 P0，P1×1/P2×1/P3×1 已修复）。

**User corrections**:
1. "好的，另外kb目录也移到.dsh里面去。" —— 范围扩展：kb/ 知识库整体迁入 .dsh/
2. "按推荐，使用精确工具名。然后 /ulw-plan agent 这个使用 dsh的 plan 命令" —— 指定 /ulw-plan 映射为 DSH plan 命令（plan mode）
3. "opencode-ci-fix.yml 迁移，里面并没有使用opencode。 另一个删除。" —— 纠正我推荐"两个 workflow 都删"：ci-fix 实际不含 opencode 依赖应改名迁移，仅 opencode.yml 删除
4. "push" —— 授权 push

**What went wrong**:
1. **漏改 .dsh/skills/product/SKILL.md 的 .omo/plans/**（review P1 抓出）：#263 的 sed 替换文件列表用 `grep -rl` 生成，路径参数漏了当时还在 .opencode/skills/ 的技能文件（迁移后 .dsh/skills/），该文件两处 .omo/plans/ 残留——替换规则存在但文件没进替换列表。这是"引用范围侦察不完整"系列（#117→#263→#264）的第三次变体。
2. **workflow 改名不彻底**（review P2 抓出）：git mv opencode-ci-fix.yml→ci-fix.yml 只改文件名，未检查文件内 `name:` 字段残留 opencode-ci-fix。
3. **决策判断依赖文件名而非内容**：推荐删除 opencode-ci-fix.yml 时只看名字（含 opencode），用户纠正"里面并没有使用opencode"——文件内容审查应先于命名推断。

**Lessons learned**:
1. 迁移/替换的 grep 路径范围必须覆盖全仓所有目录（含被迁移目录、技能目录、隐藏目录），不预设排除；替换后对全部已编辑文件再跑一次残留扫描——已补入 process.md 全仓 grep 规则（ref #264）。
2. 重命名文件后必须检查文件内容中的自身名字引用（workflow `name:`、package name 等）。
3. 删除/迁移决策前先读文件内容判断实际依赖，不凭文件名推断。

**Process improvements**:
- process.md「知识库同步」全仓 grep 规则补充两条：① grep 路径范围覆盖全仓所有目录（含隐藏目录与被迁移目录本身），不预设排除；② 替换完成后对全部已编辑文件再跑一次全量残留扫描（ref #264 落实）
- 反思文件归档执行：11 条已处理/过时条目归档至 reflections-archive.md（181/203/208/238/234-240/46-132-71/250/135/246/247/263），保留 5 条含 proposed 条目（230/235-213/244/245/255），行级校验通过

### Trends (last 10)
- **引用范围侦察缺陷第三次出现**（ref #117 archive → #263 只搜 md → #264 grep 路径漏技能目录）：前两次教训均已固化但执行变体仍再现——本次补充"路径范围覆盖全仓（含被迁移目录）"到 process.md 规则；独立 subagent_review 成功抓出主 agent 验证盲区（P1 漏改），审查-修复闭环价值持续实证（#255 → #264）
- **文件名推断 vs 内容事实**（#264 ci-fix.yml）：文件名含 opencode 但内容无依赖，用户纠正删除决策——文件操作类决策（删除/迁移/改名）必须先读内容再判断

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

## 2026-08-15 — ref #276 result-slot log/loading race fix

**What was done**: Fixed a CI race in `wire_backend` where fetch/screener/SEPA/index result slots cleared `*_loading` before writing the display log; moved log writes before `*_loading.set(false)` and added four deterministic regression tests that hold the loading `Dynamic::lock()` and wait for `log_count() > 0`. Docs updated in `testing.md` and `architecture.md`.

**User corrections**: None. (User decisions: 修复 / 统一修 / 按推荐 / 好 / 开始 / push.)

**What went wrong**:
1. `cargo test -p compass --lib ...` failed with "no library targets found in package `compass`" — compass is a binary-only crate; used `--bin compass` instead.
2. Initial four regression tests from the requirement-test subagent waited for intermediate result data (bars/screener_result/sepa_data/index_snapshot) before asserting `log_count > 0`; on fixed code the fetch test still flaked because data is written before the log. Refined the tests to poll the invariant itself (`log_count > 0`) while holding the loading lock.
3. Multiple `edit` attempts failed with "file changed since it was read" after subagents/cargo fmt/commit modified the file; required re-read and retry.

**Lessons learned**:
1. Check crate targets (`Cargo.toml`) before running tests: binary-only crates need `cargo test -p <pkg> --bin <bin>`, not `--lib`.
2. For ordering invariants tested by parking a worker on a mutex, poll the observable invariant itself (e.g. `log_count > 0`) rather than an intermediate state then asserting; the intermediate state can be visible before the invariant is established even in fixed code.
3. Use `read` immediately before a batch of `edit` calls on files that subagents or formatters may have touched; on "file changed since it was read", re-read and retry instead of guessing.

**Process improvements**:
- `.dsh/kb/dev/testing.md` already documents the deterministic mutex-park pattern for result-slot ordering (committed with the fix).
- `.dsh/kb/design/architecture.md` now shows log-before-loading in the result_slot diagram (committed with the fix).
- No new hook/CI proposed for this batch.

### Trends (last 10)
- `edit` "file changed since it was read" friction recurs across #273/#266 and this #276; batch edits after subagent/cargo fmt should start with a fresh `read` of the target file.
- Subagent-delivered tests requiring main-agent refinement recurred (this session's result-data wait → log-invariant wait); main agent should validate RED/GREEN determinism against both old and new code before accepting test delivery.
- Binary-only crate test command confusion is a new small pattern; consider documenting `--bin` usage in `testing.md` if it recurs.
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
