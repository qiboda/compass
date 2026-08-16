# Handoff — proxy-pool-trial

## 用途

试用 proxy_pool（jhao104/proxy_pool）作为现有 collectors 的代理方案：
先用独立验证脚本确认免费代理能否通过 THS 板块接口（10jqka），
再决定是否接入 `fetch_index_daily.py` 的 THS 板块部分。

**Issue**: https://github.com/qiboda/compass/issues/287
**Plan**: `.dsh/plans/proxy-pool-trial.md`

## 已锁定决策（grill-me 共识）

1. 目标：给现有 collectors 增加代理能力，先验证 proxy_pool 是否可用。
2. 部署：proxy_pool + Redis 用 Docker Compose 跑在本机（本机有 Docker，无 Redis）。
3. 先做独立验证脚本，不改现有 collectors。
4. 验证客户端：`curl_cffi`（与 collectors 相同的 TLS 指纹）。
5. 验证目标：THS 行业列表页 `https://q.10jqka.com.cn/thshy/` + 一个板块 kline
   （如 `https://d.10jqka.com.cn/v4/line/bk_881101/01/<今年>.js`），各 15 次共 30 次。
6. 通过标准：成功率 ≥ 50% 且平均耗时 < 5s。
7. 验证通过后：先接入 `fetch_index_daily.py` 的 THS 板块部分试点。
8. 验证不通过：先改用 Vultr 单 IP 做同样验证；再不行上付费 API。

## 下一步（worktree 会话内）

1. 同步原始分支：`git fetch origin master && git rebase origin/master`（如落后）。
2. 走 PRE-IMPLEMENTATION GATE：创建 GitHub issue → plan → RED tests → 实现。
3. 实现内容（初步）：
   - `scripts/proxy_pool/docker-compose.yml`：proxy_pool + Redis 本地启动。
   - `collectors/check_proxy_pool.py`（或 scripts/ 下）：独立验证脚本，
     从 proxy_pool API 取代理，用 curl_cffi 打 THS 列表页 + 一个板块 kline。
   - 输出成功率/平均耗时/失败原因，按锁定标准判定可用性。

## 验证结果（2026-08-16）

- 30 次请求（列表 15 + kline 15）成功率 **0%**，平均耗时 1.336s → **未通过**锁定标准。
- 代理池当时 30 个代理全部为 HTTP-only（`https: false`），HTTPS CONNECT 被拒。
- 后续路径（Vultr 单 IP / 付费 API）不在本 PR 范围，见 `.dsh/evidence/proxy-pool-trial.md`。

## 注意

- 主工作区当前在 `fix/286-tencent-amount` 且有未提交改动，本 worktree 独立隔离。
- 代理池只是试用；正式接入前必须先看验证结果。
