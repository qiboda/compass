# Handoff — industry-ths（#283 板块数据源战略调整）

## 用途

板块数据源战略调整（用户 2026-08-15 决策，grill-me 共识）：
1. **行业板块改用同花顺 90 个**（申万标准体系，代码 881xxx）替代东财 496 自编细分
2. **彻底放弃概念板块（全链路）**：删除 Dolt 中全部 concept 行 + concept_member 表/采集/导入 + GUI 大盘 tab 概念段 + SEPA 主题标签
3. **SEPA 题材模块改用行业板块数据**（stock_basic.industry 分组聚合，25% 权重保留）；backtest_result 删除 theme_score 列
4. 官方指数维持腾讯源（#278 已实现，勿动）

> 2026-08-15 23:58 更新：grill-me 延续锁定 D3（BK 行业历史数据一并删除）、D4（SEPA 概念全移除）、D5（题材改行业板块聚合）、D6（theme_score 列删除）、D7（BK 前缀 4-6 位）、D8（thshy 实时抓取）。Plan 已批准：`.dsh/plans/industry-ths.md`（B1 数据清理 → B2 采集器 → B3 Rust 数据层 → B4 SEPA 引擎 → B5 GUI → B6 docs）。issue #283 验收标准已更新。

**Issue**: https://github.com/qiboda/compass/issues/283（OPEN）
**分支**: feat/industry-ths（基于 master 3fd7248）
**原始分支**: master —— 启动后先 `git fetch origin master && git rebase origin/master`

## 已锁定决策（不得偏离）

| 决策 | 内容 |
|---|---|
| 行业源 | 同花顺 90 个唯一（页面 140 行含 50 重复，去重后 90），申万风格命名 |
| K 线接口 | `https://d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js`，按年分页（2007→2026 ~20 请求/板块），已验证 2015/2024 数据完整 |
| K 线格式 | `日期,开,高,低,收,量,额` 7 字段（与东财同构，复用 `_kline_records`） |
| 概念板块 | 彻底删除：index_basic/index_daily 中 concept 行全删，GUI 概念段移除，不再采集 |
| 快速失败 | 同花顺段受 #277 连续失败快速终止保护（复用计数器） |
| 官方指数 | 腾讯源路径不变（回归测试锁定） |

## 已验证事实（2026-08-15 实测）

- 同花顺行业列表：`https://q.10jqka.com.cn/thshy/`（GBK），href 提取 `881xxx 名称`，90 唯一
- 行业 detail clid = 881xxx 本身（无需额外抓取）
- 概念板块列表代码 30xxxx → detail clid 886xxx（本任务不再需要概念）
- 公共集合：同花顺 90 vs 东财 496 = 精确同名 56 + 模糊 30 + 独有 4
- akshare 参考实现：`stock_board_industry_hist_ths`（按年循环 + demjson 解析 data 串）
- 注意：东财行业带 Ⅱ/Ⅲ 后缀（申万分级），同花顺只有一级——两者是不同粒度体系，采集同花顺 90 个即可，不需要匹配东财名称

## 验收标准（issue #283）

1. `fetch_index_daily.py` 新增同花顺行业源（90 个 881xxx 全量按年拉取），东财失败自动切同花顺；移除概念发现逻辑（fetch_board_list t:3 部分）
2. 数据清理：Dolt `index_daily`/`index_basic` 删除全部 `index_type='concept'` 行；GUI 大盘 tab 移除概念 Segmented 段与相关 i18n
3. 官方指数腾讯源路径回归测试保持绿
4. 同花顺段受快速失败保护
5. 测试覆盖：同花顺解析/按年分页/东财失败切换/概念段移除/数据清理
6. 全套件绿：`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` + `cargo test`

## 下一步（worktree 会话）

1. 同步原始分支（fetch + rebase origin/master）
2. PRE-IMPLEMENTATION GATE：Design（GUI 概念段移除属界面变更，委派 subagent_ui_designer 评估方案——或判定为删除型变更直接列改动清单给用户确认）→ 3.5/4 委派测试子代理 RED → 实现 GREEN → commit（ref #283）→ review → 用户 push
3. 文档同步（5b）：`.dsh/kb/design/data-providers.md`（同花顺源 schema）、`.dsh/kb/design/symbols.md`（BK 符号与概念段移除）、`.dsh/kb/user/gui.md`（大盘 tab 变更）、`.dsh/kb/design/ui.md`（概念段移除）、toolchain.md（如需）；5c 决策记录补充
4. 数据清理脚本（Dolt DELETE concept 行）与采集实施分离提交
