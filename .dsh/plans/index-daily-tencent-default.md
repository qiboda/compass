# Plan — issue #354: index_daily 官方指数 Tencent 主源 + EastMoney 备用

Worktree: `fix-index-daily-tencent-default` · 分支 `fix/index-daily-tencent-default`
Base: master @ `c9a55a6` · Issue: https://github.com/qiboda/compass/issues/354
Handoff（grill 锁定契约）: `.dsh/plans/handoff.md`

## 1. 目标

`index_daily` 官方指数（OFFICIAL_INDICES，30 只）的采集/回补/诊断三条路径
统一改为 **Tencent 主源、EastMoney 备用**：

- **daily `run()`** 官方指数段：Tencent 优先；Tencent 失败/为空 → fallback EastMoney。
- **`backfill()`** 官方指数段：同上；两者都失败 → 报错（不静默）。
- **`probe_official()`**：Tencent 优先 + EastMoney fallback（CLI 诊断可用）。
- **`SOURCE` 常量**：更新为“Tencent kline + EastMoney fallback + THS industry kline”。
- 模块 doc 注释与 `.dsh/kb/` 文档同步。

## 2. 非目标（grill 决策 6，明确排除）

- **THS 行业板块逻辑不变**：`fetch_ths_kline` 仍 THS 10jqka + proxy，不引入 EastMoney、
  不修改 bad-proxy 删除/直连重试逻辑。issue #354 body 中“fetch_ths_kline 删除 bad proxy”
  建议按 handoff 决策 6 明确**不在本 PR 范围**（记录为方案偏差，issue 收尾 comment 说明）。
- 不删除 `fetch_kline` / `PUSH2HIS` / `KLINE_HOSTS` 等东财代码（决策 4）。
- 不改 `update-database.sh` / `orchestrate.rs` / `main.rs`（`index-daily-backfill` CLI 已存在；
  `probe_official` 签名不变）。

## 3. 接口契约（测试子代理 RED 依据）

在 `crates/compass-collectors/src/index_daily.rs` 新增（模块私有，`mod tests` 经 `super::*` 访问）：

```rust
/// 单一官方指数的源选择结果。
#[derive(Debug, Clone, PartialEq, Eq)]
enum OfficialDecision {
    Tencent(Vec<String>),                                   // Tencent 主源命中（非空行）
    EastMoney { klines: Vec<String>, echoed_code: String }, // 东财备用命中（非空行 + API echo code）
    NoNewBars,        // 增量窗口、至少一方**应答**（含空行）→ 无新行 no-op 成功
    Fail,             // 双 None（双方都不可达）；或非增量窗口无行 → 失败
}

/// 纯决策函数：Tencent 非空 → Tencent；否则 EastMoney 非空 → EastMoney；
/// 否则 incremental=true 且至少一方 Some(应答过，含空) → NoNewBars；
/// 否则仅双方均 None（增量）→ Fail；非增量任何无行组合 → Fail。
fn decide_official(
    tencent: Option<Vec<String>>,
    eastmoney: Option<(Vec<String>, String)>,
    incremental: bool,
) -> OfficialDecision
```

语义矩阵（决策依据：handoff 决策 1/2/7 + 现状“周末/停牌无新行按成功 no-op”兼容）：

| tencent | eastmoney | incremental | 结果 |
|---|---|---|---|
| Some(非空) | 任意 | 任意 | `Tencent` |
| None/Some(空) | Some(非空 pair) | 任意 | `EastMoney` |
| 至少一方 Some（含空） | 其余为 None/空 | true | `NoNewBars` |
| 双方均 None | — | true | `Fail`（双方都不可达才失败；双空 Some 属上行 no-op） |
| 任意空/None 组合 | — | false | `Fail` |

> **裁决（2026-09-04）**：初稿第 4 行“双方均 None **或均空-无应答**”→“Fail”与第 3 行
> “至少一方 Some → NoNewBars”矛盾。权威语义 = **双空 Some（`Some(vec![])` +
> `Some((vec![], code))`）+ incremental=true → `NoNewBars`**（保持现状“周末/停牌 no-op
> 成功”），仅**双 None** → `Fail`。对抗测试按此编码；实现按此执行。
> **“非空” = Vec 非空**（不校验行内容；`Some(vec![""])` 视为非空命中——测试⑧用弱断言兼容）。

**调用点改造**（网络编排，不做单元测试——纯函数已覆盖决策）：

- `run()` 官方段（现 line 852-957）：先 `fetch_tencent_kline(...)`，若其非空则不再调东财；
  否则 `fetch_kline(...)`；`decide_official(t, e, last_date.is_some())` →
  - `Tencent(rows)`：成功路径（bump 0、push basic、extend records，日志 `(tencent)`）；
  - `EastMoney{klines, echoed_code}`：保留现有 code-mismatch 检查（`echoed_code != code && != symbol` → skip，不 bump、不 fail）；通过则成功路径；
  - `NoNewBars`：`consecutive_failures=0`、push basic（保持现状）、无 daily 行、日志 `no new bars`；
  - `Fail`：`bump_failure` → `abort_reason`（现状 fast-fail 语义不变）。
