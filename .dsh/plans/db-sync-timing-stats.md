# Plan: 同步数据库性能（用时）统计

Issue: [#334](https://github.com/qiboda/compass/issues/334) — feat: 同步数据库性能用时统计

## 目标

为每日数据管线 `scripts/update-database.sh` 增加全链路计时：
- shell 步骤级（step 0~8）与总时长；
- `compass-collectors sync` 内每个采集器来源的 fetch/import 阶段耗时；
- 每次运行输出人类可读摘要 + 本地 JSON 文件，不写 Dolt、不输出 CSV。

## 非目标

- 不深入 `compass-data` 子命令内部计时（shell 步骤级总耗时已覆盖，源码已有 `elapsed_ms` 日志保留）。
- 不改变数据更新主流程的成功/失败语义；计时失败不阻断主流程，但必须 warning 可见。
- 不 export DuckDB。

## 契约（JSONL 事件 + 最终 JSON）

### 环境变量

| 变量 | 用途 |
|---|---|
| `COMPASS_TIMING_FILE` | 传给子进程（Rust 采集器）的 JSONL 事件文件路径；shell 自己也向同一文件追加步骤事件 |
| `SYNC_TIMING_DIR` | 最终 JSON 输出目录，缺省 `$PROJECT_ROOT/logs/sync-timings` |

### JSONL 事件行

Shell 步骤事件：
```json
{"kind":"step","step":0,"name":"sync investment_data upstream","status":"success","duration_ms":1234}
```

Rust 采集器事件：
```json
{"kind":"collector","source":"stock_basic","phase":"fetch","status":"success","duration_ms":5678}
{"kind":"collector","source":"stock_basic","phase":"import","status":"success","duration_ms":9012}
```

`status` 取值：`success` / `failed`。失败步骤也记录（便于优化分析）；失败仍由原主流程硬失败逻辑决定退出码，计时不吞错。

### 最终 JSON

输出路径：`$SYNC_TIMING_DIR/YYYY-MM-DD-<run_id>.json`，其中
`run_id=YYYYMMDD-HHMMSS-<pid>`。

```json
{
  "schema_version": 1,
  "run": {
    "id": "20260829-141500-12345",
    "date": "2026-08-29",
    "started_at": "2026-08-29T14:15:00+08:00",
    "finished_at": "2026-08-29T14:20:00+08:00",
    "total_ms": 300000,
    "status": "success"
  },
  "steps": [
    {"step":0,"name":"sync investment_data upstream","status":"success","duration_ms":1234},
    {"step":1,"name":"import market data (investment_data → Parquet)","status":"success","duration_ms":50000}
  ],
  "collectors": [
    {"source":"stock_basic","phase":"fetch","status":"success","duration_ms":5678},
    {"source":"stock_basic","phase":"import","status":"success","duration_ms":9012}
  ],
  "summary": {
    "steps_total_ms": 300000,
    "fetch_total_ms": 100000,
    "import_total_ms": 150000
  }
}
```

### 失败行为

- Rust 写 JSONL 失败：stderr warning，继续 sync（不改变退出码）。
- Shell 最终 merge/写文件失败：stderr warning，不改变主流程退出码。
- 主流程某个步骤失败：该步骤记录 `status:"failed"` 后仍按原逻辑 `exit 1`；EXIT trap 仍尝试写出已收集的 timing 文件。

## 实现任务

### Batch 1 — Rust 采集器计时

- 新增 `crates/compass-collectors/src/timing.rs`：
  - `TimingEvent` serde 结构（kind/source/phase/status/duration_ms 等）；
  - `TimingWriter`：从 `COMPASS_TIMING_FILE` 读取路径，None 时 no-op；
  - `record(source, phase, duration)` 追加一行 JSONL；写失败返回 `Err` 给调用方，调用方只 warning。
- 修改 `crates/compass-collectors/src/orchestrate.rs` 的 `sync()`：
  - 对每个来源的 fetch/import 调用包 `Instant` 计时并 `TimingWriter::record`；
  - 来源/阶段命名：`stock_basic.fetch/import`, `fin_indicators.fetch/import`, `balance_sheet.fetch/import`, `income.fetch/import`, `cash_flow.fetch/import`, `dragon.fetch/import`, `block_trade.fetch/import`, `institution_survey.fetch/import`, `main_flow.fetch/import`, `index_daily.fetch`, `index_basic.import`, `index_daily.import`。
  - `progress` / `fetch` / `import` / `backfill` 等子命令不强制计时（本次只覆盖 `sync` 主路径）。
- 新增/更新 Rust 单测：`TimingWriter` 正常追加、无 env no-op、写失败 warning 路径（如有可能）。

### Batch 2 — Shell 步骤级计时 + 汇总

- 修改 `scripts/update-database.sh`：
  - 初始化：`RUN_ID`、`SYNC_TIMING_DIR`、`TIMING_JSONL`、`FINAL_JSON`；创建输出目录；`export COMPASS_TIMING_FILE="$TIMING_JSONL"`；注册 EXIT trap 调用 `finalize_timing`。
  - `run_step()` 增加 start/end 计时，向 `$TIMING_JSONL` 追加 step 事件；失败时仍记录 `failed` 再原样退出。
  - 未走 `run_step` 的步骤（step 1b / 4b / 5 / dolt_commit_changed 等）同样用计时包装，保证 step 0~8 全部出现在 `steps` 数组。
  - `finalize_timing()`：用 `jq -s` 聚合 JSONL 中的 step/collector 事件，生成最终 JSON（原子写：临时文件 + mv），并打印人类可读摘要；任何失败仅 warning。
- 更新 `scripts/tests/test-update-database.sh`：
  - mock cargo 在 `sync` 调用时向 `$COMPASS_TIMING_FILE` 追加一条 fake collector JSONL；
  - 断言最终 JSON 生成、包含 run/steps/collectors/summary、跑完 exit 0；
  - 断言 timing 失败（如 `SYNC_TIMING_DIR` 指向不可写路径/坏 JSONL）时主流程仍 exit 0 且 stderr 有 warning。
- 更新/新增 adversarial 测试（`scripts/tests/test-update-database-adversarial.sh` 或新文件）：
  - 强制要求：计时为附加能力，不得阻断任何主步骤；
  - 失败步骤仍记录 `status:"failed"` 且主脚本仍退出非零。

### Batch 3 — 文档同步

按 AGENTS.md 映射表更新：
- `.dsh/kb/user/cli.md`：新增「同步用时统计」小节（环境变量、JSON 路径、摘要输出）。
- `.dsh/kb/design/architecture.md`：管线章节补充计时机制，`## 决策记录` 增加本次设计决策行。
- `.dsh/kb/dev/database.md`：run 统计文件位置/用途说明（简短）。
- `.dsh/kb/dev/testing.md`（如测试模式有新增则补充 shell 自测说明）。

### Batch 4 — 实现后验证

1. `cargo test`（compass-collectors + workspace 相关）、`cargo clippy -- -D warnings`、`cargo fmt --check`。
2. `bash scripts/tests/test-update-database.sh` + adversarial shell tests。
3. 真实数据冒烟（至少 `compass-collectors sync` 短路径或 `COMPASS_AUTO_HEAL=0`，确认 `COMPASS_TIMING_FILE` 生成、最终 JSON 合理、主流程无回归；视环境允许再跑完整 `update-database.sh`）。
4. commit → subagent_review → 修 review 反馈（最多 2 轮）→ 用户确认 push 前写反思 commit → push → PR。

## 需要用户确认的契约点

1. **JSON schema**（上面的最终 JSON 结构）是否可接受？
2. **失败步骤也记录**（`status:"failed"`，但主流程仍硬失败）是否符合预期？
3. **文件路径**：`logs/sync-timings/YYYY-MM-DD-<run_id>.json`，`SYNC_TIMING_DIR` 可覆盖 —— 是否可接受？

## 验收定义（Definition of Done）

- [ ] `update-database.sh` 每次运行生成一个 JSON timing 文件，包含 run 元信息、steps、collectors、summary。
- [ ] `compass-collectors sync` 在设置 `COMPASS_TIMING_FILE` 时上报每个来源 fetch/import 耗时。
- [ ] 控制台打印人类可读计时摘要。
- [ ] 计时失败仅 warning，不阻断数据管线，且错误可见。
- [ ] 上述文档同步完成；相关 design 决策记录补齐。
- [ ] 测试（Rust + shell 常规 + adversarial）全部通过；真实数据冒烟完成。
