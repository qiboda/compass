use std::future::Future;

use chrono::{DateTime, Utc};
use compass_rs::data::provider::{DataError, DataProvider};
use egui_charts::model::Bar;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Retryable error classification
// ---------------------------------------------------------------------------

/// Returns `true` when an error is transient and worth retrying (network
/// failures, rate-limit responses). Permanent errors (parse, no-data, database)
/// propagate immediately.
fn is_retryable(err: &DataError) -> bool {
    matches!(
        err,
        DataError::Network(_) | DataError::RateLimited(_)
    )
}

// ---------------------------------------------------------------------------
// Generic retry helper
// ---------------------------------------------------------------------------

/// Execute `op` up to `max_attempts` times with exponential backoff.
///
/// * `max_attempts` — total calls (1 + retries).  Must be ≥ 1.
/// * Backoff schedule: 1 s, 2 s, 4 s, … between retries.
///
/// Only [`DataError::Network`] and [`DataError::RateLimited`] trigger a retry;
/// all other errors propagate immediately.
///
/// # Panics
/// Panics if `max_attempts` is 0.
pub async fn fetch_with_retry<F, Fut, T>(
    op: F,
    max_attempts: u32,
) -> Result<T, DataError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, DataError>>,
{
    assert!(
        max_attempts > 0,
        "max_attempts must be at least 1"
    );

    let mut last_err: Option<DataError> = None;

    for attempt in 1..=max_attempts {
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) if is_retryable(&e) => {
                warn!(attempt = attempt, max_attempts = max_attempts, error = %e, "retryable error");
                last_err = Some(e);

                if attempt < max_attempts {
                    let delay_secs = 1u64 << (attempt - 1); // 1, 2, 4, 8, …
                    debug!(
                        delay_secs = delay_secs,
                        "backing off before next attempt"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                }
            }
            Err(e) => {
                // Non-retryable — propagate immediately
                return Err(e);
            }
        }
    }

    // All attempts exhausted
    Err(last_err.expect("at least one error must have been recorded"))
}

// ---------------------------------------------------------------------------
// Domain-specific convenience
// ---------------------------------------------------------------------------

/// Fetch OHLCV bars from `provider` with automatic retry on transient failures.
///
/// Convenience wrapper around [`fetch_with_retry`] that captures the provider
/// and arguments into a closure.
pub async fn fetch_bars_with_retry(
    provider: &compass_rs::data::eastmoney::EastMoneyProvider,
    symbol: &str,
    timeframe: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    max_attempts: u32,
) -> Result<Vec<Bar>, DataError> {
    let sym = symbol.to_string();
    let tf = timeframe.to_string();

    fetch_with_retry(
        || {
            let sym = sym.clone();
            let tf = tf.clone();
            async move { provider.fetch_bars(&sym, &tf, start, end).await }
        },
        max_attempts,
    )
    .await
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Build a retryable error for use in tests.
    ///
    /// Uses `DataError::RateLimited` whose construction does not depend on
    /// reqwest's private `Error::new` API.
    fn network_err(_msg: &str) -> DataError {
        DataError::RateLimited(0)
    }

    /// A call-counting mock whose `Result<T, DataError>` is driven by a
    /// user-supplied `responses` iterator.
    struct TestOp<T> {
        /// Remaining responses to return (popped from front).
        responses: Arc<Mutex<Vec<Result<T, DataError>>>>,
        call_count: Arc<Mutex<u32>>,
    }

    impl<T: Clone> TestOp<T> {
        fn new(responses: Vec<Result<T, DataError>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
                call_count: Arc::new(Mutex::new(0)),
            }
        }

        fn make_call(&self) -> impl Future<Output = Result<T, DataError>> {
            let responses = Arc::clone(&self.responses);
            let call_count = Arc::clone(&self.call_count);
            async move {
                {
                    let mut cnt = call_count.lock().expect("lock call_count");
                    *cnt += 1;
                }
                let mut guard = responses.lock().expect("lock responses");
                if guard.is_empty() {
                    Err(network_err("mock exhausted"))
                } else {
                    guard.remove(0)
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Success paths
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let op = TestOp::new(vec![Ok(42u32)]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(*op.call_count.lock().expect("lock"), 1);
    }

    #[tokio::test]
    async fn succeeds_after_two_retryable_failures() {
        let op = TestOp::new(vec![
            Err(network_err("boom1")),
            Err(DataError::RateLimited(30)),
            Ok(99u32),
        ]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        assert_eq!(result.unwrap(), 99);
        assert_eq!(*op.call_count.lock().expect("lock"), 3);
    }

    // -------------------------------------------------------------------
    // Failure paths
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn returns_last_error_when_all_attempts_fail() {
        let op = TestOp::<u32>::new(vec![
            Err(network_err("fail1")),
            Err(DataError::RateLimited(60)),
            Err(network_err("fail3")),
        ]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        assert!(result.is_err());
        // Last error should be the third one (RateLimited — used for all test mocks)
        match result.unwrap_err() {
            DataError::RateLimited(_) => {} // expected
            other => panic!("expected retryable error, got {other:?}"),
        }
        assert_eq!(*op.call_count.lock().expect("lock"), 3);
    }

    #[tokio::test]
    async fn max_attempts_of_one_makes_no_retries() {
        let op = TestOp::new(vec![
            Err(network_err("fail")),
            Ok(1u32), // would succeed if retried, but shouldn't be reached
        ]);
        let result = fetch_with_retry(|| op.make_call(), 1).await;
        assert!(result.is_err());
        assert_eq!(*op.call_count.lock().expect("lock"), 1);
    }

    // -------------------------------------------------------------------
    // Non-retryable errors propagate immediately
    // -------------------------------------------------------------------

    #[tokio::test]
    async fn no_data_error_is_not_retried() {
        let op = TestOp::new(vec![
            Err(DataError::NoData {
                symbol: "000001".into(),
            }),
            Ok(1u32), // should never be reached
        ]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        match result.unwrap_err() {
            DataError::NoData { symbol } => assert_eq!(symbol, "000001"),
            other => panic!("expected NoData, got {other:?}"),
        }
        assert_eq!(*op.call_count.lock().expect("lock"), 1);
    }

    #[tokio::test]
    async fn parse_error_is_not_retried() {
        let op = TestOp::new(vec![
            Err(DataError::Parse("bad csv".into())),
            Ok(1u32), // should never be reached
        ]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        match result.unwrap_err() {
            DataError::Parse(msg) => assert_eq!(msg, "bad csv"),
            other => panic!("expected Parse, got {other:?}"),
        }
        assert_eq!(*op.call_count.lock().expect("lock"), 1);
    }

    #[tokio::test]
    async fn database_error_is_not_retried() {
        use duckdb::Connection;
        // Create a real duckdb error by running invalid SQL
        let conn = Connection::open_in_memory().expect("in-memory db");
        let db_err = conn.execute_batch("BOGUS SQL").unwrap_err();

        let op = TestOp::new(vec![
            Err(DataError::Database(db_err)),
            Ok(1u32), // should never be reached
        ]);
        let result = fetch_with_retry(|| op.make_call(), 3).await;
        match result.unwrap_err() {
            DataError::Database(_) => {} // expected
            other => panic!("expected Database, got {other:?}"),
        }
        assert_eq!(*op.call_count.lock().expect("lock"), 1);
    }
}
