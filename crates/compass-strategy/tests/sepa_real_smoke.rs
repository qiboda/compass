//! Real-data smoke test for `run_sepa` (F3 gate, plan 2).
//!
//! Reads the real Parquet directory and runs the SEPA engine over the latest
//! trading day, verifying the engine completes without panic and produces
//! plausible scores. Ignored by default; run with:
//!
//! ```sh
//! cargo test -p compass-strategy --test sepa_real_smoke -- --ignored --nocapture
//! ```
//!
//! Uses the default Parquet dir from config; override with `SEPA_PARQUET_DIR`.

use chrono::NaiveDate;
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::run_sepa;
use compass_types::SepaQuery;

#[test]
#[ignore]
fn run_sepa_over_real_parquet() {
    let dir = std::env::var("SEPA_PARQUET_DIR").unwrap_or_else(|_| {
        "/data/compass-data/parquet_data".to_string()
    });
    let reader = ParquetReader::new(&dir).expect("open real parquet dir");
    // Latest real trading day in the data (capital_main_flow smoke import used
    // 2026-07-31); run_sepa uses its own 550-day window.
    let now = NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
    let query = SepaQuery { top_n: 10 };

    let data = run_sepa(&query, &reader, now).expect("run_sepa must not error");

    // Engine completed and produced a thermometer.
    assert!(
        (0.0..=100.0).contains(&data.thermometer.score),
        "thermometer score out of range: {}",
        data.thermometer.score
    );
    assert_eq!(data.thermometer.indicators.len(), 5);
    println!(
        "thermometer: score={:.1} position={} pct={:.0}",
        data.thermometer.score, data.thermometer.position, data.thermometer.position_pct
    );
    for ind in &data.thermometer.indicators {
        println!("  {}: {} (heat {:.2})", ind.label, ind.value_text, ind.heat);
    }

    println!("top {} rows:", data.rows.len());
    for row in &data.rows {
        assert!(
            (0.0..=100.0).contains(&row.total_score),
            "row score out of range: {}",
            row.total_score
        );
        assert!((-3.75..=0.0).contains(&row.risk), "risk out of range: {}", row.risk);
        println!(
            "#{:>2} {} {} score={:5.2} t={:4.1} th={:4.1} c={:4.1} p={:4.1} r={:5.2} price={:.2} chg={:+.2}%",
            row.rank, row.symbol, row.name, row.total_score, row.trend, row.theme, row.capital,
            row.pattern, row.risk, row.latest_price, row.change_pct
        );
    }

    assert!(data.rows.len() <= 10, "top_n truncation respected");
}
