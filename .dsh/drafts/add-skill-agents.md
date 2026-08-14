---
slug: add-skill-agents
status: awaiting-approval
intent: clear
review_required: true
pending-action: review .omo/plans/add-skill-agents.md
approach: Create 4 new opencode skill agents (qa, rustdoc, docs, reflect) under .opencode/skills/ following compass-workflow format, plus update compass-workflow SKILL.md to reference them at gate steps 3-4 + post-implementation.
review:
  momus: { status: approved, workspace_root: "/data/codes/compass", runtime_home: null, target: ".omo/plans/add-skill-agents.md", round_id: "r1", plan_sha256: "cb2c7943f2b908abae3c693cc7f6016a2f1a49a81942332895265c6352ff6961", launch_id: "l1", session: "ses_056bcbf6cffeR6eYehdI0fJ1eX", result: "APPROVE — 2 minor findings" }
  independent: { status: approved, workspace_root: "/data/codes/compass", runtime_home: null, target: ".omo/plans/add-skill-agents.md", round_id: "r1", plan_sha256: "cb2c7943f2b908abae3c693cc7f6016a2f1a49a81942332895265c6352ff6961", launch_id: "l2", session: "ses_056bcab18ffeockVxDqNp2vEQP", result: "CHANGES_REQUESTED → 4 blocking + 3 warnings → ALL FIXED, plan re-hashed as d1885f45" }
---

# Draft: add-skill-agents

## Components (topology ledger)
| C1: qa-agent | 创建 `.opencode/skills/qa/SKILL.md` — TDD/BDD 测试 agent | active |
| C2: rustdoc-agent | 创建 `.opencode/skills/rustdoc/SKILL.md` — pub API 文档 agent | active |
| C3: docs-agent | 创建 `.opencode/skills/docs/SKILL.md` — 项目书 + kb 维护 agent | active |
| C4: reflect-agent | 创建 `.opencode/skills/reflect/SKILL.md` — 反思 + 趋势分析 agent | active |
| C5: workflow-update | 更新 `compass-workflow/SKILL.md` — gate 步骤引用新 agent | active |
| C6: agents-md-update | 更新 `AGENTS.md` — available_skills 列表 | active |

## Open assumptions (announced defaults)
| A1: Trigger mechanism | Skills are standalone `/` slash commands invoked by agent at gate steps, not auto-loaded sub-skills. compass-workflow gate text instructs agent to invoke them. | Agent types `/test` etc. at gate step | Yes |
| A2: Gate step 4 split | Rustdoc checks `cargo doc --no-deps` (4a) then Docs identifies kb/ mapping (4b). Sequential, not parallel — rustdoc must pass first. | No shared step ambiguity | Yes |
| A3: reflect vs manual reflection | reflect replaces the existing "REFLECTION RECORD (MANDATORY)" section. compass-workflow no longer instructs agent to append reflection manually — it says "invoke `/reflect`". | Single source of truth | Yes |
| A4: Trend analysis scope | "Examines last 10 reflections for repeating patterns; produces ≤3 bullet points appended as 'Trends' subsection. Skipped if <3 entries exist." | Bounded, not open-ended | Yes |

## Findings (cited - path:lines)
- compass-workflow 格式参考：`.opencode/skills/compass-workflow/SKILL.md` — frontmatter (name+description) + markdown body with rules
- GitHub bot 角色格式参考：`kb/github/ask.md`, `kb/github/impl.md`, `kb/github/pr-review.md` — ## Role + ## Constraints + ## Output Format 三段式
- kb 文件结构：`kb/design/` (architecture, data-providers, symbols), `kb/dev/` (testing, process, reflections), `kb/user/` (index, gui, cli, config), `kb/github/` (ask, fix, impl, pr-review, ci-fix, comments, labels)
- 测试框架：rstest + tokio::test + httpmock + DuckDB in-memory，见 `kb/dev/testing.md`
- reflections 格式：`## [date] — <issue ref> <title>` + What was done / went wrong / lessons learned，见 `kb/dev/reflections.md`
- AGENTS.md 中 available_skills 列表引用格式：`<skill>` block with name, description, location，见 `AGENTS.md:lines 85-110`

## Decisions (with rationale)
1. 4 个 skill 全部采用 compass-workflow 的 frontmatter + markdown 格式
2. 每个 skill 必须包含：`name:`（slash 命令名）、`description:`、触发条件、职责边界、工作流程、输出格式、边界情况、Must NOT 禁止项
3. qa agent 引用 `kb/dev/testing.md` 的测试框架约定；qa 补充 TDD（BDD test 场景设计 + 覆盖率分析），不替代现有 Test First 规则
4. docs agent 必须包含完整的 kb/ 文件 → 变更类型映射表（覆盖全部 13+ 文件），扩展 compass-workflow 现有的不完整映射
5. reflect agent：每次 feature/bugfix 后写一条 reflection entry；检查最近 10 条 reflections 找重复模式；找到则追加 ≤3 条 "Trends" bullet；<3 条 entries 时跳过趋势分析
6. compass-workflow gate step 3 引用 qa，step 4 拆为 4a (rustdoc: `cargo doc --no-deps`) + 4b (docs: kb/ 映射)，post-implementation review step 5 引用 reflect
7. 每个 skill 的 directory name ≠ frontmatter `name`：qa → `name: test`，rustdoc → `name: rustdoc`，docs → `name: docs`，reflect → `name: reflect`

## Scope IN
- 创建 4 个 `.opencode/skills/<name>/SKILL.md` 文件
- 更新 `.opencode/skills/compass-workflow/SKILL.md`（gate steps 3-4 + review step 5 + reflection section）
- 更新 `AGENTS.md` 的 available_skills 列表（项目书部分）
- 更新 `kb/dev/process.md`（OpenCode workflow 表格加 slash command 行；Knowledge base sync 表格可由 docs agent 更新）

## Scope OUT (Must NOT have)
- 不修改 `kb/github/` 下的 GitHub bot 角色文件
- 不修改 `.github/workflows/` CI 配置
- 不修改 Rust 源码
- 不修改 `.opencode/opencode.json`（skill 是文件系统自动发现）
- docs agent 不创建新的 kb/ 文件（仅维护现有结构）
- rustdoc agent 不修改 `///` 文档内容（仅识别缺失的）

## Open questions
<!-- None — all resolved in grill-me -->

## Approval gate
status: awaiting-approval
All 6 components are active. No surviving forks — all design decisions locked in grill-me (Q1-Q9). Approach: write 4 SKILL.md files + update compass-workflow + update AGENTS.md, all referencing existing patterns.
