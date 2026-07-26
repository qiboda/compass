//! A-share stock code conversion and exchange inference.

/// Extract the exchange code for a given A-share stock code.
///
/// Supports explicit market prefixes (case-insensitive):
/// - `sh.000001` → `"SH"` (Shanghai, 上证指数)
/// - `sz.000001` → `"SZ"` (Shenzhen, 平安银行)
/// - `bj.8xxxxx` → `"BJ"` (Beijing, 北交所)
///
/// ## ⚠️ Heuristic fallback (inaccurate — DO NOT RELY ON)
///
/// When no explicit prefix is present, this function guesses the exchange
/// from the first digit of the code:
/// - `6xxxxx` → `"SH"`, `8xxxxx` → `"BJ"`, everything else → `"SZ"`
///
/// **This heuristic is fundamentally inaccurate.** Many SZ/BJ stocks share
/// code ranges with SH stocks. For example, `000001` could be SZ (平安银行)
/// or SH (上证指数) — there is no way to distinguish from the code alone.
///
/// **Always prefer explicit prefixes (`sh.`/`sz.`/`bj.`) from stock metadata.**
pub fn to_exchange(code: &str) -> &str {
    let lower = code.to_lowercase();

    if lower.starts_with("sh.") {
        return "SH";
    }
    if lower.starts_with("sz.") {
        return "SZ";
    }
    if lower.starts_with("bj.") {
        return "BJ";
    }

    // ⚠️ Heuristic fallback — inaccurate. Prefer explicit prefix from stock metadata.
    tracing::warn!(
        code = %code,
        "to_exchange: using inaccurate heuristic fallback — exchange should be explicit"
    );

    if code.starts_with('6') {
        "SH"
    } else if code.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

/// Convert a bare stock code to full ts_code format: `"{code}.{exchange}"`.
///
/// Examples:
/// - `"000001"` → `"000001.SZ"`
/// - `"600519"` → `"600519.SH"`
/// - `"sh.000001"` → `"000001.SH"`
/// - `"bj.830799"` → `"830799.BJ"`
pub fn to_ts_code(symbol: &str) -> String {
    let lower = symbol.to_lowercase();

    // Handle explicit prefixes: extract the bare code part.
    if let Some(code) = lower.strip_prefix("sh.") {
        return format!("{}.SH", code);
    }
    if let Some(code) = lower.strip_prefix("sz.") {
        return format!("{}.SZ", code);
    }
    if let Some(code) = lower.strip_prefix("bj.") {
        return format!("{}.BJ", code);
    }

    let exchange = to_exchange(symbol);
    format!("{}.{}", symbol, exchange)
}

/// Parse an explicit exchange prefix from a qualified symbol.
///
/// Returns `(exchange, bare_code)`:
/// - `"sz.000001"` → `("SZ", "000001")`
/// - `"SH.600519"` → `("SH", "600519")`
/// - `"SZ000001"` → `("SZ", "000001")` (Dolt-native, no dot)
///
/// Returns `("", code)` if no prefix is found.
pub fn parse_explicit_prefix(code: &str) -> (&str, &str) {
    if code.len() >= 3 && code[..3].eq_ignore_ascii_case("sh.") {
        ("SH", &code[3..])
    } else if code.len() >= 3 && code[..3].eq_ignore_ascii_case("sz.") {
        ("SZ", &code[3..])
    } else if code.len() >= 3 && code[..3].eq_ignore_ascii_case("bj.") {
        ("BJ", &code[3..])
    } else if let Some(rest) = code.strip_prefix("SH") {
        ("SH", rest)
    } else if let Some(rest) = code.strip_prefix("SZ") {
        ("SZ", rest)
    } else if let Some(rest) = code.strip_prefix("BJ") {
        ("BJ", rest)
    } else {
        ("", code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // -----------------------------------------------------------------------
    // to_exchange
    // -----------------------------------------------------------------------

    #[rstest]
    #[case("600519", "SH")]
    #[case("688001", "SH")]
    #[case("601318", "SH")]
    #[case("000001", "SZ")]
    #[case("000002", "SZ")]
    #[case("300750", "SZ")]
    #[case("002415", "SZ")]
    #[case("8xxxxx", "BJ")]
    #[case("sh.000001", "SH")]
    #[case("SH.000001", "SH")]
    #[case("sz.000001", "SZ")]
    #[case("sh.688001", "SH")]
    #[case("bj.830799", "BJ")]
    fn to_exchange_returns_correct(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_exchange(input), expected);
    }

    // -----------------------------------------------------------------------
    // to_ts_code
    // -----------------------------------------------------------------------

    #[rstest]
    #[case("000001", "000001.SZ")]
    #[case("600519", "600519.SH")]
    #[case("688001", "688001.SH")]
    #[case("300750", "300750.SZ")]
    #[case("8xxxxx", "8xxxxx.BJ")]
    #[case("sh.000001", "000001.SH")]
    #[case("SH.000001", "000001.SH")]
    #[case("sz.000001", "000001.SZ")]
    #[case("bj.830799", "830799.BJ")]
    fn to_ts_code_returns_correct(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_ts_code(input), expected);
    }

    #[test]
    fn to_ts_code_unknown_heuristic_defaults_to_sz() {
        assert_eq!(to_ts_code("FOOBAR"), "FOOBAR.SZ");
        assert_eq!(to_ts_code("999999"), "999999.SZ");
    }

    #[test]
    fn parse_explicit_prefix_dot_format() {
        assert_eq!(parse_explicit_prefix("sz.000001"), ("SZ", "000001"));
        assert_eq!(parse_explicit_prefix("sh.600519"), ("SH", "600519"));
        assert_eq!(parse_explicit_prefix("bj.830799"), ("BJ", "830799"));
    }

    #[test]
    fn parse_explicit_prefix_dolt_native() {
        assert_eq!(parse_explicit_prefix("SZ000001"), ("SZ", "000001"));
        assert_eq!(parse_explicit_prefix("SH600519"), ("SH", "600519"));
        assert_eq!(parse_explicit_prefix("BJ830799"), ("BJ", "830799"));
    }

    #[test]
    fn parse_explicit_prefix_bare_code_returns_empty() {
        assert_eq!(parse_explicit_prefix("000001"), ("", "000001"));
        assert_eq!(parse_explicit_prefix("600519"), ("", "600519"));
    }
}
