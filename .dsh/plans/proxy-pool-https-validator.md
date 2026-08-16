# Plan — proxy-pool-https-validator (#290)

## 用途

修正 proxy_pool 的 HTTPS 验证逻辑：`httpsTimeOutValidator` 的 `proxies`
改为同时提供 `http://` 代理地址给 http 和 https，使免费 HTTP 代理能通过
标准 CONNECT 验证 HTTPS；通过自定义 Dockerfile 打补丁镜像，重建后重跑
`collectors/check_proxy_pool.py` 验证 THS 板块接口成功率。

**Issue**: https://github.com/qiboda/compass/issues/290
**分支**: `feat/proxy-pool-https-validator`
**原始分支**: `master`

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

## 范围（本次 PR）

- `scripts/proxy_pool/validator.patch`：上游 `jhao104/proxy_pool:2.4.2`
  `helper/validator.py` 的一行补丁——`httpsTimeOutValidator` 的 https key
  从 `https://{proxy}` 改为 `http://{proxy}`。
- `scripts/proxy_pool/Dockerfile`：基于 `jhao104/proxy_pool:2.4.2`，构建时
  应用补丁（可复现）。
- `scripts/proxy_pool/docker-compose.yml`：proxy_pool 服务改用 `build: .`，
  不再直接使用上游镜像 tag。
- RED 测试（需求验收 + 对抗性）：验证补丁内容、Dockerfile 应用补丁、compose
  使用 `build: .`。
- 文档同步：`.dsh/kb/dev/process.md` 的 proxy_pool 章节。
- 真实验证证据：`.dsh/evidence/proxy-pool-https-validator.md`。

不在范围：切换到 Vultr 单 IP / 付费代理 API；正式接入 `fetch_index_daily.py` 等
collectors 采集逻辑。

## Tasks

| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #290 | 委派 adversarial test 子代理写 RED（补丁/Dockerfile/compose 契约） | — |
| done | #290 | 委派 requirement test 子代理写 RED（需求验收） | — |
| done | #290 | 实现 `scripts/proxy_pool/validator.patch` | RED 测试 |
| done | #290 | 实现 `scripts/proxy_pool/Dockerfile` | RED 测试 |
| done | #290 | 更新 `scripts/proxy_pool/docker-compose.yml` 使用 `build: .` | RED 测试 |
| done | #290 | 运行 Python 测试 + 覆盖率门禁 | 实现 |
| done | #290 | 真实重建 proxy_pool 容器并重跑验证脚本，写入 `.dsh/evidence/` | 实现 |
| done | #290 | 文档同步（`.dsh/kb/dev/process.md` + `toolchain.md`） | 实现 |
| pending | #290 | commit → review →（用户确认后）push → PR | 全部 |

## 验收标准

1. 补丁把 `httpsTimeOutValidator` 中 https 代理 scheme 从 `https://` 改为
   `http://`（两个 key 均为 `http://`）。
2. `Dockerfile` 基于 `jhao104/proxy_pool:2.4.2` 并可在构建时稳定应用补丁。
3. `docker-compose.yml` 的 proxy_pool 服务使用 `build: .`，不直接使用上游
   镜像 tag。
4. 新增测试覆盖补丁/Dockerfile/compose 契约；Python 全套件绿
   （`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q`）。
5. 真实重建后重跑验证脚本，记录 `https: true` 代理是否出现、THS 成功率是否
   变化。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| HTTPS 验证代理 scheme | `https://ip:port` / `http://ip:port` 两者 | 两个 key 均用 `http://ip:port` | 免费 HTTP-only 代理只支持普通 HTTP 连接，用 `http://` 才能走标准 CONNECT 隧道完成 HTTPS 验证 | `https://` 会让客户端先与代理建立 TLS，HTTP-only 代理不支持，验证必失败 |
| 镜像交付方式 | 直接改上游镜像 tag / 本地 Dockerfile 打补丁 | 本地 `Dockerfile` + `validator.patch` | 可复现、可审计、不依赖上游发布新版本 | 直接改 tag 不可复现且污染第三方镜像 |
| compose 引用方式 | `image: jhao104/proxy_pool:2.4.2` / `build: .` | `build: .` | 保证本地补丁镜像被实际使用 | 继续用 image tag 会绕过补丁，验证结果失真 |
