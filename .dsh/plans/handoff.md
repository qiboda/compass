# Handoff — proxy-pool-https-validator

## 用途

修正 proxy_pool 的 HTTPS 验证逻辑：`httpsTimeOutValidator` 的 `proxies`
改为同时提供 `http://` 代理地址给 http 和 https，使免费 HTTP 代理能通过
标准 CONNECT 验证 HTTPS；通过自定义 Dockerfile 打补丁镜像，重建后重跑
`collectors/check_proxy_pool.py` 验证 THS 板块接口成功率。

**Issue**: https://github.com/qiboda/compass/issues/290
**Plan**: `.dsh/plans/proxy-pool-https-validator.md`

## 已锁定决策（grill-me 共识）

1. 修改 `helper/validator.py` 中 HTTPS 验证的 proxies：
   ```python
   proxies = {"http": f"http://{proxy}", "https": f"http://{proxy}"}
   ```
2. 在 `scripts/proxy_pool/` 增加 `Dockerfile` + 补丁文件，`docker-compose.yml`
   改用本地构建的补丁镜像（可复现）。
3. 重建容器后重跑 `collectors/check_proxy_pool.py`，观察 `https: true` 代理
   是否出现、THS 成功率是否变化。
4. 按项目门禁走：新 worktree → issue → RED tests → 实现 → 文档。

## 下一步（worktree 会话内）

1. 同步原始分支（如落后）：`git fetch origin master && git rebase origin/master`。
2. 走 PRE-IMPLEMENTATION GATE：创建 GitHub issue → plan → RED tests → 实现。
3. 实现内容（初步）：
   - `scripts/proxy_pool/validator.patch`（或等价补丁文件）。
   - `scripts/proxy_pool/Dockerfile`：基于 `jhao104/proxy_pool:2.4.2` 应用补丁。
   - 更新 `scripts/proxy_pool/docker-compose.yml` 使用 `build: .`。
   - 测试：补丁/镜像构建逻辑或验证脚本相关测试。
4. 真实重建并重跑验证，结果写入 `.dsh/evidence/`。

## 注意

- 主工作区在 master（可能落后 origin），本 worktree 已基于 `origin/master`。
- proxy_pool 上游 2.4.2 的 `httpsTimeOutValidator` 当前把 `https` 代理写成
  `https://ip:port`，这是本次要修正的点。
