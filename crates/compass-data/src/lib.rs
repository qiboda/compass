#![warn(missing_docs)]
//! compass-data — Dolt → Parquet data pipeline binaries.
//!
//! Provides `import_dolt` / `import_compass` (Dolt → Parquet exports),
//! `validate` (stock_daily gap checks) and the `compass-data` CLI binary.

pub mod import_compass;
pub mod import_dolt;
pub mod validate;
