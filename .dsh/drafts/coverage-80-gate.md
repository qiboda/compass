---
slug: coverage-80-gate
status: awaiting-approval
intent: clear
review_required: true
pending-action: write .omo/plans/coverage-80-gate.md
approach: 一步到位（落地策略 A）——先补全部测试（Rust 3 crate + Python），最后改 CI 加 80% 强制门槛，一个 PR 合并
---

# Draft: coverage-80-gate

## Components (topology ledger)
<!-- id | outcome (one line) | status: active|deferred | evidence path -->
| id | outcome | status |
|----|---------|--------|
| rust-core | compass-core 补测（duckdb 62 + parquet 73 行缺口）→ ≥80%（现 ~90%） | active |
| rust-data | compass-data 补测（main/baostock/export/import_compass/import_dolt 缺口）→ ≥80%（现 ~75-80%） | active |
| rust-gui | compass GUI 补测（kittest 集成测试 + backend tokio）→ ≥80%（现 28.87%） | active |
| py-collectors | Python 补测（全量计入，18% → 80%） | active |
| ci-gate | CI 强制门槛（llvm-cov 4 命令 + pytest --cov-fail-under） | active |

## Open assumptions (announced defaults)
<!-- assumption | adopted default | rationale | reversible? -->
| assumption | adopted default | rationale | reversible? |
|-----------|-----------------|-----------|-------------|
| GUI 测试存放方式 | 不加 lib.rs，kittest 测试内嵌各文件 `#[cfg(test)] mod`（main.rs tests mod 可访问私有 CompassApp） | 避免 binary→lib 结构重构风险；kittest 已证明无头可驱动 eframe::App | 是（后续可抽 lib） |
| egui_dock tab 切换测试 | 程序化 `DockState::set_active_tab` + 断言 tab 内容；tab 按钮渲染由 kittest 跑帧覆盖（不交互断言） | egui_dock 0.20.1 tab 按钮无 accesskit label，`get_by_label` 不可定位 | 是 |
| CI 门槛命令 | 4 条 `cargo llvm-cov`（workspace + 3 crate `-p`），各自 `--fail-under-lines 80`，保留 --html artifact | 内建 fail-under 机制最简单可靠；接受 CI 耗时增加（coverage job 本就要跑测试） | 是 |
| Python omit 配置 | pyproject `[tool.coverage.run] omit = ["tests/*"]` + `--cov=. --cov-fail-under=80` | CLI `--cov-omit` 更啰嗦；配置文件与 Makefile 共享 | 是 |
| Python HTTP mock | conftest 手写 stub AsyncSession（async get()，canned JSON，429/异常注入），不用 respx | curl-cffi 不被 respx/responses 支持；session 已是函数参数，注入即通 | 是 |
| baostock.rs 重构 | `fetch_adj_factors` 提取 `fetch_adj_factors_with_script(script, ...)` 注入脚本路径（默认保持原路径） | 真实脚本 `scripts/fetch_adj_factor.py` 不存在，当前函数必然失败；注入后纯解析可测 | 是 |
| main.rs 重构（Rust CLI + Python main.py） | 提取可测的 dispatch/run 函数，main 变薄包装 | 0% 文件唯一可测路径 | 是 |
| egui_kittest 版本 | dev-dep `egui_kittest = "0.35"`（匹配 egui 0.35.0，零 feature，MSRV 1.92 < 项目 1.96） | crates.io 实测存在；需网络拉取（CI/本地均可） | 是 |
| CompassApp 构造 | 测试内 `egui::Context::default()` + `wire_backend`（配置指不存在的 parquet dir）+ 纯构造其余字段 | BackendHandle 无 pub 构造器，仅此路径；AsyncDispatcher 自带 runtime，非 async 测试可用 | 是 |

## Findings (cited - path:lines)
- Rust 基线（本机 cargo llvm-cov）：总 72.44%；compass-core ~90%（duckdb.rs 94.39%、parquet.rs 86.68%）；compass-data ~75-80%（baostock 67.67%、import_compass 67.42%、import_dolt 87.85%、export 88.14%、main.rs 0%）；compass GUI 28.87%（backend.rs 0%、tabs.rs 0%、main.rs 28.87%、modal 38%、searchable_dropdown 51%、toast 52%、chart 43%、logger 15%、dispatcher 27%、theme 43%）
- Python 基线：`--cov=.` 34%（含测试文件），不含测试文件 **18%**；0% 模块：main.py、fetch_cash_flow.py、fetch_income.py、fetch_fin_indicators.py
- `crates/compass` 是纯 binary crate（无 lib.rs、无 tests/、无 dev-deps）— crates/compass/Cargo.toml
- `_frame` 参数在 CompassApp::ui 未使用 — crates/compass/src/main.rs:245-298
- eframe 0.35 `App::ui(&mut self, ui, frame)` 为 required 方法，`Frame::_new_kittest()`/`CreationContext::_new_kittest()` 专为无头测试 — eframe 0.35.0 epi.rs L117-138, L711-731
- egui_kittest 0.35.0 匹配 egui ^0.35.0、kittest ^0.4.0，无 default features；Harness 走 `ctx.run_ui()` 纯 CPU 无头；`enable_accesskit()` 自动调用 — egui_kittest 0.35.0 lib.rs L128, L146
- egui 0.35 accesskit 非 feature（PR #7701 起总是启用），但树需 `enable_accesskit()` 才生成 — egui 0.35.0 context.rs L1224-1229
- egui_dock 0.20.1 零 accesskit 处理；tab 按钮 raw `ui.interact`（Role::Unknown 无 label）、标题 raw TextShape — egui_dock leaf.rs L968, L1016-1024
- compass-data `baostock.rs` 调用 `scripts/fetch_adj_factor.py`，该文件不存在 — crates/compass-data/src/baostock.rs
- ci.yml coverage job L107-132：continue-on-error:true、`cargo llvm-cov --html`、上传 artifact、已装 Dolt；python-test job L142-154 无 --cov — .github/workflows/ci.yml
- opencode-ci-fix.yml 在 CI failure 时自动建 CI Failure issue（硬门槛的直接副作用）
- 测试基础设施：Rust 用 tempdir + in-memory duckdb + `dolt --data-dir <tmp> init`（无持久 Dolt 测试库）；Python 用 tempfile + COMPASS_DATA_DIR monkeypatch；Dolt binary 在 /usr/bin/dolt
- 无任何现存覆盖率阈值/文档 — kb/dev/testing.md、kb/dev/process.md 均无 coverage 章节

