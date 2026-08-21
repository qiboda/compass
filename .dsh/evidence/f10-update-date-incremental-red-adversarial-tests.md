# RED evidence: adversarial tests for issue #299

## 状态
- 测试文件：`collectors/tests/test_f10_incremental_adversarial.py`
- 命令：`cd collectors && uv run pytest tests/test_f10_incremental_adversarial.py -q`
- 结果：**13 failed, 1 passed**（RED 符合预期；1 passed 为 `dedupe_csv` 默认行为回归守卫，实现后需保持绿色）

## 失败类别
- `common.update_date_anchor` / `common.fetch_by_update_date` 不存在 → `AttributeError`
- `run(incremental=True)` 无该参数 → `TypeError`
- `common.dedupe_csv(path, date_col=...)` 无该参数 → `TypeError`

## 覆盖的对抗维度
- 锚点双源取 min、单源缺失、未来日期 clamp、data_updates 按 DOLT_TABLE 查询
- incremental 空结果不写 state、UPDATE_DATE 全缺失保留旧 anchor
- `fetch_by_update_date` filter/sort 形状、pages cap 500
- `dedupe_csv` 默认 REPORTDATE 回归 + F10 `REPORT_DATE` 参数

## 委派说明
- 三次尝试委派 `subagent_skwy_adversarial_test`（含一次 resume）均因子代理 token/context 上限在写文件前中断（无产物）。
- 为满足门禁 RED 证据，主 agent 依据已批准 plan 的接口契约补写对抗性测试文件并运行验证；未修改生产代码。
- 需求验收 RED 由 `subagent_skwy_requirement_test` 正常产出（见 `f10-update-date-incremental-red-requirement-tests.md`）。
