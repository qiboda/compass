//! Data provider abstractions and implementations.
//!
//! The module contains the trait system ([`provider::DataProvider`],
//! [`provider::DataWriter`], [`provider::NegativeCache`]) and concrete
//! implementations for DuckDB, Parquet, and synthetic test data.

pub mod duckdb;
pub mod parquet;
pub mod provider;
pub mod symbol;
mod synthetic;
