# 测试

## 框架

| Crate | 用途 |
|---|---|
| `rstest` | 参数化测试 + fixtures |
| `httpmock` | HTTP mock 服务器 |
| `tempfile` | 临时文件/目录创建 |

## 测试运行器

```sh
cargo test                              # standard runner
cargo nextest run                       # recommended: faster, better output
cargo test --test integration_test      # integration tests only
```

## 测试组织

- **单元测试**：`#[cfg(test)] mod tests` 位于每个源文件底部。
  测试可以访问私有函数和结构体。
- **集成测试**：`tests/` 目录。仅测试 `compass-core`（library crate）的公开 API。

## 编写测试

### 使用 rstest 编写异步测试

```rust
#[rstest]
#[case("SZ000001", "1d")]
#[case("SH600519", "1w")]
#[tokio::test]
async fn test_name(#[case] symbol: &str, #[case] timeframe: &str) {
    // test body
}
```

顺序很重要：`#[rstest]` 在最外层，`#[tokio::test]` 在最内层。

### 内存 DuckDB

使用 `DuckDbProvider::new_in_memory()` 创建完全隔离的测试数据库：

```rust
let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
// Each call creates a separate in-memory DB — tests never interfere.
```

无需清理 — 数据库在 `provider` 离开作用域时自动释放。

### Dolt（测试数据库）

需要 Dolt 数据库的测试使用 `dolt init` + `dolt sql` 在运行时创建临时、
自包含的数据库。不依赖外部数据。

```rust
let tmp = tempfile::tempdir().expect("create temp dir");

// Set identity for dolt init (uses git underneath)
std::process::Command::new("dolt")
    .arg("config").arg("--global").arg("--add")
    .arg("user.email").arg("test@compass.local")
    .output().expect("dolt config");
std::process::Command::new("dolt")
    .arg("config").arg("--global").arg("--add")
    .arg("user.name").arg("Test")
    .output().expect("dolt config");

// Init and create schema
std::process::Command::new("dolt")
    .arg("--data-dir").arg(tmp.path())
    .arg("init").output().expect("dolt init");

std::process::Command::new("dolt")
    .arg("--data-dir").arg(tmp.path())
    .arg("sql").arg("-q")
    .arg("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)")
    .output().expect("dolt sql");

// Query via run_dolt_sql_parquet / run_dolt_sql_csv
let data = run_dolt_sql_parquet(tmp.path(), "SELECT * FROM t").unwrap();
```

CI 从 GitHub releases 安装 `dolt`。测试通过 `TempDir` 的 drop 自动清理。
`investment_data` 仓库（1800 万+ 行）不会被克隆。

### DuckDB 死锁规避

当编写混合了直接 `db.conn.lock()` 调用和异步 `DuckDbProvider`
方法（内部通过 `spawn_blocking` 加锁）的测试时，将所有直接加锁访问
归入 ONE 作用域内，放在任何异步 `db` 方法调用之前：

```rust
// SAFE: all direct conn access before any async db calls
let (count_a, count_b) = {
    let conn = db.conn.lock().expect("lock");
    let c1 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE symbol='SZ000001'", ...)?;
    let c2 = conn.query_row("SELECT COUNT(*) FROM stock_daily WHERE symbol='SH600519'", ...)?;
    (c1, c2)
}; // lock released

// Now safe to call async db methods
let range = db.get_stored_range("SZ000001").await?;
```

`DuckDbProvider` 的异步方法使用 `spawn_blocking`，它在线程池上尝试锁定 `conn`。
如果你在外层作用域持有 `conn.lock()`，然后调用异步 `db` 方法，spawn 的任务
会被阻塞，等待你已持有的锁 — 死锁。

## 测试模式

1. **Provider 隔离**：每个测试用例创建新的 provider。不要共享。
2. **断言隔离**：保存后，用不同的 symbol/timeframe 提取数据来
   验证不存在交叉污染。
