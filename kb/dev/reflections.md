# Reflection Log

Post-implementation reflections for every feature and bugfix. Each entry
captures what was done, what went wrong, and what to do differently.

---

## 2026-07-23 — ref #8 feat: all imports default to skip-existing, --overwrite to force replace

**What was done**: Added `overwrite: bool` parameter to all DuckDB write methods
and CLI subcommands. Default `false` (skip existing rows, migration-style),
`--overwrite` flag to force replace. Spanning 17 files across compass-core
and compass-data crates.

**What went wrong**: Skipped the entire compass-workflow PRE-IMPLEMENTATION GATE:
- No `/grill-me` interview before starting (step 0)
- No GitHub issue created before implementation (step 1)
- No plan agent used for 9-file, 2-crate change (step 2)
- No test-first — tests written after implementation (step 3)
- Docs updated after push, not in same commit (step 4)
- Also: `.gitignore` had `data/` which accidentally matched
  `crates/compass-core/src/data/`, hiding 7 core source files from git

**Lessons learned**:
1. The workflow skill was loaded but lacked structural enforcement — rules
   were advisory, not blocking. Fixed by adding GATE MODE trigger with
   explicit tool restrictions.
2. Grill-me is now step 0 in the gate — it MUST run before issue creation.
3. Post-implementation reflection is now mandatory — appended to this file.
4. `.gitignore` should use `/data` not `data/` to only match root.
