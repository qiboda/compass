# 产品路线图

## 产品愿景

Compass 是一款 **local-first A-share 股票图表桌面应用**。核心理念：数据本地化，图表即时渲染。

- 所有 OHLCV 数据下载并缓存在本地（Dolt → Parquet），无网络依赖
- 图表操作（缩放、平移、十字光标）零延迟，不经过任何远程服务
- 支持 6000+ A 股标的，覆盖沪深北三市
- 数据管线自动化：EastMoney API → Dolt → Parquet → DuckDB 查询

目标用户：需要在本地快速浏览、分析 A 股历史数据的投资者和研究者。

## 当前 Sprint

Sprint 通过 GitHub Milestones 管理，每周一期（周一～周日，周末为核心开发窗口）。

- **周一**：product agent 自动扫描代码库和 open issues，提出 3-5 个 milestone 候选
- **周日**：回顾完成情况，close 已完成的 milestone
- 手动触发：`/product brainstorm` 随时获取候补需求

## 已完成

- Dolt `investment_data` 导入管线（`compass-data import`）
- 单文件 `stock_daily.parquet` 存储（带 `symbol` 列，参数绑定查询）
- DuckDB in-memory + Parquet-backed 查询
- egui 蜡烛图（candlestick）+ 缩放、平移、十字光标
- 股票搜索（searchable dropdown）
- 多标签页布局（egui_dock）
- 日志面板（LoggerPanel）
- 配置文件支持（`~/.config/compass/config.toml`）

## 规划中

由 GitHub Milestones 和 product agent 动态生成。参见 `.omo/plans/` 中的具体功能计划。
