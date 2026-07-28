# Labels

Issue and PR labels follow the [Bevy](https://github.com/bevyengine/bevy) taxonomy
with a prefix-based category system. Each label is composed of `<PREFIX>-<Name>`.

## Prefixes

| Prefix | Category | Meaning |
|---|---|---|
| **A-** | Area | Which part of the codebase |
| **C-** | Category | What kind of work |
| **D-** | Difficulty | How complex is it |
| **P-** | Priority | How important is it |
| **S-** | Status | Current state of the issue/PR |

## A- Area

| Label | Scope |
|---|---|
| `A-GUI` | GUI chart window (`crates/compass-gui`) |
| `A-Data` | Data pipeline, providers, storage (`crates/compass-data`, `compass-core`) |
| `A-CLI` | CLI tools (`compass-data` binary) |
| `A-CI` | CI workflows, hooks, build system |
| `A-Docs` | Project book (`kb/`), `AGENTS.md`, README |

## C- Category

| Label | Usage |
|---|---|
| `C-Bug` | Unexpected or incorrect behavior |
| `C-Feature` | New feature or capability |
| `C-Code-Quality` | Refactoring, code that is hard to understand or change |
| `C-Performance` | Speed, memory, or compile time improvement |
| `C-Docs` | Documentation addition or correction |
| `C-Question` | Discussion or investigation (may become a feature request) |
| `C-Chore` | Dependencies, CI scripts, config, or other non-code changes |

## D- Difficulty

| Label | Meaning |
|---|---|
| `D-Trivial` | Simple and obvious fix |
| `D-Straightforward` | Clear solution exists, moderate effort |
| `D-Complex` | Requires research, design, or domain expertise |

## P- Priority

| Label | Meaning |
|---|---|
| `P-Critical` | Must be resolved immediately — blocks key workflow |
| `P-High` | High importance |
| `P-Medium` | Medium importance |
| `P-Low` | Low importance — can wait |

## S- Status

| Label | Meaning |
|---|---|
| `S-Blocked` | Cannot proceed until another task is completed |
| `S-Needs-Investigation` | Requires further investigation before action |
| `S-CI-Failure` | Auto-created by the CI failure workflow (`opencode-ci-fix`) |

## Usage

- Every issue and PR must have at least one **A-** and one **C-** label.
- **D-**, **P-**, and **S-** are optional.
- PRs inherit the issue's labels; add or remove as needed.
