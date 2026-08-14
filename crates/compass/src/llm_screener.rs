//! LLM screener prompt builder and response parser (epic #243 Batch 4).
//!
//! Pure functions shared by the GUI backend and tests: [`build_screener_prompt`]
//! turns a free-text screener request into a system prompt that instructs an
//! LLM to emit a single `Filter` JSON object; [`parse_filter_response`] strips
//! a surrounding ```json/``` code fence (if any), deserializes the JSON into a
//! `Filter` and runs `validate_filter` on it. No network access here — the
//! LLM client lives in `compass-core::llm`.

use compass_types::{Filter, validate_filter};

/// Build the system prompt for the screener LLM.
///
/// The prompt declares the strict single-JSON-object output contract, documents
/// the serde tagged-union format of `Filter` (`Meta`/`Series`/`And`/`Or`/`Not`),
/// lists every `MetaCond`/`SeriesFactor`/`CmpOp`/`SeriesCond` variant with unit
/// conventions (market cap in 亿元, returns in %), embeds one complete worked
/// example and insists on structurally sound numeric values. `description` is
/// embedded as the user requirement so the model answers the concrete request.
pub fn build_screener_prompt(description: &str) -> String {
    const PROMPT: &str = r#"你是 A 股选股条件生成助手。请严格输出单个 JSON 对象，表示一个选股 Filter AST（serde tagged-union 格式）。禁止输出 markdown 代码围栏、注释或任何多余文字。

Filter AST 是递归结构，JSON 中每一层只保留一个键：
{"Meta": {...}}        元数据约束（MetaCond）
{"Series": {...}}      行情序列条件（SeriesCond）
{"And": [Filter, ...]} 全部子条件同时满足
{"Or": [Filter, ...]}  任一子条件满足即可
{"Not": {...}}         对内部 Filter 取反

MetaCond 变体（作为 Meta 的值）：
- "Industry": ["白酒", "银行", ...]  行业属于集合（OR 语义；空集合 = 不限行业）
- "Exchange": ["SH", "SZ", "BJ"]     交易所属于集合
- "Board": ["主板", "创业板"]         板块属于集合
- "ListYears": 3                     上市满 N 年
- "Delisted": false                  是否包含退市股（false = 排除退市股）
- "MarketCap": {"min": 100.0, "max": 5000.0}   市值范围，单位亿元；单边可省略（缺省一侧 = 无界）

SeriesFactor 因子（作为因子值）：
- "Close"                   最新复权收盘价
- "Sma": 20                 N 日简单移动平均
- "ChangePct": 20           N 日涨跌幅（%）
- "DayPct"                  单日涨跌幅（%）
- "AvgVolume": 10           N 日平均成交量
- "NDayHigh": 120           N 日最高价

CmpOp 比较符（snake_case）："eq" / "ne" / "gt" / "ge" / "lt" / "le"

SeriesCond 变体（作为 Series 的值）：
- "Cmp": {"factor": <SeriesFactor>, "op": <CmpOp>, "value": {"Const": 数值} 或 {"Factor": <SeriesFactor>}}   因子与参考值比较
- "UpDays": {"n": 5, "min_pct": 3.0}   连续 n 天每天涨幅 > min_pct（%，n >= 1）
- "Count": {"factor": <SeriesFactor>, "op": <CmpOp>, "value": ..., "window": 10, "at_least": 5}   最近 window 天中至少 at_least 天满足比较（1 <= at_least <= window）
- "VolumeSurge": {"days": 10, "times": 2.0}   最近 days 日平均成交量不低于基线 times 倍（days >= 1）

单位约定：市值一律用亿元，涨跌幅一律用 %，A 股红涨绿跌。

完整示例：用户需求"最近5天每天涨超3%"应输出：
{"Series":{"UpDays":{"n":5,"min_pct":3.0}}}

数值约束（必须遵守）：
- 窗口类参数（Sma/ChangePct/AvgVolume/NDayHigh 的 n、UpDays.n、Count.window/at_least、VolumeSurge.days）>= 1
- 所有浮点数必须是有限数，禁止 NaN / Infinity（含 min_pct、times、Const 值、MarketCap 边界）
- MarketCap 的 min <= max（两侧均给出时）

只输出单个 JSON 对象本身，不要任何解释文字。

用户需求：__DESCRIPTION__"#;
    PROMPT.replace("__DESCRIPTION__", description)
}

/// Parse a model response into a validated screener `Filter`.
///
/// Pipeline: trim surrounding whitespace → strip a ```json/``` code fence if
/// present → deserialize as [`Filter`] → run [`validate_filter`]. Pure and
/// total — never panics. Deserialization failures are wrapped as
/// `invalid filter JSON: {e}`; semantic validation failures pass through
/// unchanged so the caller can surface the offending field.
pub fn parse_filter_response(content: &str) -> Result<Filter, String> {
    let body = strip_code_fence(content);
    let filter: Filter =
        serde_json::from_str(body).map_err(|e| format!("invalid filter JSON: {e}"))?;
    validate_filter(&filter)?;
    Ok(filter)
}

