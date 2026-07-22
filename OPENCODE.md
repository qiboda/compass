# OpenCode Workflow Rules

AGENTS.md is auto-loaded by OpenCode — contains architecture, tech stack, conventions.
See `kb/` for detailed docs on specific subsystems.

Development follows the issue-driven cycle:
user raises requirement → /grill-me to clarify → create GitHub issue → plan (if multi-step) → implement → push.

## Rules (ordered by priority)

1. **doc-sync (CRITICAL)**: Any code change affecting behavior, public APIs, data structures,
   config, or workflows MUST update the relevant `kb/` files AND `AGENTS.md` in the SAME commit.
   Stale docs are worse than no docs.

2. **requirement-flow (CRITICAL)**: Before writing any feature or bugfix code:
   a) verify an open GitHub issue exists (`gh issue view <N>`),
   b) if none exists, create one with `gh issue create`,
   c) `gh issue view <N>` to confirm,
   d) only then implement.
   Refactors, docs, lint fixes, and typos skip this.

3. **plan-first**: Multi-step tasks (2+ modules, architecture changes, new data sources,
   ambiguous scope) MUST use the plan agent first. Single-file fixes, tests, and doc updates
   may proceed directly.

4. **test-first**: Feature and bugfix work follows RED → GREEN → REFACTOR.
   Write the failing test FIRST, watch it fail for the right reason, then implement.
   Exploratory changes may write tests after. Pure refactors: pin current behavior with
   characterization tests first.

5. **per-step-verify**: After every code change, run `cargo test` and ensure
   `lsp_diagnostics` is clean on changed files.

6. **local-verify-before-commit**: Run before committing:
   ```sh
   cargo test && cargo clippy -- -D warnings && cargo fmt --check
   ```

7. **no-type-escape**: Never use `unwrap()` in production code — use `.expect(msg)` or
   proper error handling. Never suppress type errors with `as` casts or `@ts-ignore`.

8. **branching**: Trunk-based: push directly to `main`. No feature branches for solo work.

## Commit style

- `feat:` / `fix:` / `test:` / `refactor:` / `docs:` / `chore:`
- Atomic: one logical unit per commit
- Push immediately after commit — don't batch across multiple changes

## Code style

- Rust edition 2024, thiserror for errors, async-trait for trait async fns, tracing for logging
- Match existing conventions in the file you're editing
