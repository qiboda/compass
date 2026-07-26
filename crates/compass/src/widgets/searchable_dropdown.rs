use compass_core::model::{Exchange, StockBasic};

#[allow(dead_code)]
pub fn filter_stocks<'a>(
    stocks: &'a [StockBasic],
    query: &str,
    exchange: &Exchange,
) -> Vec<&'a StockBasic> {
    let lower = query.trim().to_lowercase();
    let mut result: Vec<&StockBasic> = stocks
        .iter()
        .filter(|s| exchange.matches(s))
        .filter(|s| {
            if lower.is_empty() {
                return true;
            }
            s.symbol.to_lowercase().starts_with(&lower)
                || s.name.to_lowercase().contains(&lower)
        })
        .collect();
    result.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    result
}

pub struct StockPicker {
    pub filter_text: String,
    pub selected_symbol: String,
    pub selected_name: String,
    pub selected_exchange: String,
    pub popup_open: bool,
    cached_indices: Vec<usize>,
    last_filter_text: String,
}

impl StockPicker {
    pub fn new(default_symbol: &str, stock_list: &[StockBasic]) -> Self {
        let stock = stock_list.iter().find(|s| s.symbol == default_symbol);
        let name = stock.map(|s| s.name.clone()).unwrap_or_default();
        let exchange = stock
            .and_then(|s| s.exchange.clone())
            .unwrap_or_default();

        Self {
            filter_text: String::new(),
            selected_symbol: default_symbol.to_string(),
            selected_name: name,
            selected_exchange: exchange,
            popup_open: false,
            cached_indices: Vec::new(),
            last_filter_text: String::new(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        stock_list: &[StockBasic],
    ) {
        let display_text = format_display(&self.selected_exchange, &self.selected_symbol, &self.selected_name);

        let response = if self.popup_open {
            ui.text_edit_singleline(&mut self.filter_text)
        } else {
            let mut dummy = display_text.clone();
            let resp = ui.text_edit_singleline(&mut dummy);
            if resp.clicked() {
                self.popup_open = true;
                self.filter_text = display_text.clone();
            }
            resp
        };

        if self.popup_open {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.popup_open = false;
                return;
            }

            let needs_refilter = self.filter_text != self.last_filter_text;

            if needs_refilter {
                tracing::debug!(
                    filter = %self.filter_text,
                    "refiltering stock list"
                );
                let lower = self.filter_text.trim().to_lowercase();
                self.cached_indices = stock_list
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        if lower.is_empty() {
                            return true;
                        }
                        s.symbol.to_lowercase().starts_with(&lower)
                            || s.name.to_lowercase().contains(&lower)
                    })
                    .map(|(i, _)| i)
                    .collect();
                self.cached_indices.sort_by(|a, b| {
                    stock_list[*a].symbol.cmp(&stock_list[*b].symbol)
                });
                self.last_filter_text.clone_from(&self.filter_text);
            }

            let filtered_count = self.cached_indices.len();

            let max_rows = 12.min(filtered_count);
            let row_height = 20.0;
            let popup_height = 8.0 + max_rows as f32 * row_height;

            egui::Area::new(egui::Id::new("stock_picker_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(response.rect.left_bottom())
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(320.0);
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(popup_height)
                                .show_rows(ui, row_height, filtered_count, |ui, range| {
                                    for &idx in &self.cached_indices[range] {
                                        let stock = &stock_list[idx];
                                        let text = format!(
                                            "{} | {} | {}",
                                            stock.exchange.as_deref().unwrap_or(""),
                                            stock.symbol,
                                            stock.name
                                        );
                                        let selected = stock.symbol == self.selected_symbol;
                                        if selected {
                                            ui.colored_label(
                                                ui.visuals().selection.bg_fill,
                                                &text,
                                            );
                                        } else {
                                            let row = ui.selectable_label(false, &text);
                                            if row.clicked() {
                                                self.selected_symbol = stock.symbol.clone();
                                                self.selected_name = stock.name.clone();
                                                self.selected_exchange =
                                                    stock.exchange.clone().unwrap_or_default();
                                                self.popup_open = false;
                                                self.filter_text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_core::model::StockBasic;

    fn make_stock(symbol: &str, name: &str, exchange: &str) -> StockBasic {
        StockBasic {
            symbol: symbol.into(),
            name: name.into(),
            area: None,
            industry: None,
            market: None,
            exchange: Some(exchange.into()),
            list_date: None,
            delist_date: None,
        }
    }

    #[test]
    fn format_display_full() {
        assert_eq!(
            format_display("SZ", "000001", "平安银行"),
            "SZ | 000001 | 平安银行"
        );
    }

    #[test]
    fn format_display_no_name() {
        assert_eq!(format_display("SZ", "000001", ""), "SZ | 000001");
    }

    #[test]
    fn format_display_no_exchange() {
        assert_eq!(format_display("", "000001", "平安银行"), "000001 | 平安银行");
    }

    #[test]
    fn format_display_symbol_only() {
        assert_eq!(format_display("", "000001", ""), "000001");
    }

    #[test]
    fn stock_picker_starts_with_empty_cache() {
        let stocks = vec![
            make_stock("000001", "平安银行", "SZ"),
            make_stock("600519", "贵州茅台", "SH"),
        ];
        let picker = StockPicker::new("000001", &stocks);
        assert!(picker.cached_indices.is_empty());
    }

    #[test]
    fn stock_picker_detects_filter_change() {
        let stocks = vec![make_stock("000001", "平安银行", "SZ")];
        let mut picker = StockPicker::new("000001", &stocks);
        picker.filter_text = "平安".into();
        picker.popup_open = true;
        assert_ne!(picker.filter_text, picker.last_filter_text);
    }
}
                                    }
                                    if filtered_count == 0 {
                                        ui.label("No results");
                                    }
                                });
                        });
                });
        }
    }
}

fn format_display(exchange: &str, symbol: &str, name: &str) -> String {
    if name.is_empty() {
        if exchange.is_empty() {
            symbol.to_string()
        } else {
            format!("{exchange} | {symbol}")
        }
    } else if exchange.is_empty() {
        format!("{symbol} | {name}")
    } else {
        format!("{exchange} | {symbol} | {name}")
    }
}
