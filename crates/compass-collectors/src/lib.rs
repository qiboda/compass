//! Rust collectors crate — Python collectors migration target.
//!
//! Provides HTTP/TLS fetching (wreq/Chrome fingerprint), CSV output, Dolt
//! write/import, trading-calendar helpers, proxy-pool client and progress
//! tracking, as the infrastructure for A-share data collectors.

pub mod balance_sheet;
pub mod block_trade;
pub mod calendar;
pub mod cash_flow;
pub mod check_proxy_pool;
pub mod config;
pub mod csv;
pub mod dolt;
pub mod dragon;
pub mod eastmoney;
pub mod error;
pub mod fin_indicators;
pub mod financial;
pub mod freeproxy;
pub mod http;
pub mod income;
pub mod incremental;
pub mod index_daily;
pub mod institution_survey;
pub mod keepalive;
pub mod main_flow;
pub mod orchestrate;
pub mod progress;
pub mod proxy;
pub mod stock_basic;
pub mod stock_basic_official;

pub use error::{CollectError, Result};
