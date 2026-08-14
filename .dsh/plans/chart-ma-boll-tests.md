# chart-ma-boll — 测试用例文档（Batch 1，ref #175 / #177）

> QA skill 阶段 0 产出，供实现 agent 在 GREEN 阶段逐条执行。本文件是**决策完备**的：
> 实现者照做即可，不需要重新推导任何断言、边界或行号。测试代码写在目标文件
> `#[cfg(test)] mod tests` 中（参照 `compass-strategy/src/sepa/indicators.rs:268-592`
> fixture 模式），不另建独立测试文件。
> 日期：2026-08-04。对应计划：`.omo/plans/chart-ma-boll.md` Todo 1、Todo 3。

## 范围

| 子 issue | 目标文件 | 被测函数 |
|---|---|---|
| #175 | `crates/compass-core/src/indicators.rs` | `ma` / `bollinger` / `adjust_ohlc` |
| #177 | `crates/compass-ui/src/tokens/color.rs` | `IndicatorTokens::dark()` / `light()`（16 值） |

## 1. ma(values: &[f64], n: usize) -> Vec<Option<f64>>

简单移动平均，滑动窗口含当前 bar；窗口不足/窗口内非有限 → None；永不 panic、不产生 NaN。

| # | 场景 | 输入 | 断言 |
|---|---|---|---|
| M1 | 平坦序列（满窗后为常数） | `[10.0;10]`, n=5 | len=10；`out[..4]` 全 None；`out[4..]` 全 `Some(10.0)` |
| M2 | 线性上升（手工算均值） | `[1.0..=6.0]`, n=3 | `out[..2]` 全 None；`out[2..]=Some(2,3,4,5)`（均值 (i-2..=i)/3） |
| M3 | 空输入 | `[]`, n=5 | 返回空 vec（不 panic） |
| M4 | n=0 | `[1.0,2.0,3.0]`, n=0 | len=3 全 None（零窗口无意义） |
| M5 | 窗口不足 | `[1.0,2.0,3.0]`, n=5 | len=3 全 None（不 panic） |
| M6 | 窗口内 NaN → 该位置 None | `[1,2,NaN,4,5,6]`, n=3 | `out[0..=2]` 全 None；`out[3]=Some(5)`（窗 [4,5,6]） |
| M7 | 窗外 NaN 不污染 | `[NaN,2,3,4,5]`, n=3 | `out[3]=Some(3)`（窗 [2,3,4] 不含 NaN）；`out[4]=Some(4)` |

## 2. bollinger(values: &[f64], period, k) -> Vec<(Option<f64>, Option<f64>, Option<f64>)>

布林带 (upper, mid, lower)；mid = SMA；std = **总体**标准差（除以 period）；窗口不足/窗口内非有限 → None。

| # | 场景 | 输入 | 断言 |
|---|---|---|---|
| B1 | 平坦序列 | `[10.0;8]`, p=3, k=2 | len=8；`out[..2]` 全 None；`out[2..]` 全 `(10,10,10)`（std=0） |
| B2 | 手工总体 std | `[1,2,3]`, p=3, k=1 | `out[2]=(2+√(2/3), 2, 2-√(2/3))`（1e-9） |
| B3 | k=0 坍缩 | `[1,2,3]`, p=3, k=0 | `out[2]` 三值均 = 2（1e-9） |
| B4 | 窗口不足 | `[1.0,2.0]`, p=5 | len=2 全 None（不 panic） |
| B5 | NaN 污染 | `[1,NaN,3,4]`, p=3 | `out[0..=3]` 全 None（窗 [1,NaN,3]、[NaN,3,4] 均含 NaN） |

> B5 注意：`[1,NaN,3,4]` 窗口 p=3 时 index3 窗 = `[NaN,3,4]`（1-based 后 3 个）→ None。

## 3. adjust_ohlc(raw: &[RawBar], adjclose: &[f64]) -> Vec<egui_charts::model::Bar>

前复权缩放：`factor_i = adjclose_i / close_i`（最新日 adjclose==close → factor=1.0）；OHLC × factor；
volume 原样；close<=0 或 adjclose 非有限 → factor=1.0 守卫；日期升序保留；不 panic。

fixture：`raw_bars(closes)` → 从 2026-08-01 起逐日 +1，open=close-1, high=close+2, low=close-2, vol=1000。

| # | 场景 | 输入 | 断言 |
|---|---|---|---|
| A1 | 最新日锚点 + 历史缩放 | `closes=[10,20]`, adj=`[8,20]` | len=2；最新 bar: o=19,h=22,l=18,c=20,v=1000（factor=1）；旧 bar: o=7.2,h=9.6,l=6.4,c=8.0（factor=0.8），v=1000 |
| A2 | 日期保留 | 同上 | `bars[i].time.date_naive() == raw[i].date` |
| A3 | close=0 守卫 | `closes=[0,20]`, adj=`[0,20]` | `bars[0]` 四价全 `is_finite()`；close==0.0（factor 回落 1.0 不缩放） |
| A4 | adjclose=NaN 守卫 | `closes=[10,20]`, adj=`[NaN,20]` | `bars[0].close.is_finite()`；close==10.0（factor 回落 1.0） |
| A5 | 空输入 | `raw=[]`, `adj=[]` | 空 vec（不 panic） |

## 4. IndicatorTokens（#177，16 值断言）

`crates/compass-ui/src/tokens/color.rs`：`ColorTokens` 下新增 `pub indicator: IndicatorTokens`
子结构（8 字段），dark()/light() 两套。断言参照现有 `dark_palette_matches_design_spec`
（color.rs:169-179）逐字段 `assert_eq!` 模式。

| # | token | 暗色 | 亮色 |
|---|---|---|---|
| T1 | ma5 | `#D1D4DC`（=text_primary） | `#1B2430`（=text_primary） |
| T2 | ma10 | `#F5A623`（=warning） | `#B57A00`（=warning） |
| T3 | ma60 | `#BA68C8` | `#7B1FA2` |
| T4 | ma120 | `#00BCD4` | `#00838F` |
| T5 | ma250 | `#A1887F` | `#6D4C41` |
| T6 | bb_upper | `#90A4AE` | `#546E7A` |
| T7 | bb_middle | `#90A4AE` | `#546E7A` |
| T8 | bb_lower | `#90A4AE` | `#546E7A` |

| # | 场景 | 断言 |
|---|---|---|
| C1 | dark 8 值精确匹配 | `ColorTokens::dark().indicator.ma5 == Color32::from_rgb(0xD1,0xD4,0xDC)` 等 8 条 |
| C2 | light 8 值精确匹配 | `ColorTokens::light().indicator.ma10 == Color32::from_rgb(0xB5,0x7A,0x00)` 等 8 条 |
| C3 | ma5 复用 text_primary / ma10 复用 warning | `indicator.ma5 == text_primary`、`indicator.ma10 == warning`（暗亮两套均成立） |
| C4 | BOLL 三线同色 | 暗色 `bb_upper == bb_middle == bb_lower`（亮色同理） |

> 结构缺失字段 → **编译失败**即 RED（#177 无独立逻辑测试，靠结构声明 + 16 值断言）。

## 5. RED 判据（阶段 1 证据）

- `ma`/`bollinger`/`adjust_ohlc`：当前实现为 `unimplemented!()` → `cargo test -p compass-core indicators`
  编译失败（unimplemented! panic）/测试失败。GREEN 后上述场景全绿。
- IndicatorTokens：当前无 `indicator` 字段 → `cargo test -p compass-ui` 编译失败（缺字段）。
  因 Batch 1 先做 #175，C 组测试随 #177 一并写入同一 RED 阶段。
