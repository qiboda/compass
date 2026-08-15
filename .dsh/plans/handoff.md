# Handoff — fix-277-fast-fail

## 用途

collectors 采集器**连续失败快速终止**（反爬封禁不再空转数小时）+ 全局限流调大。

**Issue**: https://github.com/qiboda/compass/issues/277（OPEN）
**分支**: fix/fast-fail-collector（基于 master 44c9d4a）
**原始分支**: master —— worktree 会话启动后先 `git fetch origin master && git rebase origin/master` 同步，再开始工作

## 背景

2026-08-15 首次真实采集指数数据（epic #255/#260）时东财 push2his 反爬封禁（45 请求/2 分钟触发，
IP 级 HTTP 000 全镜像封锁）。`fetch_index_daily.py::run()` 对失败标的从不中断流程：单标的失败
（2 hosts × 3 attempts 重试后）仅打印 FAILED 并 continue。封禁后 955 板块 + 30 官方指数 × 6 次尝试
≈ 3.5 小时空转。详细诊断见 `.dsh/evidence/index-fetch-resume-2026-08-15.md`（master 上已有）。

## grill-me 锁定决策（不得偏离）

| 决策 | 选择 |
|---|---|
| 终止粒度 | **连续 5 个标的失败（含重试）即终止**——反爬封禁必连续失败，及时止损；网络偶发抖动不误杀 |
| 失败定义 | 请求失败（`_get_json` 返回 None，所有 host×attempt 用尽）或 empty 响应（klines 为空）均计入连续失败 |
| 终止行为 | 保留已抓数据（写 CSV，可续采）+ 抛 RuntimeError 提示疑似反爬/接口故障 |
| 限流 | `common.py::EM_MIN_INTERVAL` 全局 0.5s → **2s**（用户确认全局调大，影响全部采集器） |
| 工作区 | 独立 worktree（本目录），因主工作区被其他会话（#276）占用 |

## 验收标准（issue #277）

1. `run()` 维护连续失败计数器；连续 5 个标的失败（FAILED 或 empty）→ 立即终止，不再请求剩余标的
2. 终止前已抓 daily/basic 记录写入 CSV（保留可续采），然后 RuntimeError 抛出
3. 失败-成功交错不触发终止（成功即清零计数器）
4. 连续 4 个失败不终止（第 5 个才触发）
5. `common.py::EM_MIN_INTERVAL` = 2.0
6. 测试覆盖：连续失败终止 + CSV 保留 + 交错不误杀 + 边界（4/5）+ 限流值断言
7. `uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` 全绿

## 下一步（worktree 会话）

1. 同步原始分支（fetch + rebase origin/master）
2. PRE-IMPLEMENTATION GATE 剩余步骤：委派 subagent_skwy_requirement_test + subagent_skwy_adversarial_test 写 RED 测试（注入项目 Python 测试方法论：make_stub_session / COMPASS_DATA_DIR tmp_path / tests 目录模式）
3. 实现 GREEN（fetch_index_daily.py run() 连续失败计数 + common.py EM_MIN_INTERVAL=2.0）
4. 全套件验证 → commit（ref #277）→ subagent_review → 待用户 push
5. 文档同步：toolchain.md 反爬排查卡补充"快速失败机制"；续采记录 .dsh/evidence/index-fetch-resume-2026-08-15.md 更新限流建议（EM_MIN_INTERVAL 已全局 2s）
