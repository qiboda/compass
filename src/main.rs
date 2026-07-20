use chrono::{Duration, Utc};
use egui_charts::model::{Bar, BarData};
use egui_charts::theme::Theme;
use egui_charts::widget::Chart;
use egui_charts::ChartType;
use rand::Rng;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Compass — Stock Chart",
        options,
        Box::new(|_cc| Ok(Box::new(CompassApp::new()))),
    )
}

struct CompassApp {
    chart: Chart,
}

impl CompassApp {
    fn new() -> Self {
        let bars = generate_synthetic_bars(200, 100.0);
        let data = BarData::from_bars(bars);

        let mut chart = Chart::new(data);
        chart.set_chart_type(ChartType::Candles);
        chart.set_visible_bars(100);
        chart.set_symbol("COMPASS");
        chart.set_timeframe_label("1d");

        Self { chart }
    }
}

impl eframe::App for CompassApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::dark();
        egui_charts::theme::apply_to_egui(ctx, &theme);

        egui::CentralPanel::default().show(ctx, |ui| {
            self.chart.show(ui);
        });
    }
}

fn generate_synthetic_bars(count: usize, start_price: f64) -> Vec<Bar> {
    let mut rng = rand::rng();
    let mut bars = Vec::with_capacity(count);
    let mut price = start_price;
    let now = Utc::now();

    for i in 0..count {
        let time = now - Duration::days((count - i) as i64);

        let change: f64 = rng.random_range(-2.0..2.0);
        let open = price;
        let close = price + change;
        let high = open.max(close) + rng.random_range(0.0..1.5);
        let low = open.min(close) - rng.random_range(0.0..1.5);
        let volume = rng.random_range(1_000_000.0..10_000_000.0);

        bars.push(Bar::new(time, open, high, low, close, volume));
        price = close;
    }

    bars
}
