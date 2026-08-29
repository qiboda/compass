#![warn(missing_docs)]
//! Rust collectors crate — Python collectors migration target.
//!
//! Provides HTTP/TLS fetching (wreq/Chrome fingerprint), CSV output, Dolt
//! write/import, trading-calendar helpers, proxy-pool client and progress
//! tracking, as the infrastructure for A-share data collectors.

pub mod balance_sheet;
/// Block-trade (大宗交易) collector.
pub mod block_trade;
/// Trading-calendar helpers for auto-heal ranges.
pub mod calendar;
pub mod cash_flow;
pub mod check_proxy_pool;
/// Config.toml loading and path resolution.
pub mod config;
/// CSV writing helpers for raw collector output.
pub mod csv;
/// Dolt command execution and import helpers.
pub mod dolt;
/// Dragon-tiger list (龙虎榜) collector.
pub mod dragon;
/// EastMoney API record fetching.
pub mod eastmoney;
/// Unified error type for the collector crate.
pub mod error;
pub mod fin_indicators;
/// Shared F10 financial-statement fetch/import machinery.
pub mod financial;
pub mod freeproxy;
/// HTTP/TLS client and EastMoney rate limiter.
pub mod http;
pub mod income;
/// Incremental UPDATE_DATE-based fetching helpers.
pub mod incremental;
pub mod index_daily;
/// Institution-survey (机构调研) collector.
pub mod institution_survey;
pub mod keepalive;
/// Main-capital-flow (主力资金流) collector.
pub mod main_flow;
pub mod orchestrate;
/// JSON progress tracking for long-running collectors.
pub mod progress;
/// Proxy-pool client for the local proxy_pool API.
pub mod proxy;
/// A-share stock basic info collector from EastMoney.
pub mod stock_basic;
pub mod stock_basic_official;
pub mod timing;

pub use error::{CollectError, Result};
