# AGENTS.md — compass

Stock chart desktop application built with egui.

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- **GUI app** — needs a display server (X11/Wayland). `cargo run` opens a window.
- Dependencies: `egui 0.33`, `eframe 0.33`, `egui-charts 0.2`, `chrono`, `rand`.

## Commands

```sh
cargo build
cargo run          # opens stock chart window
cargo test
cargo fmt
cargo clippy
```

## Architecture

- Single binary crate (`src/main.rs`). `Cargo.lock` committed.
- `CompassApp` holds a `Chart` widget — all rendering via `egui-charts`.
- Synthetic OHLCV bars are generated on startup (`generate_synthetic_bars`).
- Chart type: candles, dark theme via `egui_charts::theme::apply_to_egui`.

## egui-charts API

- `Bar::new(time, open, high, low, close, volume)` — OHLCV bar
- `BarData::from_bars(bars)` — dataset wrapper
- `Chart::new(data)` — interactive chart widget (pan, zoom, crosshair)
- `chart.set_chart_type(ChartType::Candles)` — candlestick display
- `chart.show(ui)` — render inside any `egui::Ui`
