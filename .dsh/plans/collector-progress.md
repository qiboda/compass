# Plan — collector-progress: 6 个一次性写 CSV 的 collector 抓取进度可查询

> **Issue**: #267
> **Worktree**: .worktrees/collector-progress (feat/collector-progress)
> **状态**: in_progress

## 动机

6 个「全部抓取完成后一次性写 CSV」的 collector（main_flow/block_trade/index_daily/
institution_survey/concept_member/dragon）抓取耗时长（节流 0.5s+/迭代，dragon/index_daily
数百次迭代），运行期间无法得知进展。目标：抓取期间另一终端 `main.py progress` 实时可查，
CSV 保持一次性写入语义。

## 已锁定决策（grill-me，用户确认）

1. 需求场景 = 上述 6 个一次性写 CSV 的 collector，**全部**接入
2. 方案 = Progress 类 + `csv_dir()/<name>.progress.json`（原子写）+ `main.py progress` 查询命令
3. 范围外：append 型 collector（income/balance_sheet/cash_flow/fin_indicators）与
   2 个 stock_basic 采集器**不**接入

## 方向评估（2026-08-15，用户质疑后重新审视）

**结论：半成品方向正确，无需重写**。证据：
- 原子写（tmp+os.replace）→ 读进程无撕裂 ✅
- 6 collector 统一 `with Progress(...)`，动态 total、早退 finish ✅
- 测试 457 passed（Progress 5 测 + dispatch_progress 8 测 + 各 collector finish 断言）✅
- ⚠️ 缺口 1：`progress` 命令 target choices 硬编码 11 个名字（含 5 个未接入者）
- ⚠️ 缺口 2：`.dsh/kb/user/cli.md` 缺 `progress` 命令文档

## Tasks

| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #267 | 批次1: 创建 issue（A-Data, C-Feature） | — |
| in_progress | #267 | 批次2: adversarial + requirement 独立测试（RED，含 choices 失败测试） | — |
| pending | #267 | 批次3: 实现修补（progress choices 收敛为 6 个接入者）+ cli.md doc-sync | 批次2 |
| pending | #267 | 批次4: 分步 commit（Progress 类 → 6 collector 接入 → progress 子命令 → choices 修复 → docs） | 批次3 |
| pending | #267 | 批次5: review（5 角度并行）→ 修复 ≤2 轮 | 批次4 |
| pending | #267 | 批次6: 用户确认 push → 反思 → push → PR → issue 收尾 | 批次5 |

## 验证

- 每批 `cd collectors && uv run pytest tests/ -q` 全绿（基线 457 + 新增）
- 全程无网络请求（StubResponse / conftest 隔离 COMPASS_CSV_DIR）
- 真实数据冒烟由用户后续运行抓取时验证

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 进度存储形态 | JSON 文件 / SQLite / 日志行 | `csv_dir()/<name>.progress.json` 原子写 | 轻量、零依赖、跨进程可读、与 CSV 同目录便于排查 | SQLite 过重；日志行无结构化查询 |
| 进度更新粒度 | 每迭代全量写 / 节流写 | 每迭代原子写 | 迭代频率 = 请求频率（0.5s+），写盘开销可忽略；实现最简单 | 节流引入状态复杂度 |
| 查询命令 | `main.py progress` 子命令 / 独立脚本 | `main.py progress [target] [--json]` | 复用现有 CLI 入口与 argparse | 独立脚本需额外入口 |
| 完成后文件保留 | 保留 / 删除 | 保留（status=completed/failed） | 可复查上次运行结果 | 删除则丢失失败诊断信息 |
| progress target 范围 | 11 个全量 / 6 个接入者 | 6 个接入者（本 PR 修补） | 未接入的 target 查询必失败，choices 收敛到真实有效值 | 全量 choices 误导用户 |
| 覆盖范围 | 仅 6 个一次性写 collector / 含 append 型 | 仅 6 个 | append 型增量追加无"整体进度"语义（用户确认） | 范围扩张无收益 |
