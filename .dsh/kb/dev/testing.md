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
- **集成测试**：`tests/` 目录，按 crate 组织（`cargo test --test <name>`）：
  - `compass-core`：`integration_test`、`requirement_index_duckdb`、`index_duckdb_fallback`、`index_symbol_bk`、`llm`
  - `compass-data`：`requirement_index_import`、`index_import_compass`、`data_quality_adversarial`、`requirement_name_en_data`、`name_en_data_layer_adversarial`
  - `compass-strategy`：`screener`、`screener_engine`、`screener_eval_adversarial`、`sepa`、`sepa_real_smoke`
  - `compass`（GUI）：`requirement_index_market`、`adversarial_219_fork_formats`、`adversarial_245_screener_builder`
  - `compass-types`：`validate_filter`、`adversarial_serde`、`adversarial_247_filter_serde`
  - `compass-ui`：`index_searchable_bk`、`button_230_theme_width`、`adversarial_widget_deviations`
  - `compass-collectors`：无独立 tests/ 目录（单元测试 + `scripts/update-database.sh` 冒烟）
  - `compass-i18n`：无独立 tests/ 目录（单元测试，编译期 KEY_* 常量 + 字典一致性）

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

### result-slot 时序回归（compass backend，ref #276）

`wire_backend` 的 result slot 契约：`*_loading` 可观察为 `false` 时，显示日志必须已写入。
测试用 `Dynamic::lock()` 持有 loading mutex，把 result slot 精确卡在 `*_loading.set(false)`
之前，然后轮询 `log_count() > 0`：

- 旧代码（先清 loading 再写日志）会永久卡在 `set(false)`，日志永不出现 → RED 超时
- 修复后（先写日志再清 loading）日志先出现，测试通过
- 注意：持有 loading guard 期间禁止对该 loading Dynamic 调用 `get()/set()`，避免自死锁；
  只读 log / result-data 等其它 Dynamic

覆盖 fetch / screener / SEPA / index 四个 result slot（`backend.rs` 内
`*_result_slot_writes_log_before_clearing_loading`）。

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
- **UI 问题可用截图/多模态视觉检查辅助定位**：截图可作为证据，但仍须结合形状/像素采样/断言等客观证据交叉验证

### 脚本自测（scripts/tests/，ref #151）

- `test-update-database.sh`：`bash -n` + **mock cargo/dolt**（PATH 前置假命令，
  日志记录调用参数）断言 7 步流水线调用顺序、Dolt `add` 限定表、失败非零退出、
  preflight 分支；数据目录用 `SEPA_COMPASS_DATA_DIR` 等 env 覆盖指向临时目录
- `test-timing-requirements.sh` / `test-timing-adversarial.sh`（issue #334）：
  在 mock 环境下验证同步计时 JSON 生成/schema/collector 事件合并、计时失败仅
  warning 不阻断主流程、失败步骤 `status:"failed"`、run_id 唯一性与特殊字符 JSON
  安全性；运行 `bash scripts/tests/test-timing-requirements.sh` 与
  `bash scripts/tests/test-timing-adversarial.sh`
- `justfile-test.sh` / `justfile-adversarial-test.sh`（ref #265）：justfile 回归测试
  ——需求验收（22 断言：9 recipe 存在性、逐字命令映射、默认 recipe、check 门禁
  顺序、`--fmt --check`）+ 对抗（18 项：命令静默弱化、默认漂移、recipe 集合恰
  9 个、性能、无 justfile 区分性）。只读操作（`just -n`/`--list`/`--fmt --check`），
  禁 cargo 重型命令。手动运行：`bash scripts/tests/justfile-test.sh`

### append/import-compass 增量 merge 防漂移测试（ref #298）

- 测试 schema 必须使用**生产 Dolt 全主键**，不要沿用只覆盖部分列的旧测试常量
  （`block_trade` 旧测试常量曾只有 `PRIMARY KEY (symbol, trade_date, price)`，无法表达
  真实同窄 key 的多条行）。
- 每个 append 表至少一组「同符号/同日期/不同主键后缀」的真实行：先全量导入，再增量
  `--since` 导入新行，断言所有真实行都保留、无静默替换。
- `block_trade` 必须覆盖两类失败形态：
  1. 显式 `row count mismatch`（旧行多于 merge 后行）；
  2. 静默替换历史（old=1, merged=1，count 守卫不触发，但历史行被新行替代）。
- fallback 语义测试：损坏 parquet 强制 DuckDB merge 失败后，断言 fallback 写入的是
  **不带 `--since` 的真全量导出**（保留 since 之前的历史），而不是过滤后的增量数据。
- 统一用 `run(CompassTable::X, ...)` 驱动；Dolt 临时库用真实 `dolt` CLI + 现有
  `setup_dolt` / `dolt_sql` 模式，无 mock。


**委派测试 agent 的自验可信度（ref #265 教训）**：测试 agent 汇报的 self-GREEN
模拟若使用与需求契约**逐字不一致**的 fixture（如契约 `cargo fmt -- --check`、
fixture 却写 `cargo fmt`），自验全绿也不可信——断言可能永远落空。委派时必须
要求：① 自验 fixture 与 issue/plan 契约逐字一致；② 对关键断言做 mutation
负面验证（削弱实现 → 断言必须 FAIL）。主 agent 收到自验报告后，应在真实实现
上重跑一遍两批测试再采信。

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
| `compass-strategy` | `screener_eval` | Screener Filter AST 评估器（issue #246）：合成横截面 + `run_screener` |

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
Rust 侧为 **per-crate 阈值**（按可测试性设定，2026-08-12，ref #250）：
纯逻辑/serde 可测的 crate（compass-core / compass-data / compass-i18n /
compass-strategy / compass-types / compass-ui）95%，GUI 主程序 compass
（事件循环/线程/交互难测）90%，workspace 总 93%（workspace 口径排除
compass-collectors，该 crate 单独设 20% 门槛，2026-08-29, epic #310）：

