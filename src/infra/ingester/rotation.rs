// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Parquet rotation 的有界原因集合。

mod adaptive;

pub use adaptive::AdaptiveRotation;

/// Flush 被触发的原因。该值可安全用作 metric label。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationReason {
    Size,
    Age,
    Retry,
    Forced,
}

impl RotationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Age => "age",
            Self::Retry => "retry",
            Self::Forced => "forced",
        }
    }
}
