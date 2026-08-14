# workflow-practices - Work Plan

## TL;DR (For humans)

**What you'll get:** 三个新的工作流实践落地：你的每次纠正会被自动记录（摩擦记录），每周一会自动生成产品需求候补（全局规划），每个设计文档会说明「为什么选这个方案」（决策记录）。

**Why this approach:** 全部嵌入现有流程 —— friction 对等 reflect 技能，sprint 对等 "先有计划再执行" 的纪律，决策记录对等设计文档本身。不引入新系统，不动 Rust 代码。

**What it will NOT do:** 自动创建 GitHub issues/milestones（product agent 只分析，不执行），修改已有设计文档的内容，引入新的测试或编译步骤。

**Effort:** Quick
**Risk:** Low — 纯文档和配置变更，无运行时影响
**Decisions to sanity-check:** product agent 是作为独立 skill 还是嵌入 AGENTS.md 的行为指令（已选：独立 skill）

Your next move: approve。完整执行细节如下。

---

> TL;DR (machine): Quick, Low risk, 7 doc/skill-config deliverables — no Rust code

## Scope
### Must have
- `kb/dev/friction.md` — friction record template with example entry
- `.opencode/skills/friction/SKILL.md` — auto-detect + manual friction recording
- `kb/design/roadmap.md` — product vision + sprint placeholder
- `.opencode/skills/compass-workflow/SKILL.md` — updated with sprint hook, friction trigger, decision record GATE check
- `AGENTS.md` — updated with new sections and skill table entries
- `kb/design/architecture.md`, `data-providers.md`, `symbols.md` — each gets `## 决策记录` section
- `.opencode/skills/product/SKILL.md` — product agent as a skill

### Must NOT have
- Rust code changes, compilation, or test runs
- Auto-creation of GitHub issues/milestones (product agent is read-only)
- Changes to existing design prose content
- Modifications to grill-me, pre-implementation gate, or review process rules
- New dependencies or toolchain changes

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: none — documentation-only, no Rust compilation needed
- Evidence: grep-based verification for each file (see acceptance criteria per todo)

## Execution strategy
### Parallel execution waves
Single wave — all 7 files are independent. No compilation dependencies. All can be written in parallel.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. friction.md | — | — | 2, 3, 4, 5, 6, 7 |
| 2. roadmap.md | — | — | 1, 3, 4, 5, 6, 7 |
| 3. friction skill | — | — | 1, 2, 4, 5, 6, 7 |
| 4. compass-workflow | — | — | 1, 2, 3, 5, 6, 7 |
| 5. AGENTS.md | — | — | 1, 2, 3, 4, 6, 7 |
| 6. design decision records | — | — | 1, 2, 3, 4, 5, 7 |
| 7. product agent | — | — | 1, 2, 3, 4, 5, 6 |

## Todos
> All tasks are documentation/skill-config only. No Rust code. No compilation needed.
> All tasks run in a single wave — every file is independent.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. Create `kb/dev/friction.md` with header and entry template
  What to do: Write a new file `kb/dev/friction.md` with:
    - `# 摩擦记录` heading
    - Template header comment: fields = `[日期] [关联会话] [我的偏差] [你的纠正] [教训]`
    - One example entry from THIS grill-me session (the grill-me-anchoring correction)
  Must NOT do: Touch reflections.md, include design decisions, add to kb/user/ or kb/github/
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `kb/dev/reflections.md:1-5` for format style
  Acceptance criteria: `grep "摩擦记录" kb/dev/friction.md` succeeds, template fields present
  QA scenarios: Manual: read kb/dev/friction.md, verify template + example entry exist
  Commit: Y | docs: add friction record template

- [ ] 2. Create `kb/design/roadmap.md` with product vision and milestone placeholder
  What to do: Write a new file `kb/design/roadmap.md` with:
    - `# 产品路线图` heading
    - 「产品愿景」section: local-first A-share chart app 的产品目标（1-2 段）
    - 「当前 Sprint」section: placeholder linking to GitHub Milestones
  Must NOT do: Fill in specific sprint backlog items (product agent does that)
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `kb/design/architecture.md:1-10` for what Compass is
  Acceptance criteria: `grep "产品路线图" kb/design/roadmap.md` succeeds, vision section exists
  QA scenarios: Manual: read kb/design/roadmap.md, verify vision + sprint placeholder sections
  Commit: Y | docs: add product roadmap

- [ ] 3. Create `.opencode/skills/friction/SKILL.md` — friction recording skill
  What to do: Model after `reflect/SKILL.md`. The skill MUST:
    - Auto-detect: when user corrects AI behavior → prompt "记录摩擦?"
    - Manual: `/friction` slash command → append entry to `kb/dev/friction.md`
    - Template format: `## YYYY-MM-DD — <关联会话>` followed by fields
    - Append-only (never edit past entries)
  Must NOT do: Overlap with reflect skill (reflections = post-implementation, friction = during-work)
  Must NOT do: Record design decisions (those go in kb/design/)
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `.opencode/skills/reflect/SKILL.md` (full file, 151 lines) for structure
  Acceptance criteria: File exists at `.opencode/skills/friction/SKILL.md`, contains `/friction` trigger description
  QA scenarios: Manual: read SKILL.md, verify auto-detect + manual trigger described
  Commit: Y | docs: add /friction skill