```sh
# Rust：单次 llvm-cov nextest --json 采集（nextest 语义，与 cargo nextest run 同口径），
# 脚本按 per-crate 阈值表校验（8 门槛 1 次运行）
cargo llvm-cov nextest --json --summary-only --output-path target/llvm-cov/coverage.json
bash scripts/check-coverage.sh target/llvm-cov/coverage.json

```

- Rust 用 `cargo-llvm-cov`（需 `rustup component add llvm-tools`），行覆盖率口径。
- `scripts/check-coverage.sh` 用 jq 解析 llvm-cov JSON，内嵌 per-crate 阈值表
  （compass-core / compass-data / compass-i18n / compass-strategy /
  compass-types / compass-ui → 95，compass → 90，
  compass-collectors → 20，workspace 总 → 93，workspace 排除
  compass-collectors 文件）；
  任一低于各自阈值或未测到文件即退出码 1。
  单次运行而非每条 `-p` 命令，避免 8 次全量测试（约 8x 加速）。
- compass-collectors 是网络/Dolt 子进程密集代码，单元测试只覆盖纯逻辑，
  生产正确性由 `update-database.sh` 冒烟保证；将其从 workspace 总门槛排除、
  单独设 20% 门槛（epic #310，2026-08-29）。
- Python 采集层及其覆盖率门禁已随 epic #310 退役；`scripts/check-coverage.sh`
  不再包含 Python 目标。
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

#### kittest Node API 限制（ref #217）

kittest 查询/断言用的 `Node` 是基于 AccessKit 树的**查询视图**，不是组件
对象——**没有** `label()` / `color()` / `side()` 等方法（曾误用导致编译
失败，ref #217）。可用能力：

- **文本**：`node.value()`（Label 等控件的文本值），不是 `label()`
- **渲染属性**：`harness.output().shapes` 扫描 `Shape::Text`（galley job
  sections 的 `text`/`galley.pos`/`color`）验证颜色/位置；或 `ctx` 查询
  `response.rect` 做渲染级断言（见下）
- **组件自定义属性**（如 Tag pill 的 side）：**不能**从 Node 直接读——
  通过渲染断言（rect/像素）或组件暴露的测试钩子验证

#### egui wrapped 布局陷阱：`Frame::show` 撑宽父级 max_rect（ref #217）

`egui::Frame::show(ui, ...)` 会**撑宽父级 `max_rect`**（把自身 inner
rect 宽度并进父容器可用宽度），破坏 wrapped 换行——Tag 类 pill 组件
曾因此 35 个单行标签溢出 4 倍宽（ref #217）。**正确模式**
（Tag 类 pill，ref #217 落地）：

```rust
// allocate_exact_size + painter 背景 + ui.put Label（保 accesskit）
let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());
ui.painter().rect_filled(rect, rounding, color);   // 背景自己画
ui.put(rect, egui::Label::new(text));              // 文本独立布局
```

`allocate_exact_size` 不给父级撑宽；`ui.put` 在固定 rect 内布局文本，
不参与 wrapped 流式布局。

#### 渲染断言 vs 字段断言（ref #226/#228）

组件**宽度/尺寸/位置语义**用**渲染级断言**（`response.rect.width()`、
`response.rect.min/max`），不用**字段级断言**（如 `side == 32.0`）——
字段断言可能"全绿而渲染错位"（字段值对但布局没按预期生效）。
GUI 冒烟同理：**组件尺寸语义必须以渲染输出为准**，不信任组件内部字段。

#### GUI 冒烟：像素采样法（ref #226/#228）

GUI 冒烟验证可用截图/多模态视觉检查辅助，同时以客观像素证据交叉验证：

```sh
# Wayland: grim；X11: import（注意参数顺序，ref #226）
grim -o <output> screenshot.png        # Wayland
import -window root screenshot.png     # X11

# ImageMagick 直方图像素采样：验证颜色分布/区域颜色
convert screenshot.png -crop WxH+X+Y -colors 5 txt:   # 采样区域主色
convert screenshot.png -format %c -colors 5 histogram:info:
```

- 截图工具链：Wayland 用 `grim`（`xwininfo` 在 Wayland 无输出，ref #226）；
  X11 用 `import`（注意 `-window root` 参数顺序）
- 断言对象是**直方图/像素统计**（区域主色、颜色计数），不是人眼判图
- 视觉模型支持图像输入时，截图可作辅助证据；像素采样等客观证据仍作为最终验证手段

### Rust 采集器测试（epic #310）

`crates/compass-collectors` 的单元测试集中覆盖纯逻辑：CSV 序列化与 BOM、
日期生成/增量锚点、proxy scheme 归一化、financial 共享 fetch/upsert 的参数
组合、CLI 参数校验等。B1–B6 期间的真实双跑/网络级验证以 `dual_run_*.sh`
与 `.dsh/evidence/` 落盘；B7 后该批脚本随 Python 退役一并移除，生产正确性由
`scripts/update-database.sh` 冒烟与 Rust 单元/集成测试保证。