3. **参数化**：使用 `#[case]` 实现相同逻辑、不同输入。
4. **错误路径**：测试空数据、错误 JSON、文件缺失。
5. **集成测试**：使用内存 DuckDB 运行完整管线
   （import → save stock_daily → fetch bars → verify counts）。

### SEPA 引擎（compass-strategy `mod sepa`，ref #147-#149）

- **纯函数指标**：`ma/atr20/momentum_return/volume_ratio/rs_score/vcp_score/
  drawdown_from_high` 均为 `&[&CrossSectionBar]` 切片风格，窗口不足返回 `None`/0
  不 panic；fixture 用内存 `TestBar` 序列（rising/falling/flat 模式）
- **VCP 区分度测试**：构造典型 20%→10%→5% 回撤收敛序列（≥0.7）vs 无收敛噪声
  序列（<0.3），断言两者分差——锁定形态识别的区分能力
- **温度计三场景 fixture**：牛市（全 >MA250 + 涨停≥80 + 高上涨占比）→ ≥80 /
  "80%-100%"；熊市 → <60 / "0%-20%"；结构行情（指数弱板块强）→ 60-80 / "40%-70%"
- **评分排序 fixture**：强趋势+热门题材股 vs 垃圾股 → 断言总分排序；题材满分恒 25
  （有/无 news 两场景，分母恒 90）；风险最差恰 −3.75（不越界）
- **真实数据冒烟**：`tests/sepa_real_smoke.rs`（`#[ignore]`，`SEPA_PARQUET_DIR`
  覆盖）读真实 Parquet 跑 `run_sepa`，断言分数/温度计区间合理——CI 不跑，F3 门手动

### egui_kittest 面板测试（compass crate，ref #152）

- **无头渲染/交互**：`Harness::new_ui` + `run()` 多帧 + `query_by_label` 断言渲染；
  点击刷新按钮 → `sepa_loading` 置位 → 注入响应 → 表格填充
- **双 tab 视觉断言**：Chart+Sepa 双 tab leaf → 扫描 `output.shapes` 断言激活 tab
  与未激活 tab 的样式形状差异（accent vs text_secondary），**禁目测**
- **`score_color` 单测**：色阶端点/边界/中点 lerp/单调性
- **UI 问题一律客观证据定位**（形状/像素采样/断言），不靠"看起来对不对"猜

### 脚本自测（scripts/tests/，ref #151）

- `test-sepa-daily.sh`：`bash -n` + **mock cargo/uv/dolt**（PATH 前置假命令，
  日志记录调用参数）断言 7 步流水线调用顺序、Dolt `add` 限定表、失败非零退出、
  preflight 分支；数据目录用 `SEPA_COMPASS_DATA_DIR` 等 env 覆盖指向临时目录

## 基准测试

