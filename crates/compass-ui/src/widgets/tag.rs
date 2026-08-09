//! Tag atom: short labels for exchanges / boards / industries (design doc
//! §5.1 `Tag`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Rect, Response, RichText, Sense, Ui};

/// Tag variant; the `Exchange` variant auto-colors by the exchange code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TagVariant {
    /// Exchange badge — auto-colored for `SH` / `SZ` / `BJ`.
    Exchange,
    /// Board tag (accent tint).
    Board,
    /// Industry tag (secondary tint).
    Industry,
    /// Custom color tag.
    #[default]
    Custom,
}

/// Short pill tag (20 px tall, 9–11 px text).
pub struct Tag<'a> {
    tokens: &'a ThemeTokens,
    text: &'a str,
    variant: TagVariant,
    color: Option<Color32>,
}

impl<'a> Tag<'a> {
    /// Create a tag for the given theme and text.
    pub fn new(tokens: &'a ThemeTokens, text: &'a str) -> Self {
        Self {
            tokens,
            text,
            variant: TagVariant::Custom,
            color: None,
        }
    }

    /// Set the tag variant.
    pub fn variant(mut self, variant: TagVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the color (used by `Custom` and as the tint base otherwise).
    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// (background, text color) for this tag per the design doc.
    pub fn colors(&self) -> (Color32, Color32) {
        let c = &self.tokens.color;
        match self.variant {
            TagVariant::Exchange => (exchange_color(self.text), Color32::WHITE),
            TagVariant::Board => {
                let base = self.color.unwrap_or(c.accent);
                (tint(base, 0.18), base)
            }
            TagVariant::Industry => {
                let base = self.color.unwrap_or(c.text_secondary);
                (tint(base, 0.18), base)
            }
            TagVariant::Custom => {
                let base = self.color.unwrap_or(c.accent);
                (tint(base, 0.18), base)
            }
        }
    }

    /// Show the tag pill and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let tokens = self.tokens;
        let (bg, fg) = self.colors();
        // Measure the label, allocate an exact rect, then paint the pill
        // background. This must NOT use `Frame::show`: a Frame reports its
        // response rect to the parent layout, widening a wrapping parent's
        // max_rect so `horizontal_wrapped` never wraps (SEPA theme tags
        // sprawled on one line, overflowing the 280px panel). The label is
        // placed with `ui.put` so it stays in the fixed rect (accesskit
        // visible) without affecting layout.
        let galley = ui.painter().layout_no_wrap(
            self.text.to_owned(),
            egui::FontId::proportional(tokens.typography.caption),
            fg,
        );
        let padding = egui::vec2(12.0, 6.0);
        let size = galley.size() + padding;
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(tokens.radius.pill.round() as u8),
            bg,
        );
        let text_rect = Rect::from_min_size(rect.min + egui::vec2(6.0, 2.0), galley.size());
        ui.put(
            text_rect,
            egui::Label::new(
                RichText::new(self.text)
                    .size(tokens.typography.caption)
                    .color(fg),
            ),
        );
        response
    }
}

/// The design-mandated exchange badge colors: SH blue / SZ green / BJ purple.
pub fn exchange_color(text: &str) -> Color32 {
    match text.trim().to_uppercase().as_str() {
        "SH" => Color32::from_rgb(0x29, 0x62, 0xFF),
        "SZ" => Color32::from_rgb(0x0E, 0x9F, 0x6E),
        "BJ" => Color32::from_rgb(0x8B, 0x5C, 0xF6),
        _ => Color32::from_rgb(0x29, 0x62, 0xFF),
    }
}

