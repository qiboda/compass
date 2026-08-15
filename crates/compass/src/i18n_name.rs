//! Data-name i18n helpers (epic #266 B3).
//!
//! Data names (index/industry/concept/theme) carry a Chinese name and an
//! optional English name from the data layer (`name_en` / `industry_en`).
//! The GUI resolves which one to render from the current locale:
//! `locale == "en"` + a non-empty English name → English; everything else →
//! Chinese. An empty `Some("")` is treated as unmapped (legacy/blank row
//! artifact) and falls back to Chinese — never renders a blank label.

/// Resolve the display name for the given locale.
///
/// - `locale == "en"` and `en` is `Some(non-empty)` → `en`
/// - `locale == "en"` and `en` is `None` or `Some("")` → `zh`
/// - any other locale → `zh` (the English name never leaks into non-en UI)
pub fn display_name(locale: &str, zh: &str, en: Option<&str>) -> String {
    match en {
        Some(en) if locale == "en" && !en.is_empty() => en.to_string(),
        _ => zh.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_locale_with_en_uses_en() {
        assert_eq!(
            display_name("en", "上证指数", Some("SSE Composite")),
            "SSE Composite"
        );
    }

    #[test]
    fn en_locale_without_en_falls_back_zh() {
        assert_eq!(display_name("en", "上证指数", None), "上证指数");
    }

    #[test]
    fn en_locale_with_empty_en_falls_back_zh() {
        assert_eq!(display_name("en", "上证指数", Some("")), "上证指数");
    }

    #[test]
    fn zh_locale_never_uses_en() {
        assert_eq!(
            display_name("zh", "上证指数", Some("SSE Composite")),
            "上证指数"
        );
        assert_eq!(display_name("zh", "上证指数", None), "上证指数");
    }

    #[test]
    fn unknown_locale_falls_back_zh() {
        assert_eq!(
            display_name("fr", "上证指数", Some("SSE Composite")),
            "上证指数"
        );
    }
}
