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
    if code.len() >= 3 && code[..3].eq_ignore_ascii_case("sh.") {
        ("SH", &code[3..])
    } else if code.len() >= 3 && code[..3].eq_ignore_ascii_case("sz.") {
        ("SZ", &code[3..])
    } else if code.len() >= 3 && code[..3].eq_ignore_ascii_case("bj.") {
        ("BJ", &code[3..])
    } else if code.len() >= 2 && code[..2].eq_ignore_ascii_case("SH") {
        ("SH", &code[2..])
    } else if code.len() >= 2 && code[..2].eq_ignore_ascii_case("SZ") {
        ("SZ", &code[2..])
    } else if code.len() >= 2 && code[..2].eq_ignore_ascii_case("BJ") {
        ("BJ", &code[2..])
    } else {
        ("", code)
    }
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
}