- `backfill()` 官方段（现 line 1127-1140）：先 Tencent(None last_date) 后东财；
  `decide_official(t, e, false)` → `Tencent`/`EastMoney` extend；`Fail` →
  `Err(InvalidInput("index_daily backfill failed for official {symbol}"))`（错误消息保持不变）。
- `probe_official()`（现 line 431-441）：先 Tencent（`last_date` 透传）后东财；
  `decide_official(t, e, last_date.is_some())` → 命中返回 `(rows, code_label)`；
  `NoNewBars` → `Ok((vec![], tcode))`；`Fail` → `Err(InvalidInput("index_daily probe failed for {secid}"))`。
  code_label：`Tencent` → `tencent_code(secid)`；`EastMoney` → `echoed_code`。
- 常量 `SOURCE`（line 28）：`"Tencent kline + EastMoney fallback + THS industry kline"`。
- 模块 doc（line 1-6）：`EastMoney push2his kline with Tencent fallback` → `Tencent kline primary with EastMoney push2his fallback`（措辞随实现定）。

## 4. 测试

- **第 3.5 步 adversarial（RED）**：委派 `subagent_skwy_adversarial_test`，在 `mod tests` 内
  追加针对 `decide_official` 的对抗测试（覆盖上表全部边界：空/None/非空混合、incremental 真假、
  优先级反转、双方均空、双方不可达、非空优先于非空等）。
- **第 4 步 requirement（RED）**：委派 `subagent_skwy_requirement_test`，追加需求验收测试：
  `SOURCE` 新值相等断言、`decide_official` 主源优先/备用激活/失败语义、
  错误消息格式（backfill/probe 调用点不用测试——纯函数 + 调用点 code review）。
- 两批测试在实现前提交，编译失败即为 RED（契约已声明）。

## 5. 文档同步（门禁 5b）

| 文件 | 变更 |
|---|---|
| `.dsh/kb/design/data-providers.md` | “腾讯回退（issue #278/#286）”段（line 183）改为“腾讯主源（issue #354）+ 东财备用”；“增量”段（line ~188）“官方指数东财 beg=MAX+1、腾讯增量翻页遇 <= MAX 行即停”同步改序；`## 决策记录`（line 525）加一行 #354 决策 |
| `.dsh/kb/dev/toolchain.md` | #354 排查卡（line 845-866）追加“已于 fix/index-daily-tencent-default 修复”小节（修复内容 + 验证） |
| `.dsh/kb/design/symbols.md` | line 163 “由东财 push2his 独立采集” → “由腾讯主源（东财备用）独立采集” |
| `.dsh/kb/dev/process.md` | line 564 “push2his+THS+腾讯兜底（index_daily）” → “腾讯+THS（东财 push2his 备用，index_daily）” |

## 6. 验证与冒烟

1. `cargo fmt --check` / `cargo clippy -p compass-collectors -- -D warnings` / `cargo test -p compass-collectors`。
2. 网络前置探测：`curl` Tencent URL + push2his URL 确认本机可达性。
3. 真实冒烟（不跑完整 update-database.sh）：
   - `cargo run --bin compass-collectors -- index-daily-probe --secid 1.000001`
     （EM 不可达环境下必须成功写 CSV——验证 Tencent-first + 东财失败不阻断）。
   - 若环境允许：`index-daily-backfill --start <近两日> --end <近两日>` 小范围回补冒烟
     （THS+官方全流程，输出 `index_daily_backfill.csv` 有行数为证；不写 Dolt——backfill CLI
     只写 CSV，import 由 auto-heal 自己调；本 PR 不触发 Dolt 写库）。

## 7. Commit 序列（均含独立行 `ref #354`）

1. `test: RED acceptance+adversarial for #354 tencent-first contract`
   （两批测试同为 `mod tests` 内追加块，合并为单个 RED commit；方案调整说明：分两个
   commit 需同一文件拆分 hunk staging，记录噪音大于收益）
2. `fix: index_daily official indices tencent primary with eastmoney fallback`（GREEN）
3. `docs: sync kb for #354 tencent-first priority`
4. 反思 commit（skwy-reflect，push 前）

Review：5 角度并行审查（review/review_goal/review_quality/review_security/review_qa）→ 修复（≤2 轮）→
push 前 rebase origin/master → 反思 → push → PR（`fix/index-daily-tencent-default` →
master，A-Data/C-Bug 标签）→ issue 收尾（comment + close #354）→ worktree close。

## 8. 风险与注意

- `fetch_tencent_kline` 现返回 `Ok(Some([]))` 表示“应答但无新行”、`Ok(None)` 表示“不可用”
  （网络/畸形 payload）——`decide_official` 对二者均不“命中”，no-op/fail 语义按上表矩阵。
- 每日增量“无新行 no-op”必须保留（决策 1 的空值 fallback 只改变顺序，不把 no-op 变成失败）。
- 不修改 `fetch_kline`/`fetch_tencent_kline` 本身的参数与返回语义。
