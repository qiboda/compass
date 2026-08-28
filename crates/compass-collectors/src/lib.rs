//! Rust collectors crate — Python collectors migration target.
//!
//! Provides HTTP/TLS fetching (wreq/Chrome fingerprint), CSV output, Dolt
//! write/import, trading-calendar helpers, proxy-pool client and progress
//! tracking, as the infrastructure for A-share data collectors.

pub mod block_trade;
pub mod calendar;
pub mod config;
pub mod csv;
pub mod dolt;
pub mod eastmoney;
pub mod error;
pub mod http;
pub mod incremental;
pub mod progress;
pub mod proxy;

pub use error::{CollectError, Result};
