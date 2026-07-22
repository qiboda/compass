use compass_core::model::AdjFactor;

/// Fetch adjustment factors from Baostock Python script.
///
/// Calls `scripts/fetch_adj_factor.py` as a subprocess, passing the stock code
/// and date range. Parses the JSON output from stdout into a `Vec<AdjFactor>`.
///
/// # Arguments
/// * `ts_code` — Stock code in `XXX.SZ` / `XXX.SH` / `XXX.BJ` format.
/// * `start_date` — Start date in `YYYYMMDD` format (e.g. `"20200101"`).
/// * `end_date` — End date in `YYYYMMDD` format (e.g. `"20250722"`).
///
/// # Errors
/// Returns `Err(String)` if the subprocess fails, exits non-zero, or produces
/// invalid JSON.
#[allow(dead_code)]
pub async fn fetch_adj_factors(
    ts_code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<AdjFactor>, String> {
    let ts_code = ts_code.to_string();
    let start_date = start_date.to_string();
    let end_date = end_date.to_string();

    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("python3")
            .arg("scripts/fetch_adj_factor.py")
            .arg(&ts_code)
            .arg(&start_date)
            .arg(&end_date)
            .output()
    })
    .await
    .map_err(|e| format!("spawn_blocking error: {e}"))?
    .map_err(|e| format!("command error: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Try to extract a meaningful error from the JSON stderr output
        let msg = serde_json::from_str::<serde_json::Value>(&stderr)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| stderr.trim().to_string());
        return Err(msg);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(&stdout).map_err(|e| format!("JSON parse error: {e}"))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: write a temporary Python script that outputs the given JSON.
    fn write_mock_script(stdout_json: &str, exit_code: i32) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = dir.path().join("mock_adj.py");
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json
print(r'''{stdout_json}''', file=sys.stdout)
sys.exit({exit_code})
"#
        );
        std::fs::write(&script_path, script).expect("write mock script");
        (dir, script_path.to_string_lossy().to_string())
    }

    /// Parse `date_str` → wait for spawned task → parse output.
    async fn run_mock(
        script_path: &str,
        ts_code: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<AdjFactor>, String> {
        let ts_code = ts_code.to_string();
        let start = start.to_string();
        let end = end.to_string();
        let script = script_path.to_string();

        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("python3")
                .arg(&script)
                .arg(&ts_code)
                .arg(&start)
                .arg(&end)
                .output()
        })
        .await
        .map_err(|e| format!("spawn_blocking error: {e}"))?
        .map_err(|e| format!("command error: {e}"))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }

        serde_json::from_str(&stdout).map_err(|e| format!("JSON parse error: {e}"))
    }

    #[tokio::test]
    async fn parses_valid_json_array() {
        let json_out = json!([
            {"trade_date": "20250721", "adj_factor": 1.0},
            {"trade_date": "20250722", "adj_factor": 1.05}
        ])
        .to_string();

        let (_dir, script) = write_mock_script(&json_out, 0);
        let result = run_mock(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should parse valid JSON");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].trade_date, "20250721");
        assert!((result[0].adj_factor - 1.0).abs() < 0.001);
        assert_eq!(result[1].trade_date, "20250722");
        assert!((result[1].adj_factor - 1.05).abs() < 0.001);
    }

    #[tokio::test]
    async fn parses_empty_array() {
        let json_out = json!([]).to_string();
        let (_dir, script) = write_mock_script(&json_out, 0);
        let result = run_mock(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should parse empty array");

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn non_zero_exit_returns_error() {
        let (_dir, script) = write_mock_script(&json!([]).to_string(), 1);
        let result = run_mock(&script, "000001.SZ", "20250721", "20250722").await;

        assert!(result.is_err(), "expected error for non-zero exit");
    }

    #[tokio::test]
    async fn empty_stdout_returns_empty_vec() {
        let (_dir, script) = write_mock_script("", 0);
        let result = run_mock(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should handle empty stdout");

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let (_dir, script) = write_mock_script("not valid json", 0);
        let result = run_mock(&script, "000001.SZ", "20250721", "20250722").await;

        assert!(result.is_err(), "expected JSON parse error");
    }
}
