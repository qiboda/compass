# proxy_pool trial — verification evidence (issue #287)

- Date: 2026-08-16 (Asia/Shanghai)
- Worktree: `proxy-pool-trial`, branch `feat/proxy-pool-trial`
- Command: `uv run --project collectors python collectors/check_proxy_pool.py --count 15 --timeout 15`
- Result: **FAIL** — success rate 0.0% (< 50%), avg elapsed 1.336s (< 5s)

## Raw JSON output

```json
{"success_rate": 0.0, "avg_elapsed": 1.335994602701006, "verdict": "FAIL", "judge_reason": "FAIL: success_rate=0.000 (>=0.500: False), avg_elapsed=1.336s (<5.000s: True)", "targets": [{"target": "https://q.10jqka.com.cn/thshy/", "total": 15, "success": 0, "success_rate": 0.0, "avg_elapsed": 2.60426602899873}, {"target": "https://d.10jqka.com.cn/v4/line/bk_881101/01/2026.js", "total": 15, "success": 0, "success_rate": 0.0, "avg_elapsed": 0.06772317640328158}]}
```

## Environment notes

- proxy_pool + Redis were started locally with Docker Compose. This DSH sandbox
  cannot create bridge-network veth pairs (`operation not supported`), so the
  verification run used a temporary host-network override
  (`/tmp/proxy_pool-host-compose.yml`), not the committed compose file.
- The published `jhao104/proxy_pool:latest` image is Alpine-based and its
  default ENTRYPOINT invokes `bash`, which is not installed in the image. The
  committed compose file works around this by starting
  `python proxyPool.py server` / `python proxyPool.py schedule` via `sh`.
- At verification time the pool contained 30 proxies; all were HTTP-only
  (`https: false`). Every HTTPS CONNECT attempt to the THS endpoints was
  rejected by the proxies (`CONNECT tunnel failed, response 400`).

## Conclusion per locked criteria

- Success rate >= 50%? **No** (0/30).
- Average elapsed < 5s? **Yes** (1.336s).
- Overall: **trial does not pass**. Next steps per handoff: try Vultr single IP,
  then paid proxy API (not in this PR's scope).
