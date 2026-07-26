use compass_core::model::{Exchange, StockBasic};

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
            s.symbol.starts_with(&lower) || s.name.to_lowercase().contains(&lower)
        })
        .collect();
    result.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    result
}

pub struct StockPicker {
    pub filter_text: String,
    pub selected_symbol: String,
    pub selected_name: String,
    pub popup_open: bool,
}

impl StockPicker {
    pub fn new(default_symbol: &str, stock_list: &[StockBasic]) -> Self {
        let name = stock_list
            .iter()
            .find(|s| s.symbol == default_symbol)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        Self {
            filter_text: String::new(),
            selected_symbol: default_symbol.to_string(),
            selected_name: name,
            popup_open: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        stock_list: &[StockBasic],
        exchange: &Exchange,
    ) {
        let display_text = if self.selected_name.is_empty() {
            &self.selected_symbol
        } else {
            &self.selected_name
        };

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
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.popup_open = false;
                return;
            }

            let filtered = filter_stocks(stock_list, &self.filter_text, exchange);

            let max_rows = 12.min(filtered.len());
            let row_height = 20.0;
            let popup_height = 8.0 + max_rows as f32 * row_height;

            egui::Area::new(egui::Id::new("stock_picker_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(response.rect.left_bottom())
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(popup_height)
                                .show(ui, |ui| {
                                    for stock in &filtered {
                                        let text = format!(
                                            "{} | {} | {}",
                                            stock.exchange.as_deref().unwrap_or(""),
                                            stock.symbol,
                                            stock.name
                                        );
                                        let row = ui.selectable_label(
                                            stock.symbol == self.selected_symbol,
                                            &text,
                                        );
                                        if row.clicked() {
                                            self.selected_symbol = stock.symbol.clone();
                                            self.selected_name = stock.name.clone();
                                            self.popup_open = false;
                                            self.filter_text.clear();
                                        }
                                    }
                                    if filtered.is_empty() {
                                        ui.label("No results");
                                    }
                                });
                        });
                });
        }
    }
}
