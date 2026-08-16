# proxy_pool HTTPS validator patch — verification evidence (issue #290)

- Date: 2026-08-16 (Asia/Shanghai)
- Worktree: `proxy-pool-https-validator`, branch `feat/proxy-pool-https-validator`
- Issue: https://github.com/qiboda/compass/issues/290
- Image: `proxy_pool_https_validator:local` (docker build ID `ad3cc044c1d0`)
- Dockerfile: `scripts/proxy_pool/Dockerfile`（多阶段构建：基于 `jhao104/proxy_pool:2.4.2` 的 build 阶段 `apk add patch` + `RUN patch -p1 < validator.patch`，final 阶段复制补丁后的 `helper/validator.py`，运行时镜像不含 patch/补丁文件）
- Patch: `scripts/proxy_pool/validator.patch`（仅改 `httpsTimeOutValidator` 的 https key 为 `http://`）

## 构建与运行环境

- 受限沙箱不允许 bridge 网络创建 veth，构建使用
  `docker build --network=host -t proxy_pool_https_validator:local scripts/proxy_pool`。
- 运行使用 host-network 直接启动 Redis 与 proxy_pool：
  `docker run -d --name proxy_redis --network host redis:7-alpine redis-server --appendonly no`
  `docker run -d --name proxy_pool --network host -e DB_CONN=redis://@127.0.0.1:6379/0 --entrypoint /bin/sh proxy_pool_https_validator:local -c "python proxyPool.py server & python proxyPool.py schedule & wait"`
- 已确认运行容器内补丁生效：
  `/app/helper/validator.py` 第 75 行为
  `proxies = {"http": "http://{proxy}".format(proxy=proxy), "https": "http://{proxy}".format(proxy=proxy)}`
- 多阶段 final 镜像已确认不含 `patch` 二进制（`which patch` → no-patch-in-final）。

## 代理池状态

- 等待约 6 分钟后 `/all/` 仅收集到 3 个免费代理。
- 池内 `https: true` 数量：**0**（3 个全部 `https: false`）。
- 日志显示 `RawProxyCheck` / `UseProxyCheck` 对 3 个代理均 pass，但
  `DoValidator.validator` 的 `httpsValidator` 仍返回 False——这些免费代理
  不支持标准 CONNECT 隧道（或已失效）。

## 验证脚本输出

命令：`uv run --project collectors python collectors/check_proxy_pool.py --count 15 --timeout 15`

```json
{"success_rate": 0.0, "avg_elapsed": 0.064956248505041, "verdict": "FAIL", "judge_reason": "FAIL: success_rate=0.000 (>=0.500: False), avg_elapsed=0.065s (<5.000s: True)", "failures": ["Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details."], "targets": [{"target": "https://q.10jqka.com.cn/thshy/", "total": 3, "success": 0, "success_rate": 0.0, "avg_elapsed": 0.06378208100795746}, {"target": "https://d.10jqka.com.cn/v4/line/bk_881101/01/2026.js", "total": 3, "success": 0, "success_rate": 0.0, "avg_elapsed": 0.06613041600212455}]}
```

## 补充验证：受控 CONNECT 代理（机制证明）

由于免费代理池中的代理本身不支持 CONNECT，另做一个**受控机制验证**：在沙箱内
启动一个 HTTP 代理（`127.0.0.1:8888`，链路上游走 Clash `127.0.0.1:7897`），
它支持标准 CONNECT 隧道，可访问 `https://example.com` 与 THS kline 接口。

- 将 `127.0.0.1:8888` 手动写入 proxy_pool Redis（`use_proxy`）。
- 为适配沙箱网络，proxy_pool 以 `HTTP_URL=http://example.com`、
  `HTTPS_URL=https://example.com` 启动（默认 `https://www.qq.com` 在本沙箱
  HEAD 返回 501，会误伤可用代理）。
- 补丁后的 `httpsTimeOutValidator` 将该代理标记为 **`https: true`**：
  ```json
  {"proxy": "127.0.0.1:8888", "https": true, "check_count": 1, "last_status": true}
  ```
- 含该代理时重跑 `collectors/check_proxy_pool.py`，THS 成功率从 **0% 提升到
  **25%**（两个 target 各 1/4 成功；剩余失败来自免费代理 CONNECT 400/405）：
  ```json
  {"success_rate": 0.25, "avg_elapsed": 0.10807431212015217, "verdict": "FAIL", "judge_reason": "FAIL: success_rate=0.250 (>=0.500: False), avg_elapsed=0.108s (<5.000s: True)", "failures": ["Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 405. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 400. See https://curl.se/libcurl/c/libcurl-errors.html first for more details.", "Failed to perform, curl: (56) CONNECT tunnel failed, response 405. See https://curl.se/libcurl/c/libcurl-errors.html first for more details."], "targets": [{"target": "https://q.10jqka.com.cn/thshy/", "total": 4, "success": 1, "success_rate": 0.25, "avg_elapsed": 0.08759356148948427}, {"target": "https://d.10jqka.com.cn/v4/line/bk_881101/01/2026.js", "total": 4, "success": 1, "success_rate": 0.25, "avg_elapsed": 0.12855506275082007}]}
  ```

## 结论

- 补丁镜像构建成功，容器内 `httpsTimeOutValidator` 已按 #290 改为
  `http://{proxy}`。
- 仅靠当前免费代理源：**未观察到 `https: true` 代理出现**，THS 成功率仍为 0%
  （CONNECT tunnel failed 400）——原因是这些免费代理本身不支持 CONNECT 隧道
  （或已失效），不是补丁未生效。
- 受控 CONNECT 代理验证：补丁后的 proxy_pool **能正确把支持 CONNECT 的 HTTP
  代理标记为 `https: true`**，且 `check_proxy_pool.py` 能通过该代理成功请求
  THS HTTPS 接口（成功率 0% → 25%）。这证明 #290 的代码修复有效。
- 若要 THS 成功率达标（≥50%），仍需按 #287 的后续路径尝试 Vultr 单 IP / 付费
  代理，不在 #290 范围内。
