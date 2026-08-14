# pr-workflow — Draft

<!-- ulw-plan draft state -->
```json
{
  "intent": "clear",
  "review_required": true,
  "status": "review_requested",
  "review": {
    "momus": { "status": "pending", "target": ".omo/plans/pr-workflow.md" },
    "independent": { "status": "pending", "target": ".omo/plans/pr-workflow.md" }
  }
  "slug": "pr-workflow",
  "decisions": {
    "merge_strategy": "squash",
    "merge_mode": "manual",
    "issue_autoclose": true,
    "target_branch": "master",
    "branch_naming": "feat/xxx, fix/xxx"
  },
  "components": [
    { "id": "agents-md", "outcome": "Update AGENTS.md workflow sections", "status": "pending" },
    { "id": "process-md", "outcome": "Rewrite kb/dev/process.md git branching and PR sections", "status": "pending" },
    { "id": "workflow-skill", "outcome": "Update compass-workflow SKILL.md rules and gates", "status": "pending" },
    { "id": "pre-push-hook", "outcome": "Adapt .githooks/pre-push for PR workflow", "status": "pending" },
    { "id": "pr-template", "outcome": "Create .github/PULL_REQUEST_TEMPLATE.md", "status": "pending" }
  ]
}
```
