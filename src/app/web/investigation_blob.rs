// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Investigation blob use case helpers（web-investigation-shell）。
//!
//! 当前 use case 只有"大小校验"一项纯逻辑；存取本身是 thin wrapper over repo。

/// payload 体积上限：内容已外置到对象存储(S3)，不再受 PG 行宽约束，从 64 KiB 放宽到
/// 1 MiB（保持在 axum 默认 2 MiB body limit 之下）。前端 > 4 KiB 才走 blob。
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

pub fn ensure_within_limit(byte_len: usize) -> Result<(), String> {
    if byte_len > MAX_PAYLOAD_BYTES {
        Err(format!("payload exceeds {MAX_PAYLOAD_BYTES} bytes"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_limit_ok() {
        assert!(ensure_within_limit(MAX_PAYLOAD_BYTES).is_ok());
        assert!(ensure_within_limit(0).is_ok());
    }

    #[test]
    fn over_limit_err() {
        assert!(ensure_within_limit(MAX_PAYLOAD_BYTES + 1).is_err());
    }
}
