//! A-share stock code prefix parsing.
//!
//! Symbols are exchange-prefixed throughout the data pipeline (e.g.
//! `SZ000001`); `parse_explicit_prefix` splits a qualified symbol into its
//! exchange code and bare 6-digit code.

/// Parse an explicit exchange prefix from a qualified symbol.
///
/// Returns `(exchange, bare_code)`:
/// - `"sz.000001"` → `("SZ", "000001")`
/// - `"SH.600519"` → `("SH", "600519")`
/// - `"SZ000001"` → `("SZ", "000001")` (Dolt-native, no dot)
///
/// Returns `("", code)` if no prefix is found.
pub fn parse_explicit_prefix(code: &str) -> (&str, &str) {
    if code.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("sh.")) {
        ("SH", &code[3..])
    } else if code.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("sz.")) {
        ("SZ", &code[3..])
    } else if code.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("bj.")) {
        ("BJ", &code[3..])
    } else if code.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("SH")) {
        ("SH", &code[2..])
    } else if code.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("SZ")) {
        ("SZ", &code[2..])
    } else if code.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("BJ")) {
        ("BJ", &code[2..])
    } else {
        ("", code)
    }
}

/// Infer the exchange prefix for a legacy unprefixed 6-digit code
/// (pre-D10 data, mirroring the pre-D9 heuristic: 6→SH, 8/92/43→BJ,
/// else→SZ — the 43xxxx segment covers real BJ codes like 430047).
/// Non-digit or non-6-digit values return `None`.
pub fn infer_exchange_prefix(code: &str) -> Option<&'static str> {
    if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
        if code.starts_with('6') {
            Some("SH")
        } else if code.starts_with('8') || code.starts_with("43") || code.starts_with("92") {
            Some("BJ")
        } else {
            Some("SZ")
        }
    } else {
        None
    }
}

/// Exchange code from a symbol's explicit prefix, falling back to the
/// legacy bare-code shape heuristic for pre-migration data.
pub fn exchange_of_symbol(symbol: &str) -> &str {
    let (exchange, _) = parse_explicit_prefix(symbol);
    if !exchange.is_empty() {
        return exchange;
    }
    // Bare-code fallback delegates to the full shape heuristic (6→SH,
    // 8/92→BJ, else→SZ) so the two can never drift.
    infer_exchange_prefix(symbol).unwrap_or("SZ")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_explicit_prefix_dolt_native_case_insensitive() {
        assert_eq!(parse_explicit_prefix("sz000001"), ("SZ", "000001"));
        assert_eq!(parse_explicit_prefix("SZ000001"), ("SZ", "000001"));
        assert_eq!(parse_explicit_prefix("sh600519"), ("SH", "600519"));
        assert_eq!(parse_explicit_prefix("Sh600519"), ("SH", "600519"));
        assert_eq!(parse_explicit_prefix("bj830799"), ("BJ", "830799"));
    }

    #[test]
    fn parse_explicit_prefix_bare_code_returns_empty() {
        assert_eq!(parse_explicit_prefix("000001"), ("", "000001"));
        assert_eq!(parse_explicit_prefix("600519"), ("", "600519"));
    }

    #[test]
    fn parse_explicit_prefix_non_ascii_does_not_panic() {
        // Multi-byte UTF-8 must not be byte-sliced mid-char (regression:
        // `code[..2]`/`code[..3]` panicked on non-char-boundary indices,
        // crashing GUI startup when a user config carries a stock name).
        assert_eq!(parse_explicit_prefix("平安银行"), ("", "平安银行"));
        assert_eq!(parse_explicit_prefix("贵"), ("", "贵"));
        // An ASCII prefix before a name still parses without panic.
        assert_eq!(parse_explicit_prefix("SZ平安"), ("SZ", "平安"));
        assert_eq!(parse_explicit_prefix("SH平"), ("SH", "平"));
    }

    #[test]
    fn infer_exchange_prefix_bj_43_segment() {
        // Real BJ (北交所/新三板) codes include the 43xxxx segment
        // (e.g. 430047, 436149); the official collector classifies
        // 4/8/9-prefixed codes as BJ.
        assert_eq!(infer_exchange_prefix("430047"), Some("BJ"));
        assert_eq!(infer_exchange_prefix("436149"), Some("BJ"));
        assert_eq!(infer_exchange_prefix("920000"), Some("BJ"));
        assert_eq!(infer_exchange_prefix("600519"), Some("SH"));
        assert_eq!(infer_exchange_prefix("830799"), Some("BJ"));
        assert_eq!(infer_exchange_prefix("000001"), Some("SZ"));
    }

    #[test]
    fn exchange_of_symbol_explicit_prefix_wins() {
        assert_eq!(exchange_of_symbol("SZ000001"), "SZ");
        assert_eq!(exchange_of_symbol("sh600519"), "SH");
        assert_eq!(exchange_of_symbol("BJ830799"), "BJ");
    }

    #[test]
    fn exchange_of_symbol_bare_code_shape_fallback() {
        // Legacy pre-migration bare codes use the shape heuristic.
        assert_eq!(exchange_of_symbol("600519"), "SH");
        assert_eq!(exchange_of_symbol("830799"), "BJ");
        assert_eq!(
            exchange_of_symbol("920001"),
            "BJ",
            "92-prefixed codes are BJ"
        );
        assert_eq!(exchange_of_symbol("000001"), "SZ");
    }
}
