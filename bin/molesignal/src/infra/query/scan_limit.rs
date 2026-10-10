// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[querier].max_scan_rows`：单次读取最多扫描的行数。

use crate::shared::{Error, Result};

/// 候选文件与缓冲批的总行数超过上限时拒绝查询；`max_scan_rows == 0` 表示不限制。
///
/// 行数来自 FileCatalog 记录的文件行数，读取任何数据之前就能得出，超限的查询不会产生
/// 对象存储读取。
pub fn ensure_within_scan_limit(candidate_rows: u64, max_scan_rows: u64) -> Result<()> {
    if max_scan_rows > 0 && candidate_rows > max_scan_rows {
        return Err(Error::invalid(format!(
            "the query would scan {candidate_rows} rows, over the limit of {max_scan_rows} rows \
             per query; narrow the time range or add filters"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_within_scan_limit;

    #[test]
    fn rows_up_to_the_limit_are_allowed() {
        assert!(ensure_within_scan_limit(0, 10).is_ok());
        assert!(ensure_within_scan_limit(10, 10).is_ok());
    }

    #[test]
    fn rows_over_the_limit_are_a_client_error() {
        let error = ensure_within_scan_limit(11, 10).unwrap_err();
        assert_eq!(error.http_status_code(), 400);
        assert!(error.to_string().contains("11 rows"));
    }

    #[test]
    fn zero_disables_the_limit() {
        assert!(ensure_within_scan_limit(u64::MAX, 0).is_ok());
    }
}
