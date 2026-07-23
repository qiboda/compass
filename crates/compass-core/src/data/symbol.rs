//! A-share stock code conversion and exchange inference.

/// Extract the exchange code for a given A-share stock code.
///
/// Supports explicit market prefixes for disambiguation (case-insensitive):
/// - `sh.000001` → `"SH"` (Shanghai, 上证指数)
/// - `sz.000001` → `"SZ"` (Shenzhen, 平安银行)
/// - `bj.8xxxxx` → `"BJ"` (Beijing, 北交所)
///
/// Without prefix, infers the exchange from A-share code ranges:
/// - `6xxxxx` → `"SH"` (Shanghai: 主板 600/601/603/605, 科创板 688)
/// - `000xxx`–`004xxx` → `"SZ"` (Shenzhen 主板)
/// - `300xxx`, `301xxx` → `"SZ"` (创业板)
/// - `002xxx`, `003xxx` → `"SZ"` (Shenzhen)
/// - `8xxxxx` → `"BJ"` (北交所)
/// - everything else → `"SZ"` (default)
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

    // Heuristic: strip any prefix before inferring from the numeric code.
    // The code may contain a prefix like "sh."; after stripping, we have the numeric part.
    let numeric = code;

    if numeric.starts_with('6') {
        "SH"
    } else if numeric.starts_with('8') {
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
}
