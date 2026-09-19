// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use svix_ksuid::{Ksuid, KsuidLike};
use uuid::Uuid;

/// 全局排序友好的 ID（默认 KSUID）。
///
/// 内部存为 `String`，可同时承载 KSUID（默认）或 UUID 文本（hyphenated）。
/// 为了与 sea-orm 的 `Uuid` 主键互转，提供了 [`Id::from_uuid`] / [`Id::as_uuid`] /
/// [`Id::new_uuid`] 三个辅助方法。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    /// 默认生成 KSUID，27 字符、字典序近似时间序。
    pub fn new() -> Self {
        // svix-ksuid 0.10 requires a Timestamp type annotation; pass None so
        // the crate picks "now" via its default impl on SystemTime.
        Self(Ksuid::new(None::<std::time::SystemTime>, None).to_string())
    }

    /// 生成时间序友好的 UUID v7，并以 hyphenated 字符串存入。
    /// 当目标存储是 Postgres `UUID` 列时优先用这个。
    pub fn new_uuid() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// 把 [`uuid::Uuid`] 包装为 `Id`（hyphenated 字符串）。
    pub fn from_uuid(u: Uuid) -> Self {
        Self(u.to_string())
    }

    /// 把内部字符串当作 UUID 解析。
    /// 对 KSUID 字符串会返回 `None`（KSUID 长度/字符集与 UUID 不同）。
    pub fn as_uuid(&self) -> Option<Uuid> {
        Uuid::parse_str(&self.0).ok()
    }

    /// 强制按 UUID 取值，失败 panic — 仅用于"已知是 UUID"的入口（如 sea-orm 主键映射）。
    pub fn expect_uuid(&self) -> Uuid {
        self.as_uuid()
            .unwrap_or_else(|| panic!("Id {:?} is not a valid UUID", self.0))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Uuid> for Id {
    fn from(u: Uuid) -> Self {
        Self::from_uuid(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ksuid_constructor_is_27_chars() {
        let id = Id::new();
        assert_eq!(id.as_str().len(), 27);
        assert!(id.as_uuid().is_none(), "KSUID should not parse as UUID");
    }

    #[test]
    fn uuid_roundtrip() {
        let u = Uuid::now_v7();
        let id = Id::from_uuid(u);
        assert_eq!(id.as_uuid(), Some(u));
        assert_eq!(id.expect_uuid(), u);
    }

    #[test]
    fn new_uuid_parses_back() {
        let id = Id::new_uuid();
        let u = id.as_uuid().expect("UUID Id must parse");
        // v7 sets version nibble to 7
        assert_eq!(u.get_version_num(), 7);
    }

    #[test]
    fn from_trait_works() {
        let u = Uuid::now_v7();
        let id: Id = u.into();
        assert_eq!(id.as_uuid(), Some(u));
    }
}
