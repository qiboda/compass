---
name: friction
description: Records friction moments where user corrected AI behavior deviations. Auto-detects corrections and appends to kb/dev/friction.md.
---

# Friction — Correction Recording Agent

## Role

Record moments when the user corrects AI behavior — behavior deviations,
misunderstandings, missed constraints, or anchoring bias. Stores entries
in `kb/dev/friction.md`. Append-only.

This agent is the **friction historian** — it captures what the AI got wrong
so the same mistake isn't repeated. Peer to `/reflect` (post-implementation
reflections), but runs **during** work rather than after.

## Trigger

- **Auto-detect**: when user contradicts or overrides AI's previous output →
  prompt the user: "记录这次摩擦到 kb/dev/friction.md？"
- **Manual**: `/friction` slash command → directly append entry

## Workflow

### Step 1: Detect correction

When the user says something that contradicts, overrides, corrects, or
broadens the AI's previous output, recognize it as a correction event.

Detection signals:
- User says "不是..." / "不对..." / "应该是..." / "不仅..."
- User provides a counter-example to AI's stated scope
- User adds a constraint the AI missed
- User redirects the AI's approach

### Step 2: Prompt user

After the correction has been resolved (new understanding reached), ask:

> 记录这次摩擦到 kb/dev/friction.md？

Do NOT interrupt the ongoing task flow. Ask after the immediate correction
is processed, not during it.

### Step 3: Append entry

If user confirms, append to `kb/dev/friction.md` using the template:

```markdown
## YYYY-MM-DD — <关联会话或issue>

**我的偏差**: <what the AI got wrong>

**你的纠正**: <what the user corrected>

**教训**: <actionable lesson learned>
```

If `kb/dev/friction.md` doesn't exist, create it with a `# 摩擦记录` heading
and a brief description, then append the entry.

### Step 4: Decline

If user declines, respect the choice. Note "skipped" silently — do not
re-prompt for the same correction.

## Output Format

```
## Friction: <session/issue context>

### Entry
<the appended entry>

### Verdict
<Entry appended to kb/dev/friction.md> or <User declined — skipped>
```

## Edge Cases

| Scenario | Behavior |
|---|---|
| friction.md doesn't exist | Create with heading, then append |
| User declines recording | Respect silently; note "skipped" |
| Same correction detected twice | Skip — don't create duplicate entries |
| Multiple corrections in one turn | Record each separately; one entry per correction |
| Correction happens during grill-me | Record normally — grill-me corrections are valid friction |

## Must NOT

- **Modify past friction entries** — only append new ones
- **Record design decisions** — those go in `kb/design/` 决策记录 sections
- **Overlap with reflect skill** — reflections = post-implementation, friction = during-work
- **Interrupt active work** — prompt after correction is resolved, not during
- **Judge the user's correction** — record factually, don't editorialize
- **Create issues or modify code** — read + write to friction.md only

## Collaboration with compass-workflow

1. compass-workflow Rule 11 (Friction Record): "When user corrects AI behavior →
   pause and suggest recording via `/friction`"
2. The compass-workflow agent detects the correction and invokes this skill
3. This skill handles the recording flow independently
4. Friction entries are NOT committed separately — they ship with the next commit

## Template Reference

The canonical entry format in `kb/dev/friction.md`:

```markdown
## YYYY-MM-DD — <关联会话或issue>

**我的偏差**: <what the AI got wrong>

**你的纠正**: <what the user corrected>

**教训**: <actionable lesson>
```
