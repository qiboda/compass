#![warn(missing_docs)]

//! A-share stock data library for the Compass chart application.
//!
//! Provides the data model, provider abstractions, and I/O implementations
//! shared by the GUI (`compass`) and CLI (`compass-data`) binaries.
//!
//! # Architecture
//!
//! Data access is abstracted behind three traits in [`data::provider`]:
//! - `DataProvider` — read-only fetch and search
//! - `DataWriter` — write-through persistence
//! - `NegativeCache` — cache for known-empty symbols
//!
//! [`data::CachedProvider`] implements a read-through cache pattern,
//! checking a local DuckDB first before falling back to remote APIs.

pub mod data;
pub mod model;
