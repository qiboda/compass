# Handoff — fix/index-daily-incremental

## 用途
修复 `collectors/fetch_index_daily.py` 的“非真增量”缺陷：THS 行业板块与官方指数
K 线当前每次同步都会从 2007 年全量拉取再靠 INSERT IGNORE 去重，导致单次 sync
耗时约 1 小时、大量旧年份 404/502/504。目标改为真正的日期增量拉取。

## 对应 Issue
https://github.com/qiboda/compass/issues/292

## 已锁定的 grill-me 决策
1. THS 行业板块改真增量：查 Dolt index_daily 每板块 MAX(trade_date)，
   已有数据板块只拉 MAX 所在年份→今年并过滤旧行；新板块仍 2007→今年全量回填；
   周末/停牌无新行不记为失败。
2. 官方指数也改增量：东财 beg=0 改 beg=上次日期+1；腾讯回退只拉最近页并在遇到
   <= 上次日期时停止翻页。
3. 保留 last_report_date==今天整体短路。
4. 按项目 gate 走 worktree → issue → plan → RED → 实现 → docs → 反思。

## Git 基线
- 分支：fix/index-daily-incremental
- 基点：master（含本地未推送 docs 提交 10e71a2）
- 实现、测试、docs 全部在本 worktree 内完成，PR 合并回 master。
- 同步提醒：开始前如 master 有更新，先 `git fetch origin master && git rebase origin/master`。

## 数据目录
- collectors 写入 Dolt：/data/compass-data/compass_data
- 行情主 Dolt：/data/compass-data/investment_data
- 注：collectors/main.py sync-investment-data 与 scripts/sync-investment-data.sh
  中 investment_data 路径仍写 PROJECT_ROOT/investment_data 已过时（实际在
  /data/compass-data/investment_data）。
