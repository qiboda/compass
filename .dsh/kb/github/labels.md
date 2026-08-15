# 标签

Issue 和 PR 标签遵循 [Bevy](https://github.com/bevyengine/bevy) 分类法，采用基于前缀的分类体系。每个标签由 `<PREFIX>-<Name>` 组成。

## 前缀

| 前缀 | 类别 | 含义 |
|---|---|---|
| **A-** | 领域 | 代码库的哪一部分 |
| **C-** | 类别 | 什么类型的工作 |
| **D-** | 难度 | 有多复杂 |
| **P-** | 优先级 | 有多重要 |
| **S-** | 状态 | issue/PR 的当前状态 |

## A- 领域

| 标签 | 范围 |
|---|---|
| `A-GUI` | GUI 图表窗口（`crates/compass-gui`） |
| `A-Data` | 数据管线、提供者、存储（`crates/compass-data`、`compass-core`） |
| `A-CLI` | CLI 工具（`compass-data` 二进制文件） |
| `A-CI` | CI 工作流、钩子、构建系统 |
| `A-Docs` | 项目书（`.dsh/kb/`）、`AGENTS.md`、README |

## C- 类别

| 标签 | 用途 |
|---|---|
| `C-Bug` | 意外或不正确的行为 |
| `C-Feature` | 新功能或能力 |
| `C-Code-Quality` | 重构、难以理解或修改的代码 |
| `C-Performance` | 速度、内存或编译时间改进 |
| `C-Docs` | 文档添加或修正 |
| `C-Question` | 讨论或调研（可能转为功能请求） |
| `C-Chore` | 依赖、CI 脚本、配置或其他非代码变更 |

## D- 难度

| 标签 | 含义 |
|---|---|
| `D-Trivial` | 简单且显而易见的修复 |
| `D-Straightforward` | 方案明确，中等工作量 |
| `D-Complex` | 需要研究、设计或领域专业知识 |

## P- 优先级

| 标签 | 含义 |
|---|---|
| `P-Critical` | 必须立即解决 —— 阻塞关键工作流 |
| `P-High` | 高优先级 |
| `P-Medium` | 中等优先级 |
| `P-Low` | 低优先级 —— 可以等待 |

## S- 状态

| 标签 | 含义 |
|---|---|
| `S-Blocked` | 在其他任务完成之前无法继续 |
| `S-Needs-Investigation` | 在行动前需要进一步调研 |
| `S-CI-Failure` | 由 CI 失败工作流（`opencode-ci-fix`）自动创建 |

## 使用

- 每个 issue 和 PR 必须至少有一个 **A-** 和一个 **C-** 标签。
- **D-**、**P-** 和 **S-** 是可选的。
- PR 继承 issue 的标签；根据需要添加或移除。
