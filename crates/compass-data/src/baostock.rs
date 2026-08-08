use compass_core::model::AdjFactor;

/// Fetch adjustment factors from Baostock Python script using the default
/// script path (`scripts/fetch_adj_factor.py`).
///
/// Convenience wrapper around the crate-internal `fetch_adj_factors_with_script`.
#[allow(dead_code)]
pub async fn fetch_adj_factors(
    ts_code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<AdjFactor>, String> {
    fetch_adj_factors_with_script("scripts/fetch_adj_factor.py", ts_code, start_date, end_date)
        .await
}

/// Fetch adjustment factors from a Baostock-compatible Python script at
/// `script_path`.
///
/// Spawns `python3 <script_path> <ts_code> <start_date> <end_date>` as a
/// subprocess. Parses the JSON stdout into a `Vec<AdjFactor>`. On non-zero
/// exit, if stderr contains JSON with an `"error"` key, that value is used
/// as the error message; otherwise the raw stderr text is used.
///
/// # Arguments
/// * `script_path` — Path to the Python script (e.g. `"scripts/fetch_adj_factor.py"`).
/// * `ts_code` — Stock code in `XXX.SZ` / `XXX.SH` / `XXX.BJ` format.
/// * `start_date` — Start date in `YYYYMMDD` format (e.g. `"20200101"`).
/// * `end_date` — End date in `YYYYMMDD` format (e.g. `"20250722"`).
///
/// # Errors
/// Returns `Err(String)` if the subprocess fails, exits non-zero, or produces
/// invalid JSON.
pub(crate) async fn fetch_adj_factors_with_script(
    script_path: &str,
    ts_code: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<AdjFactor>, String> {
    let script = script_path.to_string();
    let ts_code = ts_code.to_string();
    let start_date = start_date.to_string();
    let end_date = end_date.to_string();

    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("python3")
            .arg(&script)
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

    /// Write a temp Python script that prints `stdout_json` to stdout and
    /// optionally writes `stderr_content` to stderr before exiting with `exit_code`.
    fn write_mock_script(
        stdout_json: &str,
        exit_code: i32,
        stderr_content: Option<&str>,
    ) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = dir.path().join("mock_adj.py");
        let stderr_block = match stderr_content {
            Some(s) => format!("print(r'''{s}''', file=sys.stderr)"),
            None => String::new(),
        };
        let script = format!(
            r#"#!/usr/bin/env python3
import sys, json
print(r'''{stdout_json}''', file=sys.stdout)
{stderr_block}
sys.exit({exit_code})
"#
        );
        std::fs::write(&script_path, script).expect("write mock script");
        (dir, script_path.to_string_lossy().to_string())
    }

    // ── Tests calling fetch_adj_factors_with_script (real function) ──────

    /// Valid JSON array on stdout → Ok(vec) with correct fields.
    #[tokio::test]
    async fn with_script_valid_json_array() {
        let json_out = json!([
            {"trade_date": "20250721", "adj_factor": 1.0},
            {"trade_date": "20250722", "adj_factor": 1.05}
        ])
        .to_string();

        let (_dir, script) = write_mock_script(&json_out, 0, None);
        let result = fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should parse valid JSON");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].trade_date, "20250721");
        assert!((result[0].adj_factor - 1.0).abs() < 0.001);
        assert_eq!(result[1].trade_date, "20250722");
        assert!((result[1].adj_factor - 1.05).abs() < 0.001);
    }

    /// Empty array on stdout → Ok(empty vec).
    #[tokio::test]
    async fn with_script_empty_array() {
        let json_out = json!([]).to_string();
        let (_dir, script) = write_mock_script(&json_out, 0, None);
        let result = fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should parse empty array");

        assert!(result.is_empty());
    }

    /// Empty stdout → Ok(empty vec).
    #[tokio::test]
    async fn with_script_empty_stdout() {
        let (_dir, script) = write_mock_script("", 0, None);
        let result = fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722")
            .await
            .expect("should handle empty stdout");

        assert!(result.is_empty());
    }

    /// Non-zero exit + stderr containing JSON with "error" key → Err with
    /// extracted message string.
    #[tokio::test]
    async fn with_script_non_zero_exit_with_json_error() {
        let error_json = json!({"error": "stock not found", "code": 404}).to_string();
        let (_dir, script) = write_mock_script(&json!([]).to_string(), 1, Some(&error_json));
        let result =
            fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722").await;

        assert!(result.is_err(), "expected error for non-zero exit");
        assert_eq!(result.unwrap_err(), "stock not found");
    }

    /// Non-zero exit + plain-text stderr (no JSON) → Err with raw stderr text.
    #[tokio::test]
    async fn with_script_non_zero_exit_plain_stderr() {
        let (_dir, script) = write_mock_script(&json!([]).to_string(), 1, Some("script crashed"));
        let result =
            fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722").await;

        assert!(result.is_err(), "expected error for non-zero exit");
        assert_eq!(result.unwrap_err(), "script crashed");
    }

    /// Invalid JSON on stdout → Err.
    #[tokio::test]
    async fn with_script_invalid_json() {
        let (_dir, script) = write_mock_script("not valid json", 0, None);
        let result =
            fetch_adj_factors_with_script(&script, "000001.SZ", "20250721", "20250722").await;

        assert!(result.is_err(), "expected JSON parse error");
        assert!(result.unwrap_err().contains("JSON parse error"));
    }

    /// Non-existent script path → Err (script missing, python3 exits non-zero).
    #[tokio::test]
    async fn with_script_missing_script_file() {
        let result = fetch_adj_factors_with_script(
            "/nonexistent/path/mock.py",
            "000001.SZ",
            "20250721",
            "20250722",
        )
        .await;

        assert!(
            result.is_err(),
            "expected error for missing script, got: {result:?}"
        );
        // The error is python3's stderr (e.g. "can't open file").
        assert!(!result.unwrap_err().is_empty());
    }

    /// The `fetch_adj_factors` convenience wrapper (dead-code-annotated,
    /// CLI-unused) delegates to fetch_adj_factors_with_script with the
    /// repo-relative script path; a missing script must surface as Err.
    #[tokio::test]
    async fn fetch_adj_factors_wrapper_propagates_script_error() {
        let result = fetch_adj_factors("000001.SZ", "20250721", "20250722").await;
        assert!(result.is_err(), "expected error, got: {result:?}");
    }
}
