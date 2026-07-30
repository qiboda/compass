use crate::state::SharedState;
use crate::theme::CompassTheme;
use egui_charts::model::BarData;
use egui_charts::widget::Chart;
use egui_charts::ChartType;
use egui_citizen::{Citizen, CitizenId, CitizenState};

/// Chart panel citizen — renders an interactive OHLCV candlestick chart.
///
/// Reads `bars` from `SharedState` reactively and updates the chart
/// widget whenever data is available. The active theme's chart colors
/// (candles, grid, crosshair) are applied each frame via `app_theme`.
pub struct ChartCitizen {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    chart: Chart,
}

impl Citizen for ChartCitizen {
    fn id(&self) -> &CitizenId {
        &self.citizen_id
    }

    fn citizen_state(&self) -> &CitizenState {
        &self.citizen_state
    }

    fn citizen_state_mut(&mut self) -> &mut CitizenState {
        &mut self.citizen_state
    }
}

impl ChartCitizen {
    /// Creates a new `ChartCitizen` with an empty candlestick chart.
    ///
    /// The chart is pre-configured with 100 visible bars, symbol label
    /// "COMPASS", and "1d" timeframe label — the real data and theme
    /// colors are applied reactively on each frame in `show`.
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState) -> Self {
        let bars: Vec<egui_charts::model::Bar> = Vec::new();
        let data = BarData::from_bars(bars);
        let mut chart = Chart::new(data);
        chart.set_chart_type(ChartType::Candles);
        chart.set_visible_bars(100);
        chart.set_symbol("COMPASS");
        chart.set_timeframe_label("1d");
        Self {
            citizen_id,
            citizen_state,
            chart,
        }
    }

    /// Renders the chart panel with the given theme.
    ///
    /// Applies `app_theme` chart colors (candles, grid, crosshair) each
    /// frame, reads `bars` from shared state, and delegates rendering to
    /// the egui-charts widget.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        state: &SharedState,
        app_theme: &CompassTheme,
    ) {
        app_theme.apply_to_chart(&mut self.chart);

        let bars = state.bars.get();
        if !bars.is_empty() {
            self.chart.update_data(BarData::from_bars(bars));
        }

        self.chart.set_timeframe_label(&state.timeframe.get());
        self.chart.show(ui);
    }
}
