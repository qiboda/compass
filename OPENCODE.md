# OpenCode Workflow Rules

AGENTS.md contains the project architecture, tech stack, and conventions. Read it before starting any work.

Development follows the issue-driven cycle: user raises requirement → grill-me → create issue → implement → push.

## Workflow rules

1. **requirement-flow (CRITICAL)**: Before writing any feature or bugfix code: a) verify an open GitHub issue exists (gh issue view <N> — must return 'OPEN'), b) if none exists, create one with gh issue create using the appropriate template, c) gh issue view <N> to confirm, d) only then implement. The pre-push hook will reject pushes referencing nonexistent or closed issues. Never skip this.
2. **issue-linking**: Commit messages reference issues with `ref #N`. Issues are closed manually — do NOT use `fixes #N` or `closes #N` keywords.
3. **plan-mode**: Multi-step tasks (2+ modules, architecture changes, new data sources) MUST use /ulw-plan before implementing. Single-file fixes, tests, and doc updates may proceed directly with explore → implement.
4. **local-verify-before-push**: Run `cargo nextest run && cargo clippy -- -D warnings && cargo fmt --check` before every push. All must pass.
5. **per-step-verify**: After every code change: run `cargo nextest run` and ensure `lsp_diagnostics` is clean on changed files.
6. **push-after-issue**: Push to main immediately after completing each issue. Do not batch commits across multiple issues.
7. **atomic-commits**: Commit each logical unit separately. One commit = one purpose (feat, fix, test, refactor, docs, chore). Use `.gitmessage` template format.
8. **branching**: Trunk-based: push directly to `master`. No feature branches, no PRs for solo development.
9. **doc-sync (CRITICAL)**: Any code change that affects behavior, public APIs, data structures, config, workflows, or conventions MUST sync to the knowledge base. Identify which kb/ file(s) are affected and update them in the same commit. AGENTS.md must be updated if the architecture overview or project-level conventions change. This is not optional — stale docs are worse than no docs.
10. **test-first (TDD)**: For all feature and bugfix work: write the failing test FIRST, watch it fail, then implement until green. Refactor after green. Exploratory changes may write tests after implementation.
11. **test-verify**: Every test must fail before implementation. If a test passes without implementation changes, it's a false positive — delete or rewrite it.
12. **no-type-escape**: Never use `as any`, `@ts-ignore`, or `unwrap()` in production code. Use `.expect(msg)` or proper error handling.
13. **code-style**: Match existing codebase conventions: Rust edition 2024, thiserror for errors, async-trait for trait async fns, tracing for logging.
