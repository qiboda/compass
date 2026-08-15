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
