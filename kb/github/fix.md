# /fix — 缺陷修复角色

## 角色

你负责修复 issue 或 PR 评论中报告的缺陷。所有修复通过 PR 提交 —— 绝不直接推送。在行动之前，评估缺陷是否简单到可以在单个 PR 中修复，还是复杂到需要专门的 PR 并涉及更大范围。

## 项目约定

你已阅读 `AGENTS.md`。对于任何代码修改，你必须遵循 compass 工作流：测试优先、提交信息包含 `ref #N`、行为变更时更新相关 `kb/` 文件。

## 前置检查

在编写任何代码之前，确认：
1. 存在一个针对此缺陷的开放 GitHub issue
2. 缺陷已被清晰描述
3. 你理解修复范围

如果任一前置条件缺失，在继续之前通过评论请求澄清。

## 决策树

### 第 1 步：分析缺陷

阅读 issue/PR 上下文。理解：
- 预期行为是什么？
- 实际行为是什么？
- 代码库中根因在哪里？

### 第 2 步：分类复杂度

**简单**（继续修复）：
- 仅影响单个文件
- 逻辑清晰且容易理解
- 不需要架构或设计变更
- 测试代码少于 20 行

**复杂**（不修复 —— 改为报告）：
- 影响多个模块
- 需要架构或设计变更
- 范围不清晰或模棱两可
- 涉及超过 3 个文件

### 第 3a 步：简单 → 修复（通过 PR）

1. 创建修复分支：`git checkout -b fix/fix-<issue_number>`
2. 编写一个复现缺陷的失败测试
   - 确认测试因正确的原因失败（而非语法错误）
3. 实现修复（最小化改动）
4. 验证：
   - 运行你的特定测试确认通过
   - `cargo test` — 所有测试通过
   - `cargo clippy -- -D warnings` — 干净
   - `cargo fmt --check` — 干净
   - `lsp_diagnostics` 对修改的文件干净
5. 如果行为、API 或配置变更：更新相关 `kb/` 文件
   （根据 `.opencode/skills/docs/SKILL.md` § 变更 → kb/ 映射表确定对应的 kb 文件）
6. 提交信息格式：`fix: <description>\n\nref #<issue_number>`
   - 将 kb/ 更新包含在同一提交中
7. 创建 PR：`gh pr create --title "fix: <description>" --body "Addresses #<issue_number>" --label "C-Bug,<A-label>"`
8. 在 issue 中评论附上 PR 链接 —— 由人工审核并合并

### 第 3b 步：复杂 → 报告

在 issue/PR 中发布评论，包含：
1. **根因分析**：你发现了什么
2. **建议方案**：你将如何修复
3. **建议**："此缺陷复杂度足够高，需要专门的 PR。建议 @mention 相关责任人。"
4. 不要实现。不要提交。

## CI 失败类 Issue

当 issue 带有 `S-CI-Failure` 标签时，它是由 `opencode-ci-fix` 工作流在 CI 运行失败后自动创建的。Issue 正文包含：

- 失败分支名称
- 提交 SHA（`head_sha`）
- CI 运行和详细日志的链接

### CI 专项分析

在应用标准决策树之前，收集 CI 上下文：

1. 通过 issue 正文中的 URL 阅读 CI 运行日志
2. 识别失败的 job：Build、Clippy、Format、Docs、Test、Bench、Coverage、Python Lint、Python Test
3. 分类失败类型：

| 类型 | 示例 | 典型修复 |
|---|---|---|
| 编译错误 | 类型不匹配、缺少导入 | 直接修复 |
| Clippy 警告 | `unwrap()`、死代码 | 直接修复 |
| 格式检查 | 缩进、行长超限 | `cargo fmt` |
| 测试失败 | 断言失败、panic、超时 | 分析测试 |
| 文档错误 | 断链、缺失文档 | 直接修复 |
| 基础设施 | Dolt 安装、网络问题 | 报告（临时故障） |

4. 通过 `git log -1` 检查可能的起因提交
5. 应用标准决策树（简单 vs 复杂）

### 示例

Issue："CI Failure: feat/new-provider"（标签：S-CI-Failure）

你：
- 阅读 CI 日志 → `clippy` job 在 `src/data/provider.rs:42` 因 `unwrap()` 失败
- 这是简单的（单文件，修复明确）
- 编写测试，修复 `unwrap()`，验证，提交，创建 PR

## 约束

- 对于简单缺陷，始终先编写测试
- 提交信息必须包含指向 issue 的 `ref #N`
- 始终创建 PR 分支并提交 PR 供审查 —— 绝不直接推送到 main/master
- 不要在 PR 正文中使用自动关闭 (`fixes #N` / `closes #N`) —— issue 在合并后由人工关闭
- 绝不使用 `as any` 或 `@ts-ignore` 压制类型错误
- 绝不使用 `unwrap()` —— 使用 `.expect()` 或正确的错误处理
- 如果对复杂度不确定，默认归类为复杂（报告，不修复）
