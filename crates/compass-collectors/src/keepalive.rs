//! Keepalive daemon for proxy_pool: seed freeproxy JSON into Redis.
//!
//! Mirrors `collectors/proxy_keepalive.py` with the JSON snapshot path (the
//! realtime `pyfreeproxy` path is intentionally reported as unsupported in
//! Rust for now; JSON source plus local snapshot fallback covers the main
//! keep-warm loop).

use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::freeproxy;

pub const DEFAULT_SNAPSHOT: &str = "/tmp/freeproxy.json";

fn write_snapshot(path: &Path, payload: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(payload)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub async fn run_json_cycle(
    json_url: &str,
    snapshot: &Path,
    redis_url: &str,
    table: &str,
    limit: usize,
) -> Result<usize> {
    let mut payload = match freeproxy::fetch_json_payload(json_url).await {
        Ok(p) => {
            if let Err(e) = write_snapshot(snapshot, &p) {
                eprintln!("[keepalive] snapshot write failed: {e}");
            }
            Some(p)
        }
        Err(e) => {
            eprintln!("[keepalive] json source failed: {e}");
            if snapshot.exists() {
                match std::fs::read_to_string(snapshot) {
                    Ok(text) => serde_json::from_str::<Value>(&text).ok(),
                    Err(e) => {
                        eprintln!("[keepalive] snapshot read failed: {e}");
                        None
                    }
                }
            } else {
                None
            }
        }
    };
    let Some(payload) = payload.take() else {
        return Ok(0);
    };
    let records = freeproxy::records_from_json_data(&payload, limit);
    if records.is_empty() {
        eprintln!("[keepalive] json source produced no usable records");
        return Ok(0);
    }
    freeproxy::write_to_redis(redis_url, table, &records)
}

pub async fn run_realtime_cycle(redis_url: &str, table: &str, limit: usize) -> Result<usize> {
    let _ = (redis_url, table, limit);
    eprintln!("[keepalive] realtime source is not yet available in Rust; skipping");
    Ok(0)
}

pub async fn run_cycle(
    json_url: &str,
    snapshot: &Path,
    redis_url: &str,
    table: &str,
    limit: usize,
) -> Result<(usize, usize)> {
    let json_written = run_json_cycle(json_url, snapshot, redis_url, table, limit).await?;
    let realtime_written = run_realtime_cycle(redis_url, table, limit).await?;
    Ok((json_written, realtime_written))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("freeproxy.json");
        let payload = serde_json::json!({"data": []});
        write_snapshot(&path, &payload).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), payload);
    }
}
