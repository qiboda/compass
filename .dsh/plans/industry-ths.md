# Plan — industry-ths（issue #283 板块数据源战略调整）

> 状态：待批准（2026-08-15 用户确认全部决策）
> 分支：feat/industry-ths（worktree `.worktrees/industry-ths`）
> Issue: https://github.com/qiboda/compass/issues/283（OPEN，验收标准已更新）

## 已锁定决策（grill-me 2026-08-15，不得偏离）

| # | 决策 | 内容 |
|---|---|---|
| D1 | 行业源 | **同花顺 90 个唯一**（申万一级，881xxx）替代东财 496 自编细分；列表实时抓 `q.10jqka.com.cn/thshy/`（GBK，href 提取） |
| D2 | K 线接口 | `https://d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js`，按年分页（2007→当前年），7 字段 CSV 复用 `_kline_records` |
| D3 | 旧行业数据 | 删除东财 BK 行业行（index_basic 496 名称行，无行情数据；index_daily industry 行本就为 0）。2026-08-16 用户两次确认后定稿：保留会致 picker 双名称重复（如两个半导体），删除不损失行情 |
| D4 | 概念全链路 | `index_*` concept 行、`concept_member` 表、`fetch_concept_member.py`、`ConceptMember` 模型、reader、import/export、GUI 概念段 + SEPA 主题标签（`themes`）、`concept_names` map —— **全部移除** |
| D5 | SEPA 题材 | 题材模块 25% 权重**保留**，数据源改用行业板块（按 `stock_basic.industry` 分组聚合替代 concept_member 分组）；GUI 题材列/题材卡保留 |
| D6 | backtest | `backtest_result` 表删除 `theme_score` 列 |
| D7 | 符号 | BK 前缀扩展 4-6 位数字（`BK881xxx`）；`validate_symbol`/`parse_explicit_prefix`/`exchange_of_symbol` 同步 |
| D8 | 官方指数 | 东财 push2his + 腾讯 fallback 路径**不变**（回归测试锁定） |
| D9 | 快速失败 | 同花顺段复用 #277 连续失败快速终止（`_MAX_CONSECUTIVE_FAILURES`） |

## 已验证事实（2026-08-16 00:00 复测）

- 同花顺 K 线接口实测可用：`quotebridge_v4_line_bk_881121_01_2024({...})` JSONP 包装，`data` 串 `;` 分隔记录
- **字段顺序 `日期,开,高,低,收,量,额` 与东财 `日期,开,收,高,低,量,额` 不同**——`_kline_records` 不能直接复用，需 THS 专用字段映射（handoff「与东财同构」有误；实测 20240102: 开7035.381/高7035.458/低6925.206/收6927.422）
- 列表页 `q.10jqka.com.cn/thshy/`（GBK）href 提取 881xxx 可用
- Dolt 实测：index_basic concept 504（非 45）+ industry 496 + official 30；index_daily concept 2759 + official 145215、**industry 0 行**（东财行业 K 线从未入库）；concept_member 70460；**表名是 `final_score`（含 theme_score 列）非 backtest_result**（issue 命名误差）；`industry_factor` 表列名 concept_name（题材聚合持久化，改行业后需改列名）；残留 `_tmp_name_en` 表（#266 遗留，一并清理）

## 范围外（明确不做）

- `stock_basic` 行业字段（三大交易所官方源）不动——SEPA 行业展示/题材分组数据源
- `screener` 行业下拉（读 stock_basic）不动
- 官方指数白名单、腾讯源、图表/其他 tab 不动

## 批次与 DAG

```
B1 数据清理（Dolt，独立 commit）
   └─ 无依赖；脚本：DELETE concept 行 + DELETE BK 行业行 + DROP concept_member
B2 采集器（Python）
   ├─ fetch_index_daily.py：+同花顺源（列表抓取 + 按年 K 线），−fetch_board_list（东财行业/概念发现整体移除），run() 重构为 official + ths_industry 两段
   ├─ main.py：移除 concept_member 入口
   └─ 删除 fetch_concept_member.py + test_concept_member.py
B3 Rust 数据层（compass-core / compass-data / compass-types）
   ├─ model.rs 移除 ConceptMember；parquet.rs 移除 fetch_concept_member
   ├─ import_compass.rs 移除 CompassTable::ConceptMember + import_concept_member
   ├─ compass-data/src/sepa.rs：backtest_result DDL/导出删除 theme_score 列
   └─ compass-types：SepaRow 移除 themes 字段
B4 SEPA 引擎（compass-strategy）
   ├─ aggregation.rs：aggregate_concept_daily → aggregate_industry_daily（按 industry 字符串分组）
   ├─ scoring.rs：移除 ConceptMember 依赖（members/memberships/themes/best_theme/sector_* 改按 stock_basic.industry 分组）；score_theme 消费行业聚合
   ├─ backtest.rs：SepaRow 适配（themes 移除）
   └─ tests/sepa.rs 适配
B5 GUI（compass / compass-i18n）
   ├─ market.rs：SEGMENT_TYPES [industry, concept, official] → [industry, official]；segment 索引注释；相关测试
   ├─ compass-i18n：移除 index.segment.concept + SEPA 主题标签相关 key（保留 sepa.table.theme / sepa.module.theme）
   ├─ citizens/sepa.rs：移除 themes 标签渲染 + concept_names 参数（row_cells 签名）
   ├─ state.rs / main.rs：移除 concept_names + build_concept_names
   └─ symbol.rs：BK 4-6 位扩展（validate/parse/exchange）
B6 文档同步 + 决策记录（随批次同 commit）
```

## 验证门禁（每批次）

- Python：`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q`
- Rust：`cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
- 冒烟（B2 提交前）：真实拉取同花顺 1-2 个行业按年 K 线 + 列表页解析，验证行数/日期范围/数值合理性
- 数据清理验证：清理脚本执行后 `dolt status` + SELECT 计数（concept 0 行、BK 行业 0 行）

## 测试计划（门禁 3.5/4 步，实现前委派）

- **adversarial（subagent_skwy_adversarial_test）**：
  - Python：同花顺列表页异常（GBK 解码失败/空 href/重复去重 140→90）；按年分页边界（空年响应/年份循环终止）；7 字段脏数据；快速失败在 ths 段生效；concept 逻辑移除后无残留
  - Rust：BK 6 位符号校验（BK881234 合法 / BK881 拒绝 / 注入防护不削弱）；题材行业聚合空 industry 分组；SepaRow.themes 移除后 GUI 不 panic
- **requirement（subagent_skwy_requirement_test）**：
  - Python：同花顺 90 个唯一列表解析；东财失败→（行业已无东财路径）官方指数腾讯 fallback 回归；概念发现移除
  - Rust：market.rs 概念段移除后 Segmented 只有 2 段；SEPA 题材按行业聚合打分；backtest theme_score 列删除；概念数据清理 SQL

## 交付物

- commit 序列（每 commit `ref #283`）：① 数据清理脚本+执行记录 ② 采集器 ③ Rust 数据层 ④ SEPA 引擎 ⑤ GUI ⑥ docs
- 冒烟证据落 `.dsh/evidence/`
- 完成后：review（五角度并行）→ 用户 push → 完成 comment + 关闭 issue #283 → skwy-reflect 反思 commit
