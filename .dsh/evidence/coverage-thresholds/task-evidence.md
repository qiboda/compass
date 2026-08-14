# Task Evidence — coverage-thresholds (issue #250)

## Task 1: compass-types 补测（adversarial tests）
- 14 个对抗性测试写入 `crates/compass-types/tests/adversarial_serde.rs`（skwy-adversarial-test 产出）
- 覆盖：`momentum = {}` / `volume = {}` 空表、部分字段缺省、u32 边界、类型错误、无业务校验、round-trip
- 验证：`cargo nextest run -p compass-types` → 23 tests passed (9 原有 + 14 新增)
- 效果：compass-types 行覆盖 89.58% → **100%**

## Task 2: compass-i18n 补测（requirement tests）
- 提取 `is_allowed_zh_token` 纯函数 + 主测试重构 + 2 个表驱动测试（正向 16 用例 / 负面 6 key）
- skwy-requirement-test 产出代码，主 agent 落盘（3 处 edit，含 1 处 `&c` 类型修复）
- 验证：`cargo nextest run -p compass-i18n` → 6 tests passed (4 原有 + 2 新增)
- 效果：compass-i18n 行覆盖 93.94% → **99.14%**

## Task 3: check-coverage.sh 阈值表
- THRESHOLDS: workspace=93, core/data/i18n/strategy/types/ui=95, compass=90
- 头注释 (L9-12) + fallback 注释同步；grep 断言通过

## Task 4: doc-sync
- AGENTS.md L499、kb/dev/testing.md L249-253+L266-270、kb/dev/process.md L122、ci.yml L49 step name
- 残留旧阈值 grep = clean (exit 1)

## Task 5: 全量验证（主证据：task-5-verify.json）
`cargo llvm-cov nextest --json --summary-only --output-path target/llvm-cov/coverage.json`
→ EXIT=0，check-coverage.sh 8 项全 OK：

| Crate | 实测 | 阈值 |
|---|---|---|
| workspace | 96.16% | 93% |
| compass-core | 97.99% | 95% |
| compass-data | 96.81% | 95% |
| compass-i18n | 99.14% | 95% |
| compass | 92.74% | 90% |
| compass-strategy | 96.97% | 95% |
| compass-types | 100% | 95% |
| compass-ui | 97.47% | 95% |

## Task 6: 决策记录检查（Step 5c）
- kb/design/architecture.md L503 含 `## 决策记录`；L510 历史决策记录（80% 门槛，2026-08-01）按项目规则不可变，本次不修改，commit message 说明

## 工具链排查记录（llvm-cov 首次运行失败）
- 症状：`cargo llvm-cov nextest` 首次运行报 `[double-spawn] failed to exec ... No such file or directory (os error 2)`，JSON 空
- 诊断：nextest 在 compass_core 二进制尚未链接完成时尝试 `--list`（构建竞态）；二进制最终存在（987MB @21:15:47）
- 处理：重跑即成功（EXIT=0），非环境/配置问题，无需修复
- 记录：kb/dev/toolchain.md 无此问题先例，本次为一次性竞态
