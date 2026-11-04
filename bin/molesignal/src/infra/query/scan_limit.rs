// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[querier].max_scan_rows`：单次读取最多扫描的行数。

use crate::shared::{Error, Result};

/// 超限错误消息的开头；[`is_over_scan_limit`] 靠它认出这类错误。
const OVER_LIMIT: &str = "the query would scan";

/// 候选文件与缓冲批的总行数超过上限时拒绝查询；`max_scan_rows == 0` 表示不限制。
///
/// 行数来自 FileCatalog 记录的文件行数，读取任何数据之前就能得出，超限的查询不会产生
/// 对象存储读取。
pub fn ensure_within_scan_limit(candidate_rows: u64, max_scan_rows: u64) -> Result<()> {
    if max_scan_rows > 0 && candidate_rows > max_scan_rows {
        return Err(Error::invalid(format!(
            "{OVER_LIMIT} {candidate_rows} rows, over the limit of {max_scan_rows} rows per \
             query; narrow the time range or add filters"
        )));
    }

    Ok(())
}

/// 查询是否因读取行数超过上限而被拒绝：调用方据此说明原因，而不是当作失败。
pub fn is_over_scan_limit(error: &Error) -> bool {
    matches!(error, Error::InvalidArgument(message) if message.starts_with(OVER_LIMIT))
}

#[cfg(test)]
mod tests {
    use super::{ensure_within_scan_limit, is_over_scan_limit};

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
        assert!(is_over_scan_limit(&error));
    }

    #[test]
    fn other_client_errors_are_not_taken_for_the_limit() {
        assert!(!is_over_scan_limit(&crate::shared::Error::invalid(
            "unsupported trace filter field: x"
        )));
        assert!(!is_over_scan_limit(&crate::shared::Error::internal(
            "the query would scan nothing"
        )));
    }

    #[test]
    fn zero_disables_the_limit() {
        assert!(ensure_within_scan_limit(u64::MAX, 0).is_ok());
    }
}
