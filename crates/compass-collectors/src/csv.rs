use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::Result;

/// Write a slice of serializable records to a UTF-8 CSV file.
///
/// The field order is the struct/serde field order of the first record. If the
/// file already exists it is truncated (Python `write_csv` with `append=false`).
pub fn write_csv<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(path)?;
    for r in records {
        writer.serialize(r)?;
    }
    writer.flush()?;
    Ok(())
}

/// Write a slice of ordered string records to CSV, using the first record's
/// key order. Missing keys become empty strings; extra keys are ignored.
pub fn write_csv_ordered(path: &Path, records: &[Vec<(String, String)>]) -> Result<()> {
    let Some(first) = records.first() else {
        return Ok(());
    };
    let headers: Vec<String> = first.iter().map(|(k, _)| k.clone()).collect();
    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    writer.write_record(&headers)?;
    for record in records {
        let mut row = vec![String::new(); headers.len()];
        for (k, v) in record {
            if let Some(idx) = headers.iter().position(|h| h == k) {
                row[idx] = v.clone();
            }
        }
        writer.write_record(&row)?;
    }
    writer.flush()?;
    Ok(())
}

/// Dedupe a CSV in place, keeping the LAST row per `(SECURITY_CODE, <date_col>)`.
///
/// Mirrors `common.py::dedupe_csv`: files missing the key columns or empty
/// files are left untouched; rows without a usable key are skipped.
pub fn dedupe_csv(path: &Path, date_col: &str) -> Result<()> {
    if !path.exists() || path.metadata()?.len() == 0 {
        return Ok(());
    }
    let mut reader = csv::ReaderBuilder::new().from_path(path)?;
    let headers = reader.headers()?.clone();
    let code_idx = match headers.iter().position(|h| h == "SECURITY_CODE") {
        Some(i) => i,
        None => return Ok(()),
    };
    let date_idx = match headers.iter().position(|h| h == date_col) {
        Some(i) => i,
        None => return Ok(()),
    };

    let mut seen: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    let mut dupes = 0usize;
    for result in reader.records() {
        let row = result?;
        if row.len() <= code_idx.max(date_idx) {
            continue;
        }
        let key = (row[code_idx].to_string(), row[date_idx].to_string());
        if seen.contains_key(&key) {
            dupes += 1;
        } else {
            order.push(key.clone());
        }
        seen.insert(key, row.iter().map(|c| c.to_string()).collect());
    }

    if dupes == 0 {
        return Ok(());
    }

    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    writer.write_record(&headers)?;
    for key in order {
        if let Some(row) = seen.get(&key) {
            writer.write_record(row)?;
        }
    }
    writer.flush()?;
    Ok(())
}

const PERIOD_MAP: &[(&str, &str)] = &[
    ("FY", "-12-31"),
    ("Q3", "-09-30"),
    ("Q2", "-06-30"),
    ("Q1", "-03-31"),
];

/// Build sorted list of report-period date strings for the given years.
pub fn build_dates(years: &[i32], periods: &[&str]) -> Vec<String> {
    let mut dates = Vec::new();
    for period in periods {
        if let Some((_, suffix)) = PERIOD_MAP.iter().find(|(p, _)| p == period) {
            for year in years {
                dates.push(format!("{year}{suffix}"));
            }
        }
    }
    dates.sort();
    dates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_dates_sorts_and_maps_periods() {
        let dates = build_dates(&[2024, 2025], &["Q2", "FY"]);
        assert_eq!(
            dates,
            vec!["2024-06-30", "2024-12-31", "2025-06-30", "2025-12-31"]
        );
    }

    #[test]
    fn dedupe_keeps_last_per_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.csv");
        let mut records = Vec::new();
        records.push("SECURITY_CODE,REPORT_DATE,VALUE\n000001,2024-01-01,a\n000001,2024-01-01,b\n000002,2024-01-01,c\n");
        std::fs::write(&path, records.pop().unwrap()).unwrap();
        dedupe_csv(&path, "REPORT_DATE").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("000001,2024-01-01,b").count(), 1);
        assert_eq!(text.matches("000001,2024-01-01,a").count(), 0);
    }
}