/// Mix `base` over a transparent background at the given alpha (0..=1).
/// Exported for composite chips (e.g. the SEPA thermometer indicators) that
/// build their own pill with the tag's tint convention.
pub fn tint(base: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (base.r() as f32 * alpha) as u8,
        (base.g() as f32 * alpha) as u8,
        (base.b() as f32 * alpha) as u8,
        (255.0 * alpha) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Exchange colors follow the design spec for SH / SZ / BJ.
    #[test]
    fn exchange_colors_follow_design() {
        assert_eq!(exchange_color("SH"), Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(exchange_color("SZ"), Color32::from_rgb(0x0E, 0x9F, 0x6E));
        assert_eq!(exchange_color("BJ"), Color32::from_rgb(0x8B, 0x5C, 0xF6));
        // Case-insensitive and unknown codes fall back to the default blue.
        assert_eq!(exchange_color("sh"), Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(exchange_color("ZZ"), Color32::from_rgb(0x29, 0x62, 0xFF));
    }

    /// Exchange tags render white text on the exchange color.
    #[test]
    fn exchange_tag_uses_white_text() {
        let tokens = ThemeTokens::dark();
        let tag = Tag::new(&tokens, "SH").variant(TagVariant::Exchange);
        let (bg, fg) = tag.colors();
        assert_eq!(bg, exchange_color("SH"));
        assert_eq!(fg, Color32::WHITE);
    }

    /// Custom tags tint the base color at low alpha.
    #[test]
    fn custom_tag_tints_base_color() {
        let tokens = ThemeTokens::dark();
        let tag = Tag::new(&tokens, "X").color(Color32::from_rgb(0xFF, 0x00, 0x00));
        let (bg, fg) = tag.colors();
        assert_eq!(fg, Color32::from_rgb(0xFF, 0x00, 0x00));
        assert!(bg.a() < 128, "tint background must be translucent");
    }

    /// The tag text is rendered and queryable.
    #[test]
    fn tag_text_is_queryable() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Tag::new(&tokens, "SH")
                .variant(TagVariant::Exchange)
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("SH");
    }

    /// Many tags inside a wrapping layout must wrap onto multiple rows
    /// instead of sprawling one line past the container edge (SEPA theme
    /// tags: "海康威视" carries 35+ concepts; Frame::show widened the
    /// wrapped parent's max_rect so nothing wrapped).
    #[test]
    fn many_tags_wrap_within_container_width() {
        let tokens = ThemeTokens::dark();
        let themes = [
            "AI应用",
            "HS300_",
            "MSCI中国",
            "中特估",
            "云计算",
            "人工智能",
            "光纤概念",
            "大数据",
            "大盘股",
            "央国企改革",
            "央视50_",
            "存储芯片",
            "安防概念",
            "新型工业化",
            "无人机",
            "无人驾驶",
            "昨日高振幅",
            "智慧城市",
            "机器人概念",
            "权重股",
            "标准普尔",
            "深成500",
            "深股通",
            "深证100R",
            "物联网",
            "生物识别",
            "科技风格",
            "茅指数",
            "虚拟现实",
            "融资融券",
            "行业龙头",
            "超清视频",
            "趋势股",
            "车联网(车路云)",
            "边缘计算",
        ];
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(280.0, 600.0))
            .build_ui(|ui| {
                ui.horizontal_wrapped(|ui| {
                    for theme in themes {
                        Tag::new(&tokens, theme)
                            .variant(TagVariant::Custom)
                            .show(ui);
                    }
                });
            });
        harness.run_steps(2);

        // Every tag's rect must stay inside the 280px container.
        let all = harness.query_all_by_label_contains("").collect::<Vec<_>>();
        let mut offenders: Vec<String> = Vec::new();
        for node in &all {
            let label = node.value().unwrap_or_default();
            if !themes.contains(&label.as_str()) {
                continue;
            }
            if node.rect().max.x > 280.0 {
                offenders.push(format!("'{label}' right {:.1} > 280", node.rect().max.x));
            }
        }
        assert!(
            offenders.is_empty(),
            "tags must wrap inside 280px, got overflow: {offenders:?}"
        );

        // More than one row must have been produced (35 tags cannot fit one line).
        let mut ys: Vec<f32> = all
            .iter()
            .filter(|n| themes.contains(&n.value().unwrap_or_default().as_str()))
            .map(|n| n.rect().min.y)
            .collect();
        ys.sort_by(|a, b| a.total_cmp(b));
        ys.dedup_by(|a, b| (*a - *b).abs() < 2.0);
        assert!(
            ys.len() > 1,
            "35 tags must wrap onto multiple rows, got {} row(s)",
            ys.len()
        );
    }
}
