# Handoff — fix/index-daily-tencent-default

## 用途
修复 issue #354：`index_daily` 官方指数采集/回补默认使用腾讯接口，东财仅作为失败后的备用。

- Issue: https://github.com/qiboda/compass/issues/354
- Worktree: `/data/codes/compass/.worktrees/fix-index-daily-tencent-default`
- Branch: `fix/index-daily-tencent-default`
- Base: master @ `c9a55a6`（创建时 == origin/master）

## Grill 锁定决策
用户最初说“修复 354 直接排除东财的接口，默认就使用腾讯的”；在 grill 确认时选择：
**保留东财作为备用**。因此最终契约是：

1. **daily `run()`**：官方指数默认先走 `fetch_tencent_kline`；腾讯失败/为空时再 fallback `fetch_kline`（EastMoney）。
2. **`backfill()`**：官方指数也必须走 Tencent 优先；Tencent 失败/为空时 fallback EastMoney；两者都失败则报错（不静默）。
3. **`probe_official`**：一并改为 Tencent 优先 + EastMoney fallback（保持 CLI 诊断可用）。
4. **不删除东财代码**：`fetch_kline` / `KLINE_HOSTS` / `PUSH2HIS` 等保留，只调整调用顺序与 fallback 语义。
5. `SOURCE` 常量应更新为反映“Tencent 主 + EastMoney 备用”（例如 `Tencent kline + EastMoney fallback + THS industry kline`）。
6. THS 行业板块逻辑不变（仍是 THS 10jqka + proxy；东财不涉及）。
7. 所有路径禁止因东财不可达而失败：只要腾讯成功，整条管线就应成功。

## 关键代码位置（基于 master c9a55a6）
- `crates/compass-collectors/src/index_daily.rs`
  - 常量：`PUSH2HIS`（line 30）、`KLINE_HOSTS`（line 35）、`TENCENT_KLINE_URL`（line 36）、`TENCENT_PAGE_SIZE=2000`（line 37）、`TENCENT_MAX_PAGES=10`（line 38）、`SOURCE`（line 28）
  - `fetch_kline`（line 451，EastMoney）
  - `fetch_tencent_kline`（line 560，Tencent；支持 `last_date=None` 拉全历史）
  - `tencent_code`（line 507）
  - `probe_official`（line 431，当前 EastMoney）
  - daily `run()` 官方指数段（约 line 850-950，当前 EastMoney 先，Tencent 后）
  - `backfill()`（line 1080-1168，官方指数当前只调 `fetch_kline(..., None)`，无 Tencent fallback）
  - 测试模块（line 1170 起）
- `crates/compass-collectors/src/main.rs`：`probe_official` CLI 入口（line 454）。
- 相关文档：`.dsh/kb/dev/toolchain.md` 已有 #354 排查卡（`b40e57b`）；修复完成后应更新为已修复状态。

## 流程提醒
- 进入 worktree 后先 `git fetch origin master && git rebase origin/master` 再开始。
- 走 PRE-IMPLEMENTATION GATE：plan（`.dsh/plans/*.md`）→ adversarial tests RED → requirement tests RED → 实现 GREEN → 文档同步（toolchain/data-providers 决策记录等）→ 真实冒烟（官方指数 daily/backfill 腾讯成功、东财失败不再阻断）→ review → PR。
- 每个 commit 独立行 `ref #354`。
- push 前按流程写反思；push 后补完成 comment 并关闭 issue #354。
- 不 export DuckDB；不自动 push 本地 master（worktree 会话按协议处理 PR）。
