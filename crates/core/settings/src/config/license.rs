// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

/// 实例 License 的启动来源策略。签名包本身只从 `MS_LICENSE_FILE` 读取，
/// 不允许内联 TOML。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LicenseSettings {
    /// 仅在数据库没有任何 License version 时，允许把 `MS_LICENSE_FILE` 导入并激活。
    #[serde(default)]
    pub bootstrap_from_environment: bool,
    /// DB active version 损坏/不可读时才使用的显式灾备开关；默认安全降级 Community。
    #[serde(default)]
    pub disaster_fallback_from_environment: bool,
}