- [ ] 4. Update `.opencode/skills/compass-workflow/SKILL.md` — sprint hooks + friction trigger + decision record check
  What to do: Add to compass-workflow SKILL.md:
    - In Rules section: new rule "Sprint Rhythm" — Monday: plan milestone + invoke product agent. Sunday: review + close milestone.
    - In Rules section: new rule "Friction Record" — detect user correction → suggest /friction
    - In PRE-IMPLEMENTATION GATE: add check — "Are decision records present in relevant kb/design/ files?"
    - In Available Skills table: add friction row (`/friction`, post-correction / on-demand)
  Must NOT do: Remove or reorder existing rules, change GATE numbering, alter REVIEW flow
  Parallelization: Wave 1 (parallel) | Blocked by: — (reference by name only) | Blocks: —
  References: `.opencode/skills/compass-workflow/SKILL.md` (full file, 263 lines)
  Acceptance criteria: `grep "Sprint Rhythm" .opencode/skills/compass-workflow/SKILL.md` succeeds; `grep "Friction Record"` succeeds; `grep "决策记录"` in GATE section succeeds
  QA scenarios: Manual: read updated compass-workflow SKILL.md, verify new rules + friction row in table
  Commit: Y | docs: add sprint/friction/decision-records to compass-workflow

- [ ] 5. Update `AGENTS.md` — reference all new practices
  What to do: Add to AGENTS.md:
    - Available Skills table: add `/friction` row
    - New section after Scope Discipline: "## Sprint 规划" — weekly milestone rhythm, product agent on Monday, review on Sunday
    - New section: "## 摩擦记录" — when to record, file location, /friction command
    - New section: "## 决策记录" — `## 决策记录` table in design docs, compass-workflow GATE check
    - Knowledge base file table: add `kb/dev/friction.md` and `kb/design/roadmap.md`
  Must NOT do: Change GRILL-ME FIRST section, alter PRE-IMPLEMENTATION GATE, modify existing rules
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `AGENTS.md` (full file, 374 lines)
  Acceptance criteria: `grep "/friction" AGENTS.md` succeeds; `grep "Sprint 规划" AGENTS.md` succeeds; `grep "决策记录" AGENTS.md` succeeds
  QA scenarios: Manual: read AGENTS.md, verify new sections exist and don't conflict with existing rules
  Commit: Y | docs: add sprint/friction/decision-records to AGENTS.md

- [ ] 6. Add `## 决策记录` sections to `kb/design/architecture.md`, `kb/design/data-providers.md`, `kb/design/symbols.md`
  What to do: For each of the 3 design files:
    - Read the file to identify key architectural decisions already described in prose
    - At end of file (before EOF), add `## 决策记录` section with table: `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`
    - Extract at least 2-3 decisions per file from existing text
    - If a file has no identifiable decision → add placeholder "（暂无已记录的决策）"
  Must NOT do: Invent new decisions not in the file, change existing prose, remove content
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `kb/design/architecture.md`, `kb/design/data-providers.md`, `kb/design/symbols.md`
  Acceptance criteria: Each of the 3 files has a `## 决策记录` section with a table matching the format
  QA scenarios: Manual: read each file's decision record section, verify table format
  Commit: Y | docs: add decision records to design files

- [ ] 7. Create `product` agent as a skill: `.opencode/skills/product/SKILL.md`
  What to do: Create the product agent as a skill:
    - Auto-run: Monday sprint planning — scan codebase (git log, open issues, roadmap.md, kb/design/) → output 3-5 milestone candidates
    - Manual: `/product brainstorm` — same behavior on-demand
    - Output format: numbered list with title, brief rationale, suggested priority
    - NOT an implementer — read-only analysis only
  Must NOT do: Create GitHub issues or milestones automatically (read-only analysis), modify code
  Parallelization: Wave 1 (parallel) | Blocked by: — | Blocks: —
  References: `.opencode/skills/issue-workflow/SKILL.md` for skill structure pattern
  Acceptance criteria: File exists at `.opencode/skills/product/SKILL.md`, contains `/product brainstorm` trigger and output format
  QA scenarios: Manual: read product SKILL.md, verify trigger + output format described
  Commit: Y | docs: add product agent skill

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE.
- [ ] F1. `ls kb/dev/friction.md kb/design/roadmap.md .opencode/skills/friction/SKILL.md .opencode/skills/product/SKILL.md` — all 4 new files exist
- [ ] F2. `grep -l "friction\|Sprint 规划\|决策记录\|product" AGENTS.md .opencode/skills/compass-workflow/SKILL.md` — both updated files reference new practices
- [ ] F3. `grep -c "## 决策记录" kb/design/architecture.md kb/design/data-providers.md kb/design/symbols.md` — all 3 design files have decision record section
- [ ] F4. Manual: review all 7 changes for consistency with grill-me decisions

## Commit strategy
Single commit: `docs: add friction records, sprint planning, and decision records — ref #69`

## Success criteria
- `kb/dev/friction.md` exists with correct template and example entry
- `/friction` skill loadable and describes auto-detect + manual trigger
- `compass-workflow` skill includes sprint rhythm rule and friction trigger
- `product` agent loadable as skill with `/product brainstorm` command
- `AGENTS.md` references all new practices
- All 3 `kb/design/*.md` files have `## 决策记录` tables
