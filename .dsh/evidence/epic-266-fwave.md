# F-wave evidence — epic #266 data-name-i18n

- **Worktree**: data-name-i18n (feat/data-name-i18n)
- **Date**: 2026-08-15
- **Base**: master 09e8f3e

## F1 合规审计 ✅

8 commits, 每 commit 独立成行 `ref #<sub-N>`（hook 校验通过）:

| commit | 子 issue | 内容 |
|---|---|---|
| 28420ce | #268 | B1 数据层 + 映射表（feat） |
| f3ccb45 | #268 | B1 审查修复 P1-1/P2-1（fix） |
| ea9d802 | #269 | B2 Rust 数据层（feat） |
| 8afe221 | #270 | B3 GUI 渲染（feat） |
| 243e901 | #270 | B3 审查修复 + 对抗套件（fix） |
| 02036b5 | #271 | B4 搜索三路（feat） |
| e8d2f20 | #271 | B4 测试（test） |
| 20066d9 | #272 | B5 docs + 冒烟（docs） |

ref 统计: #268×3 #269×1 #270×3 #271×2 #272×2

## F2 双 agent 审查 ✅

- B1: subagent_review（P1-1 staging 表前置 drop、P2-1 双键防膨胀 → 修复提交 f3ccb45）
- B2: subagent_review（P1-1 is_missing_column 收窄 → 修复并入 B3 前；修改后通过）
- B3: subagent_review（P1-1 SC2 逆映射碰撞 → 修复提交 243e901 + 回归测试 20066d9）
- B3/B4 联合: subagent_review（通过；P2 冲突回归测试 → 20066d9 补）
- PR 级集成审查: subagent_review（进行中）

## F3 测试 + 覆盖率

- Python collectors: 465 passed, 覆盖率 96%（≥95% 门槛 ✅，CI `--cov-fail-under=95`）
- Rust workspace: 1316 passed 全绿（8 批次后）；clippy -D warnings 0、fmt 通过
- Rust llvm-cov: 运行中（阈值：workspace 总 ≥93%、compass-core/compass-data ≥95%、compass ≥90%）

## P1-1 修复（PR 级审查，2026-08-15）

PR 级集成审查发现 concept 节（486 行）未被 import JOIN 消费 → 概念板块/SEPA
主题英文名全链路断裂。用户批准方案 A：`_import_index_basic` 增加 concept 节
按名称 JOIN（COALESCE(symbol 命中, name 命中) 防膨胀）。修复 + 回归测试
（requirement concept 用例 + build_concept_names/build_industry_names 单测）。

概念 name_en 真实数据冒烟：**受限于 index_basic 表不存在**（fetch_index_daily
未运行——真实 Dolt 库无 index_basic），概念链路冒烟依赖下次 `fetch_index_daily`
运行；JOIN 逻辑经 temp Dolt 测试验证（BK0475 半导体 → Semiconductors）。

## F4 scope fidelity ✅（对照 issue #266 验收）

1. ✅ 英文界面指数/行业/概念/主题显示英文；无译名回退中文（display_name + 三元组 + 概念映射；对抗/需求测试锁定）
2. ✅ 中文界面全部中文（display_name 双向 + 测试）
3. ✅ "SSE"→上证指数 三路匹配（B4 测试）；股票 code+name 两路（D0-B 用户裁决）
4. ✅ name_en_mapping.csv 随仓库提交（591 行三节）；import JOIN 全链路（Dolt→parquet→DuckDB）
5. ✅ 全链路测试 + 真实数据冒烟：stock_basic industry_en 填充 5680/5888（208 空行业行除外，非空 100% 命中），Dolt commit+push，parquet 透传验证

## 方案偏差记录

- 验收 3 示例修订（Moutai→SSE 类）：D0-B 用户裁决（issue #266 comment 已追加）
- B3 screener 行业下拉 en 显示：label/value 分离方案（显示 en、存储 zh 键），共享 en label 回退 zh（P1-1 修复）
- Dolt `sql -r parquet` 全 NULL 列 IS NULL 谓词失效（外部工具缺陷）：测试断言改用 DESCRIBE + 值读取；GUI 读取 Option 不受影响