## Decisions (with rationale)
| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 覆盖粒度 | 总/每 crate | 总 ≥80% + 每 crate（core/data/compass）各自 ≥80% | 用户 grill-me 确认；防止 core 拉平 GUI 短板 | 仅总覆盖率可假达标 |
| GUI 测试策略 | 单元/kittest | egui_kittest 集成测试 + backend tokio 测试 | 官方测试库、无头 CI 可跑、AccessKit 查询真实交互 | 单测覆盖不到 UI 交互 |
| Python 测量范围 | 仅被测/全量 | `--cov=.` 全量计入所有 .py | 用户确认；未测文件按 0% 计 | 假达标 |
| 落地策略 | 渐进/一步 | A 一步到位（一个 PR） | 用户确认 | 渐进需多次 PR |
| 指标类型 | lines/regions | 行覆盖率（--fail-under-lines） | 业界默认，与 Python Stmts 对齐 | — |

## Scope IN
- compass-core：duckdb.rs + parquet.rs 缺口测试（error 路径、overwrite=false、空数据、search_symbols、load_all_stock_basics 等）
- compass-data：main.rs load_config + clap 解析；baostock.rs 脚本注入重构 + 解析测试；export/import_compass/import_dolt 错误分支与 WHERE 子句
- compass GUI：dev-deps（egui_kittest 0.35、rstest）；backend.rs tokio 集成测试；dispatcher/tabs/theme/widgets/citizens 测试；main.rs CompassApp kittest 集成测试
- Python：conftest stub session fixture；test_cash_flow/test_income/test_fin_indicators/test_main 新建；test_common/test_stock_basic 补缺；main.py dispatch 重构
- CI：coverage job 去 continue-on-error + 4 条 llvm-cov --fail-under-lines 80；python-test 加 --cov=. --cov-fail-under=80
- 文档：kb/dev/testing.md 加覆盖率章节；kb/user/config.md（如涉及）；AGENTS.md 提及门槛

## Scope OUT (Must NOT have)
- 不加 lib.rs / 不重构 binary→lib
- 不引入 respx/responses/httpx-mock（stub session 足够）
- 不改 egui_dock / egui 第三方库源码
- 不删除或忽略失败测试来"达标"
- 不使用 `#[allow(dead_code)]`、`as any`、`@ts-ignore` 类绕过
- 不碰 /data/compass-data 真实 Dolt 仓库（测试全用 tempdir）
- 不修 baostock 脚本缺失之外的无关 bug（如 tenacity 未使用、fin_indicators 硬编码路径——只记录不修）
- 不在 commit 中使用 fixes/closes（只用 ref #N）

## Open questions
无（grill-me 已闭合全部 owner-decision；其余按 announced defaults 采用，可被用户否决）

## Review receipts (high-accuracy, 2026-08-01)
| Reviewer | Verdict | Key findings → resolutions |
|---|---|---|
| Oracle (independent) | APPROVE with 4 corrections | ① stub 需 `__aenter__/__aexit__`（main() 用 async with）→ T2 已加；② `Harness::new_eframe` 属 egui_kittest crate（非 eframe）→ T13 已澄清；③ T10 AsyncDispatcher 自带 runtime、slot.start 用 thread::spawn 已验证 `#[test]` 可行；④ CI artifact 只保留第一条命令 → T15 已改 |
| Momus (plan critic) | 2 BLOCKING + 5 rigor 修复 | ① **Python 目标数学不自洽**（各文件目标达成仅 76.3%，fetch_balance_sheet 无 todo）→ T14 加 test_balance_sheet（run/import_to_dolt）+ stock_basic 提到 ≥85%，总和 ≈83%；② **egui_kittest 需 `features=["eframe"]`**（new_eframe 门控）→ T1 已改；③ GUI 加权仅 ~80% 卡线 → T13 加 crate 总 80% 硬验收 + 各文件目标上调；④ conftest 路径实际为 tests/conftest.py → T2 已改；⑤ T5 clap 行计入假设存疑 → 已加兜底（提取 dispatch）；⑥ T14 已核 balance_sheet run:119-180/import_to_dolt:182-255 |

## Approval gate
status: awaiting-approval
<!-- 探索已穷尽，未知已解决。等待用户批准后写 .omo/plans/coverage-80-gate.md -->