/// Strip a surrounding markdown code fence from a model response.
///
/// Accepts both ```json ... ``` and bare ``` ... ``` fences; leading/trailing
/// whitespace on the fence lines is tolerated. Non-fenced input is trimmed and
/// returned unchanged. Total function — never panics.
fn strip_code_fence(content: &str) -> &str {
    let trimmed = content.trim();
    if trimmed.starts_with("```") && trimmed.ends_with("```") {
        // Body is everything between the opening fence line (up to the first
        // newline) and the trailing fence.
        if let Some(after_open) = trimmed.find('\n').map(|i| &trimmed[i + 1..]) {
            if let Some(body) = after_open.strip_suffix("```") {
                body.trim()
            } else {
                after_open.trim()
            }
        } else {
            ""
        }
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_types::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor};

    #[test]
    fn prompt_is_non_empty_and_mentions_ast_schema() {
        let p = build_screener_prompt("最近5天每天涨超3%");
        assert!(!p.is_empty());
        for kw in [
            "Filter",
            "Meta",
            "Series",
            "UpDays",
            "And",
            "Or",
            "Not",
            "MarketCap",
        ] {
            assert!(p.contains(kw), "prompt must mention {kw}");
        }
    }

    #[test]
    fn prompt_contains_example_json() {
        let p = build_screener_prompt("最近5天每天涨超3%");
        assert!(p.contains("UpDays"), "example must use UpDays: {p}");
        assert!(p.contains("min_pct"), "example must carry min_pct: {p}");
        assert!(
            p.contains("3.0"),
            "example must carry the 3.0 pct value: {p}"
        );
    }

    #[test]
    fn prompt_embeds_user_description() {
        let desc = "市值大于100亿的股票";
        let p = build_screener_prompt(desc);
        assert!(p.contains(desc), "description must be embedded: {p}");
    }

    #[test]
    fn parses_bare_valid_json() {
        let f = parse_filter_response(r#"{"Series":{"UpDays":{"n":5,"min_pct":3.0}}}"#)
            .expect("bare valid JSON parses");
        assert_eq!(f, Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }));
    }

    #[test]
    fn parses_json_fenced_content() {
        let src = "```json\n{\"Series\":{\"UpDays\":{\"n\":5,\"min_pct\":3.0}}}\n```";
        let f = parse_filter_response(src).expect("json fence is stripped");
        assert_eq!(f, Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }));
    }

    #[test]
    fn parses_plain_fenced_content() {
        let src = "```\n{\"Series\":{\"UpDays\":{\"n\":5,\"min_pct\":3.0}}}\n```";
        let f = parse_filter_response(src).expect("plain fence is stripped");
        assert_eq!(f, Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }));
    }

    #[test]
    fn parses_fence_with_noisy_whitespace() {
        // Tolerates leading/trailing blank lines around the fence and an
        // indented opening fence line.
        let src = "\n\n  ```json\n{\"Series\":{\"UpDays\":{\"n\":5,\"min_pct\":3.0}}}\n  ```  \n\n";
        let f = parse_filter_response(src).expect("noisy fence is stripped");
        assert_eq!(f, Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }));
    }

    #[test]
    fn parses_complex_and_shape() {
        let src = r#"{"And":[{"Meta":{"MarketCap":{"min":100.0,"max":5000.0}}},{"Series":{"UpDays":{"n":5,"min_pct":3.0}}}]}"#;
        let f = parse_filter_response(src).expect("complex And parses");
        assert_eq!(
            f,
            Filter::And(vec![
                Filter::Meta(MetaCond::MarketCap {
                    min: Some(100.0),
                    max: Some(5000.0)
                }),
                Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }),
            ])
        );
    }

    #[test]
    fn parses_cmp_with_factor_ref() {
        let src =
            r#"{"Series":{"Cmp":{"factor":"Close","op":"gt","value":{"Factor":{"Sma":20}}}}}"#;
        let f = parse_filter_response(src).expect("Cmp parses");
        assert_eq!(
            f,
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(20)),
            })
        );
    }

    #[test]
    fn rejects_invalid_json() {
        let err = parse_filter_response("this is not json").expect_err("invalid JSON must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_tag() {
        let err = parse_filter_response(r#"{"Bogus":[]}"#).expect_err("unknown tag must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }

    #[test]
    fn rejects_missing_field() {
        let src = r#"{"Series":{"UpDays":{"n":5}}}"#;
        let err = parse_filter_response(src).expect_err("missing field must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }

    #[test]
    fn rejects_semantically_invalid() {
        let err = parse_filter_response(r#"{"Series":{"UpDays":{"n":0,"min_pct":3.0}}}"#)
            .expect_err("semantically invalid must error");
        assert!(
            err.contains("UpDays"),
            "validation message must surface: {err}"
        );
    }

    #[test]
    fn rejects_empty_content() {
        let err = parse_filter_response("").expect_err("empty content must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }

    #[test]
    fn rejects_whitespace_only_content() {
        let err = parse_filter_response("  \n  ").expect_err("whitespace-only must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }

    #[test]
    fn rejects_fence_only_content() {
        let err = parse_filter_response("```").expect_err("bare fence must error");
        assert!(err.contains("invalid filter JSON"), "got: {err}");
    }
}
