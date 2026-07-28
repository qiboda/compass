# Comments

## 核心规则：永远追加，绝不修改

在 issues 和 PRs 上添加 comment 时，遵循以下规则：

- **总是追加（append）新 comment** 来描述变更、补充说明、添加注意事项
- **绝不修改（edit）已有 comment**，除非该 comment 包含事实性错误（如 typo、错误的引用号）
- **即使只是补充一个字，也要追加新 comment**，不在原 comment 上叠加

## 适用范围

- 所有 GitHub Issues
- 所有 GitHub Pull Requests
- 包括 AI agent 和人类操作者

## 原因

- 每条 comment 有独立的时间戳和编辑记录，追加新 comment 保持历史清晰
- 编辑已有 comment 会导致通知丢失、上下文断裂
- GitHub 的 comment 历史可追溯，追加比修改更符合审计需求

## 注意事项

- 追加 comment 时引用上下文：`> 原内容` 或用 `ref #N` 链接相关内容
- 如果原 comment 有事实性错误，先追加更正 comment，再考虑是否编辑原文
- PR review comment（inline diff comment）同样适用此规则