性能基准测试使用 [criterion.rs](https://github.com/bheisler/criterion.rs)，
位于每个 crate 的 `benches/` 目录下。

### 运行

```sh
cargo bench                       # all benchmarks (slow — ~hours for full suite)
cargo bench --bench parquet_bench # specific benchmark
cargo bench -- --quick            # quick run (fewer samples, for development)
cargo bench --no-run              # CI: compile only, don't execute
```

结果写入 `target/criterion/`，以 HTML 报告形式呈现。

### 可用基准测试

| Crate | Bench 文件 | 测量内容 |
|---|---|---|
| `compass-core` | `parquet_bench` | ParquetReader 冷/热读取，100/1000/5000 行，真实 SZ000001 |
| `compass-core` | `duckdb_bench` | DuckDbProvider 缓存命中/未命中，保存吞吐量（10–5000 行） |
| `compass-data` | `dolt_bench` | Dolt sql -r parquet 单文件导出、符号枚举 |

### 数据需求

- **Parquet 基准测试**：需要包含真实数据的 `parquet_data/`，或通过内存 DuckDB 生成合成数据
- **Dolt 基准测试**：需要 `investment_data/` 目录和 PATH 上的 `dolt` CLI；缺失时优雅跳过
- **其他所有基准测试**：使用内存 DuckDB 或临时目录 — 无外部依赖

### CI 策略

CI 运行 `cargo bench --no-run` 以验证编译。基准测试**不会**在 CI 中执行 —
CI 环境变量过多，无法产生有意义的性能数据。
在性能敏感变更前后，在本地运行基准测试。

### 保存与对比基线

基准测试结果保存到 `bench_results/<version>/` 以实现版本化追踪：

```sh
# Save a full baseline (auto-generates timestamp-based version)
scripts/bench-save.sh

# Save with explicit version
scripts/bench-save.sh v1.0

# Quick run (fewer samples, faster)
scripts/bench-save.sh v2.0 quick

# Compare current code against a previous baseline
cargo bench -- --baseline v1.0
```

脚本运行 `cargo bench -- --save-baseline <version>`，然后将结果
从 `target/criterion/` 复制到 `bench_results/<version>/`，
使其脱离构建缓存。

## 性能分析（Tracy）

Compass 通过 `tracing-tracy` crate 支持 [Tracy profiler](https://github.com/wolfpld/tracy)。
Tracy 提供实时、纳秒级精度的 CPU 性能分析，带有 flamegraph 可视化。

### 设置

1. 从 [GitHub Releases](https://github.com/wolfpld/tracy/releases) 安装 Tracy profiler server，
   或从源码构建。你需要 `tracy-capture`（或 `tracy-profiler`）二进制文件。

2. 运行 Tracy 捕获服务器：
   ```sh
   tracy-capture -o compass.tracy
   ```
   这会打开 Tracy GUI。默认监听 `localhost:8086`。

3. 启用 `tracy` feature 运行 Compass：
   ```sh
   cargo run --bin compass --features tracy
   # or: cargo run --bin compass-data --features tracy -- import --symbols SH600519
   ```

### 工作原理

- 所有 `tracing` spans（来自 `#[instrument]` 和 `#[tracing::instrument]` 宏）
  会自动转换为 Tracy zones — 无需额外的插桩。
- 当 Tracy 未运行时，该 layer 静默地变为空操作。
- 当 `tracy` feature 在编译时未启用时，整个依赖树被剪枝 —
  零运行时和编译时开销。
- 正常使用时不加 `--features tracy` 构建。仅在性能分析时启用。

### 故障排除

| 症状 | 原因 | 修复方法 |
|------|------|----------|
| `cargo build --features tracy` 失败 | 缺少 C++ 工具链或 cmake | `sudo apt install cmake build-essential` |
| Tracy GUI 中没有数据出现 | 防火墙阻止了 8086 端口 | 检查 `tracy-capture` 是否在同一台机器上运行 |
| 链接错误：符号未找到 | `tracy-client-sys` 版本与已安装的 Tracy 不匹配 | 使用与 `tracy-client-sys 0.24` 匹配的 `tracy-capture` 版本 |

## 覆盖率

### 门槛（CI 强制）

CI coverage job 强制以下行覆盖率门槛，低于阈值退出码 1（CI 失败）。
Rust 侧为 **per-crate 阈值**：数据层（compass-core / compass-data）95%，其余 crate 与
workspace 总 80%（2026-08-04，ref #163）：

```sh
# Rust：单次 llvm-cov nextest --json 采集（nextest 语义，与 cargo nextest run 同口径），
# 脚本按 per-crate 阈值表校验（7 门槛 1 次运行）
cargo llvm-cov nextest --json --summary-only --output-path target/llvm-cov/coverage.json
bash scripts/check-coverage.sh target/llvm-cov/coverage.json

# Python
cd collectors && uv run pytest tests/ --cov=. --cov-fail-under=95
```

- Rust 用 `cargo-llvm-cov`（需 `rustup component add llvm-tools`），行覆盖率口径。
- `scripts/check-coverage.sh` 用 jq 解析 llvm-cov JSON，内嵌 per-crate 阈值表
  （compass-core / compass-data → 95，compass / compass-strategy / compass-types /
  compass-ui → 80，workspace 总 → 80）；任一低于各自阈值或未测到文件即退出码 1。
  单次运行而非每条 `-p` 命令，避免 7 次全量测试（约 7x 加速）。
- Python 用 `pytest-cov`，`--cov=.` **全量计入**所有 `collectors/*.py`
  （`[tool.coverage] omit = ["tests/*"]`），未测文件按 0% 计；`--cov-fail-under=95`。
- coverage job 用 `cargo llvm-cov nextest`——**一步完成 nextest 跑测试 + 覆盖率采集**
  （自 2026-08-08，ref #181 修复 CI 覆盖率漂移后；此前为 `cargo nextest run` + 裸
  `cargo llvm-cov` 分离两步，llvm-cov 内部用 cargo test 语义造成跑两遍 + 与本地
  nextest 验证口径不一致的 ~0.2pp 漂移）。
- 本地测量：`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json`。

### GUI 无头集成测试（egui_kittest）

`compass` crate 的 UI 测试用官方 `egui_kittest`（dev-dependency，`features = ["eframe"]`）；
`compass-ui` 同样以 egui_kittest 为 dev-dependency（组件/widget 测试，S4+ 落地）。
注意：egui_kittest 的 `eframe` feature 以 `default-features = false` 引入 eframe，
而 `eframe::run_native_ext` 被 cfg 门控在默认 `wgpu` feature 链之后——因此
compass-ui 的 dev-dependencies 必须同时包含 `eframe`（默认 features）才能编译 kittest。

- `Harness::new_ui(|ui| ...)` 直接驱动 `Fn(&mut egui::Ui)` —— 纯 CPU，**无需显示服务器**，CI 无头环境可跑。
- `Harness::new_eframe` + `eframe::Frame::_new_kittest()` 驱动完整 `eframe::App::ui`（CompassApp）。
- `get_by_label` / `Node::click` / `type_text` / `harness.run()` 模拟交互，基于 AccessKit 树查询。
- **时间敏感陷阱（重要）**：`Instant::now()` 是**真实墙钟**，而 kittest 的
  `ctx.input(|i| i.time)` 是 **egui 虚拟时间**——`RawInput.time` 恒为 `None`
  （default），`InputState::begin_pass` 用 `self.time + predicted_dt` 累积，
  每 `step()` 推进 `step_dt`（默认 0.25s，可用 `Harness::builder().with_step_dt(x)`
  覆盖）。因此：
  - 若产品代码用 `Instant::now()` 驱动动画，慢 CI 上 `harness.run()` 的
    `wait_for_images` sleep（字体首载）可能让真实时间越过动画时长，导致
    状态提前推进、测试偶发失败（ref #155/#168）。
   - **正确模式**：产品动画用 egui 虚拟时间（`ctx.input(|i| i.time)`），
     测试用细粒度 `step_dt`（如 0.01s）+ `run_steps(n)` 精确跨过动画时长，
     完全确定、与机器负载无关（toast/modal 动画即此模式，ref #168/#171）。
   - 避免"重置时间戳为 `Instant::now()` 再 run()"的 workaround——有残留竞态。
     toast/modal 的全部实例已随 #168/#171 移除，库内再无该模式。
- **限制**：egui_dock 0.20 tab 按钮不暴露 AccessKit label（raw `ui.interact` + TextShape），无法 `get_by_label` 定位 —— tab 切换测试用程序化 `DockState::set_active_tab`，断言 tab 内容 widget。

### Python 网络 mock（stub AsyncSession）

EastMoney collector 测试用 `tests/conftest.py` 的 `make_stub_session` fixture（手写 stub，不用 respx —— curl-cffi 不被 respx/responses 支持）：

- `async get(url, params, headers)` 返回 canned JSON / 注入 429 / 异常。
- 实现 `async __aenter__/__aexit__`（`main()` 用 `async with AsyncSession(...)`）。
- 所有 fetch 函数 `session` 均为参数，注入即通，无需真实网络。
