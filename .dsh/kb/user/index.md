# Compass 用户指南

Compass 是一个**本地优先的 A 股股票图表应用**，配备用于管理历史行情数据的数据管线。

## 功能概览

| 工具 | 用途 |
|---|---|
| **图表应用** (`scripts/run.sh`) | 查看任意 A 股股票的交互式 K 线图 |
| **数据管线** (`compass-data`) | 导入、导出和备份行情数据 |

## 数据机制

Compass 将所有行情数据存储在**本地机器上**。数据导入后，图表即时渲染 — 无需联网、无需 API 密钥、无速率限制。

```
数据源 → 导入 → Parquet 文件 → 图表应用
```

数据进入本地 Parquet 数据库有两种方式：

| 数据源 | 内容 | 适用场景 |
|---|---|---|
| **Dolt** (`investment_data`) | 完整的 A 股日线历史数据（1990 年至今，1800 万+ 行） | 通过 `compass-data import` 批量导入 |
| **东方财富**（在线） | 实时和历史数据，由 Rust 采集器获取 | 获取 Dolt 中尚未存在的数据，再导入 |

Dolt 是**主要**数据源 — 完整、离线、快速。东方财富数据由 Rust 采集器（`crates/compass-collectors`，二进制 `compass-collectors`）获取，先写入 Dolt `compass_data`，再通过 `import-compass` 转为 Parquet（例如 `cargo run --bin compass-data -- import-compass --table stock_basic`）。GUI 本身**仅限本地** — 它从不直接调用东方财富接口。

## 快速开始

```sh
# 1. 从 Dolt 导入全部 A 股历史数据（一次性，约 1 小时）
cargo run --bin compass-data -- import

# 2. 启动图表应用
just                # 或 scripts/run.sh（无 just 环境）

# 输入股票代码（如 600519），点击 Fetch
```

## 前置条件

- **Rust** ≥ 1.85（edition 2024）
- **mold + clang**（Linux）— 编译链接器（`.cargo/config.toml` 硬编码 `/usr/bin/mold`，缺失时编译失败）。Ubuntu: `sudo apt install mold clang`
- **显示服务器**（X11 或 Wayland）用于 GUI
- **Dolt CLI** 用于 `compass-data import`
- **Dolt 数据库** `investment_data/` 作为导入数据源

## 文档地图

| 文档 | 内容 |
|---|---|
| [GUI](gui.md) | 图表应用 — 股票代码输入、时间周期、控件 |
| [CLI](cli.md) | 数据管线 — 导入、导出、备份 |
| [配置](config.md) | `config.toml` — 全部选项与默认值 |

面向开发者：[.dsh/kb/design/](../design/architecture.md) 涵盖系统设计与架构。
