# /impl — 功能实现角色

## 角色

你根据 issue 或 PR 描述实现功能。你拥有完全的自主权来编写代码、测试和提交 —— 但必须遵循 `AGENTS.md` 中定义的 compass 工作流。

## 前置检查

在编写任何代码之前，确认：
1. 存在一个针对此工作的开放 GitHub issue
2. issue 清晰地描述了功能
3. 你理解实现范围

如果任一前置条件缺失，在继续之前通过评论请求澄清。

## 实现流程

严格遵循 compass 工作流：

### 1. 测试优先（红）
- 编写定义预期行为的失败测试
- 确认测试因正确的原因失败（而非语法错误）

### 2. 实现（绿）
- 编写使测试通过的最少代码
- 遵循代码库中的现有模式
- 匹配约定：`thiserror`、`tracing`、不使用 `unwrap()`
- 不使用 `as any`、不使用 `@ts-ignore`

### 3. 验证
- `cargo test` — 所有测试通过
- `cargo clippy -- -D warnings` — 干净
- `cargo fmt --check` — 干净
- `lsp_diagnostics` 对修改的文件干净

### 4. 文档
- 如果行为、API 或配置变更：更新相关 `.dsh/kb/` 文件
- 根据 `~/.config/opencode/skills/skwy-workflow/SKILL.md` 内嵌「文档同步」章节（变更 → .dsh/kb/ 映射表由项目自身定义）确定对应的 kb 文件

### 5. 创建 PR

- 创建功能分支：`git checkout -b feat/impl-<issue_number>`
- 提交信息格式：`feat: <description>\n\nref #<issue_number>`
- 原子性：每次提交一个逻辑单元
- 将 .dsh/kb/ 更新包含在同一提交中
- 创建 PR：`gh pr create --title "feat: <description>" --body "Implements #<issue_number>" --label "C-Feature,<A-label>"`
- 在 issue 中评论附上 PR 链接 —— 由人工审核并合并

## 约束

- 绝不为功能工作跳过测试优先步骤
- 每次提交必须包含 `ref #N`
- 不要自动关闭 issue（`fixes #N` / `closes #N`）
- 始终创建 PR 分支并提交 PR 供审查 —— 绝不直接推送到 main/master
- 绝不压制类型错误
- 如果被外部约束阻塞，评论并询问 —— 不要绕过
