use chrono::NaiveDate;

/// Default maximum days per chunk matching EastMoney's `lmt` parameter limit.
#[allow(dead_code)]
pub const DEFAULT_MAX_DAYS: u32 = 2000;

/// Split a date range (inclusive) into chunks of at most `max_days` calendar days.
///
/// Input dates are `"YYYYMMDD"` strings matching EastMoney's `beg`/`end` format.
/// Returns chronologically ordered chunks (oldest first).
/// If `start > end`, returns an empty `Vec`.
///
/// The algorithm works backwards from `end`, creating chunks of up to `max_days`
/// days each, then reverses for chronological order.
///
/// # Panics
/// Panics if either date string is not valid `"YYYYMMDD"`.
pub fn split_date_range(start: &str, end: &str, max_days: u32) -> Vec<(String, String)> {
    let start =
        NaiveDate::parse_from_str(start, "%Y%m%d").expect("start must be in YYYYMMDD format");
    let end = NaiveDate::parse_from_str(end, "%Y%m%d").expect("end must be in YYYYMMDD format");

    if start > end {
        return vec![];
    }

    let max_days = max_days as i64;
    let mut chunks: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    let mut chunk_end = end;

    loop {
        let chunk_start = (chunk_end - chrono::Duration::days(max_days - 1)).max(start);
        chunks.push((chunk_start, chunk_end));

        if chunk_start == start {
            break;
        }
        chunk_end = chunk_start - chrono::Duration::days(1);
    }

    // Reverse so oldest chunk is first (chronological order)
    chunks.reverse();

    chunks
        .into_iter()
        .map(|(s, e)| {
            (
                s.format("%Y%m%d").to_string(),
                e.format("%Y%m%d").to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_start_end_returns_one_chunk() {
        let chunks = split_date_range("20240101", "20240101", 2000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], ("20240101".to_string(), "20240101".to_string()));
    }

    #[test]
    fn zero_day_range_returns_one_chunk() {
        let chunks = split_date_range("20240101", "20240101", 2000);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn start_after_end_returns_empty() {
        let chunks = split_date_range("20240102", "20240101", 2000);
        assert!(chunks.is_empty());
    }

    #[test]
    fn range_exactly_max_days_is_one_chunk() {
        let start_d = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let end_d = start_d + chrono::Duration::days(1999); // 2000 days inclusive
        let start = start_d.format("%Y%m%d").to_string();
        let end = end_d.format("%Y%m%d").to_string();

        let chunks = split_date_range(&start, &end, 2000);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, start);
        assert_eq!(chunks[0].1, end);
    }

    #[test]
    fn three_thousand_day_range_splits_into_two_chunks() {
        let start_d = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap();
        let end_d = start_d + chrono::Duration::days(2999); // 3000 days inclusive
        let start = start_d.format("%Y%m%d").to_string();
        let end = end_d.format("%Y%m%d").to_string();

        let chunks = split_date_range(&start, &end, 2000);

        assert_eq!(chunks.len(), 2, "3000-day range should split into 2 chunks");

        // First chunk starts at start, last chunk ends at end
        assert_eq!(chunks[0].0, start);
        assert_eq!(chunks[1].1, end);

        // Chunks are contiguous
        let first_end = NaiveDate::parse_from_str(&chunks[0].1, "%Y%m%d").unwrap();
        let second_start = NaiveDate::parse_from_str(&chunks[1].0, "%Y%m%d").unwrap();
        assert_eq!(
            first_end + chrono::Duration::days(1),
            second_start,
            "chunks should be contiguous"
        );

        // Neither chunk exceeds max_days
        for (i, (s_str, e_str)) in chunks.iter().enumerate() {
            let s = NaiveDate::parse_from_str(s_str, "%Y%m%d").unwrap();
            let e = NaiveDate::parse_from_str(e_str, "%Y%m%d").unwrap();
            let days = (e - s).num_days() + 1; // inclusive
            assert!(days <= 2000, "chunk {i} has {days} days, exceeds 2000");
        }
    }

    #[test]
    fn five_thousand_day_range_splits_into_three_chunks() {
        let start_d = NaiveDate::from_ymd_opt(2010, 1, 1).unwrap();
        let end_d = start_d + chrono::Duration::days(4999); // 5000 days inclusive
        let start = start_d.format("%Y%m%d").to_string();
        let end = end_d.format("%Y%m%d").to_string();

        let chunks = split_date_range(&start, &end, 2000);

        assert_eq!(chunks.len(), 3, "5000-day range should split into 3 chunks");

        // First chunk starts at start
        assert_eq!(chunks[0].0, start);
        // Last chunk ends at end
        assert_eq!(chunks[2].1, end);

        // Chunks are contiguous and none exceed max_days
        for (i, (s_str, e_str)) in chunks.iter().enumerate() {
            let s = NaiveDate::parse_from_str(s_str, "%Y%m%d").unwrap();
            let e = NaiveDate::parse_from_str(e_str, "%Y%m%d").unwrap();
            let days = (e - s).num_days() + 1;
            assert!(days <= 2000, "chunk {i} has {days} days, exceeds 2000");
        }

        // Check contiguity
        for i in 0..chunks.len() - 1 {
            let e = NaiveDate::parse_from_str(&chunks[i].1, "%Y%m%d").unwrap();
            let s = NaiveDate::parse_from_str(&chunks[i + 1].0, "%Y%m%d").unwrap();
            assert_eq!(
                e + chrono::Duration::days(1),
                s,
                "chunks {} and {} should be contiguous",
                i,
                i + 1
            );
        }
    }

    #[test]
    fn small_max_days_creates_many_chunks() {
        let chunks = split_date_range("20240101", "20240131", 10);
        // 31-day range with max 10 days per chunk → 4 chunks
        assert_eq!(chunks.len(), 4);

        // All chunks ≤ 10 days
        for (s_str, e_str) in &chunks {
            let s = NaiveDate::parse_from_str(s_str, "%Y%m%d").unwrap();
            let e = NaiveDate::parse_from_str(e_str, "%Y%m%d").unwrap();
            assert!((e - s).num_days() + 1 <= 10);
        }

        // Verify contiguity and total coverage
        let total_days: i64 = chunks
            .iter()
            .map(|(s_str, e_str)| {
                let s = NaiveDate::parse_from_str(s_str, "%Y%m%d").unwrap();
                let e = NaiveDate::parse_from_str(e_str, "%Y%m%d").unwrap();
                (e - s).num_days() + 1
            })
            .sum();
        assert_eq!(total_days, 31);
    }

    #[test]
    fn range_less_than_max_days_is_one_chunk() {
        let chunks = split_date_range("20240101", "20240115", 2000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], ("20240101".to_string(), "20240115".to_string()));
    }

    #[test]
    fn chronological_order() {
        // With max_days=10, verify oldest chunks come first
        let chunks = split_date_range("20240101", "20240131", 10);
        for i in 1..chunks.len() {
            assert!(
                chunks[i - 1].0 < chunks[i].0,
                "chunks should be in chronological order"
            );
        }
    }

    #[test]
    fn single_day_chunk_respects_max_days_one() {
        let chunks = split_date_range("20240101", "20240105", 1);
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[0], ("20240101".to_string(), "20240101".to_string()));
        assert_eq!(chunks[4], ("20240105".to_string(), "20240105".to_string()));
    }
}
