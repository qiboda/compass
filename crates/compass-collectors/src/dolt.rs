use std::path::Path;

use crate::config::{dolt_dir, investment_data_dir};
use crate::error::{CollectError, Result};

pub const NAME_EN_MAPPING_TMP: &str = "_tmp_name_en";

async fn run_dolt(args: &[&str]) -> Result<std::process::Output> {
    let output = tokio::process::Command::new("dolt")
        .args(args)
        .output()
        .await?;
    Ok(output)
}

fn ensure_success(output: &std::process::Output, what: &str) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(CollectError::Dolt {
            stderr: format!("{what}: {stderr}"),
        })
    }
}

/// Run a SQL statement against the compass_data Dolt repo.
pub async fn dolt_sql(sql: &str) -> Result<std::process::Output> {
    let dir = dolt_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 dolt path".into()))?;
    let output = run_dolt(&["--data-dir", dir_str, "sql", "-q", sql]).await?;
    ensure_success(&output, "dolt_sql")?;
    Ok(output)
}

/// Run a CSV-mode query against the compass_data Dolt repo.
pub async fn dolt_sql_csv(sql: &str) -> Result<String> {
    let dir = dolt_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 dolt path".into()))?;
    let output = run_dolt(&["--data-dir", dir_str, "sql", "-r", "csv", "-q", sql]).await?;
    ensure_success(&output, "dolt_sql_csv")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a SQL statement against the investment_data Dolt repo.
pub async fn dolt_sql_investment(sql: &str) -> Result<std::process::Output> {
    let dir = investment_data_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 investment path".into()))?;
    let output = run_dolt(&["--data-dir", dir_str, "sql", "-r", "csv", "-q", sql]).await?;
    ensure_success(&output, "dolt_sql_investment")?;
    Ok(output)
}

/// Import a CSV into a Dolt table. With `create_sql`, the table is created
/// first with an explicit wide schema and the CSV is imported with `-u`.
pub async fn dolt_table_import(
    table_name: &str,
    csv_path: &Path,
    create_sql: Option<&str>,
) -> Result<()> {
    let dir = dolt_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 dolt path".into()))?;
    let csv_abs = csv_path
        .canonicalize()
        .unwrap_or_else(|_| csv_path.to_path_buf());
    let csv_str = csv_abs
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 csv path".into()))?;

    if let Some(create) = create_sql {
        let output = run_dolt(&["--data-dir", dir_str, "sql", "-q", create]).await?;
        ensure_success(&output, "dolt create")?;
    }

    let mode = if create_sql.is_some() { "-u" } else { "-c" };
    let output = run_dolt(&[
        "--data-dir",
        dir_str,
        "table",
        "import",
        mode,
        table_name,
        "--continue",
        csv_str,
    ])
    .await?;
    ensure_success(&output, "dolt table import")?;
    Ok(())
}

async fn last_csv_cell(output: &str) -> String {
    output
        .trim()
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Read the last report date from `data_updates` for a table.
pub async fn last_report_date(dolt_table: &str) -> Result<Option<String>> {
    let dir = dolt_dir();
    if !dir.join(".dolt").exists() {
        return Ok(None);
    }
    let out = dolt_sql_csv(&format!(
        "SELECT last_report_date FROM data_updates WHERE table_name='{dolt_table}'"
    ))
    .await?;
    let last = last_csv_cell(&out).await;
    if last.is_empty() || last == "NULL" {
        Ok(None)
    } else {
        Ok(Some(last))
    }
}

/// Insert/advance a data_updates anchor row (auto-heal path).
pub async fn set_last_report_date(table: &str, report_date: &str) -> Result<()> {
    let query = format!(
        "INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) \
         VALUES ('{table}', CURDATE(), 'auto-heal', 0, '{report_date}') \
         ON DUPLICATE KEY UPDATE last_updated = CURDATE(), source = 'auto-heal', \
         last_report_date = IF(COALESCE(last_report_date, '0000-00-00') < VALUES(last_report_date), \
         VALUES(last_report_date), last_report_date)"
    );
    dolt_sql(&query).await?;
    Ok(())
}

/// Atomically replace (or merge) a Dolt table from a CSV, then update data_updates.
///
/// Simplified port of `common.py::import_replace_table`. Returns the final table
/// row count. On any failure the previous table is preserved.
#[allow(clippy::too_many_arguments)]
pub async fn import_replace_table(
    csv_path: &Path,
    tmp_name: &str,
    ddl: &str,
    insert_sql: &str,
    dolt_table: &str,
    source_label: &str,
    last_report_expr: &str,
    create_sql: Option<&str>,
    merge: bool,
) -> Result<u64> {
    if !csv_path.exists() {
        return Ok(0);
    }

    dolt_table_import(tmp_name, csv_path, create_sql).await?;

    let before_total = if merge && table_exists(dolt_table).await? {
        let count = dolt_sql_csv(&format!("SELECT COUNT(*) FROM {dolt_table}")).await?;
        last_csv_cell(&count).await.parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    if merge {
        let _ = dolt_sql(ddl).await;
        let result = dolt_sql(insert_sql).await;
        match result {
            Ok(_) => {}
            Err(e) => {
                let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {tmp_name}")).await;
                return Err(e);
            }
        }
        let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {tmp_name}")).await;
    } else {
        let old_name = format!("{tmp_name}_old");
        let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {old_name}")).await;
        if table_exists(dolt_table).await? {
            let _ = dolt_sql(&format!("RENAME TABLE {dolt_table} TO {old_name}")).await;
        }

        let created = dolt_sql(ddl).await.is_ok();
        let result = if created {
            dolt_sql(insert_sql).await
        } else {
            Err(CollectError::InvalidInput("DDL failed".into()))
        };
        if result.is_err() {
            if created {
                let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {dolt_table}")).await;
            }
            if table_exists(&old_name).await? {
                let _ = dolt_sql(&format!("RENAME TABLE {old_name} TO {dolt_table}")).await;
            }
            let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {tmp_name}")).await;
            return match result {
                Err(e) => Err(e),
                Ok(_) => unreachable!(),
            };
        }
        let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {tmp_name}")).await;
        let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {old_name}")).await;
    }

    let count = dolt_sql_csv(&format!("SELECT COUNT(*) FROM {dolt_table}")).await?;
    let total = last_csv_cell(&count).await.parse::<u64>().unwrap_or(0);
    let last_val = dolt_sql_csv(&format!("SELECT {last_report_expr} FROM {dolt_table}")).await?;
    let last_val = last_csv_cell(&last_val).await;
    let last_val = if last_val.is_empty() || last_val == "NULL" {
        "NULL".to_string()
    } else {
        format!("'{last_val}'")
    };

    dolt_sql(&format!(
        "INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) \
         VALUES ('{dolt_table}', CURDATE(), '{source_label}', {total}, {last_val}) \
         ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count={total}, \
         last_report_date=IF(COALESCE(last_report_date, '0000-00-00') < VALUES(last_report_date), \
         VALUES(last_report_date), last_report_date)"
    ))
    .await?;

    tracing::info!(
        table = dolt_table,
        total,
        inserted = total.saturating_sub(before_total),
        "import_replace_table done"
    );
    Ok(total)
}

async fn table_exists(table_name: &str) -> Result<bool> {
    let out = dolt_sql_csv(&format!(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table_name}'"
    ))
    .await?;
    Ok(last_csv_cell(&out).await == "1")
}

/// Stage and drop the name_en mapping CSV (epic #266 behaviour).
pub async fn load_name_en_mapping() -> Result<bool> {
    let path = crate::config::name_en_mapping_path();
    if !path.exists() {
        return Ok(false);
    }
    let _ = dolt_sql(&format!("DROP TABLE IF EXISTS {NAME_EN_MAPPING_TMP}")).await;
    dolt_table_import(
        NAME_EN_MAPPING_TMP,
        &path,
        Some("CREATE TABLE _tmp_name_en (section VARCHAR(20), `key` VARCHAR(100), value VARCHAR(100))"),
    ).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn last_report_date_missing_repo_returns_none() {
        let _guard = crate::config::ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("COMPASS_DATA_DIR", dir.path());
        }
        assert_eq!(last_report_date("foo").await.unwrap(), None);
        unsafe {
            std::env::remove_var("COMPASS_DATA_DIR");
        }
    }
}
