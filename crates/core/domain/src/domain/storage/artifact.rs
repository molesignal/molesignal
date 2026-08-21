// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Artifact：Segment 下的单个物理文件。
//!
//! Parquet 主数据、Tantivy 索引以及未来的 bloom / zone-map / 字典都以显式
//! Artifact 行登记在 Catalog；读取方根据 `artifact_type + format_version` 选择
//! reader，不允许通过对象 key 后缀推断类型，也不再从 parquet 路径推导索引路径。

use serde::{Deserialize, Serialize};

use super::{
    ids::{ArtifactId, ObjectChecksum, ObjectKey, SchemaFingerprint},
    type_id::ArtifactTypeId,
};

/// Artifact 在 Segment 内的职责。新增职责会改变查询正确性语义（例如哪些文件
/// 允许排除 Segment），所以保持封闭 enum。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    /// 权威主数据；查询正确性只依赖它。
    PrimaryData,
    /// 可选加速索引；缺失、损坏或 Pending 时必须回退扫描主数据。
    Index,
    /// 聚合统计（zone map 等）；只能用于保守裁剪。
    Statistics,
    /// 字典 / 符号表。
    Dictionary,
}

impl ArtifactRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryData => "primary_data",
            Self::Index => "index",
            Self::Statistics => "statistics",
            Self::Dictionary => "dictionary",
        }
    }
}

impl std::str::FromStr for ArtifactRole {
    type Err = crate::shared::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "primary_data" => Ok(Self::PrimaryData),
            "index" => Ok(Self::Index),
            "statistics" => Ok(Self::Statistics),
            "dictionary" => Ok(Self::Dictionary),
            other => Err(crate::shared::Error::invalid(format!(
                "unknown artifact role `{other}`"
            ))),
        }
    }
}

/// Artifact 构建状态机。
///
/// ```text
/// Pending → Ready
/// Pending → Failed
/// Failed  → Pending   （rebuild worker 重试）
/// Ready   → Tombstoned
/// Pending/Failed → Tombstoned （所属 Segment 被替换/删除）
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Pending,
    Ready,
    Failed,
    Tombstoned,
}

impl ArtifactState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Tombstoned => "tombstoned",
        }
    }

    /// 状态迁移合法性；Catalog 更新入口据此拒绝非法流转。
    pub const fn can_transition_to(self, next: ArtifactState) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Ready)
                | (Self::Pending, Self::Failed)
                | (Self::Failed, Self::Pending)
                | (Self::Pending, Self::Tombstoned)
                | (Self::Failed, Self::Tombstoned)
                | (Self::Ready, Self::Tombstoned)
        )
    }
}

impl std::str::FromStr for ArtifactState {
    type Err = crate::shared::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "tombstoned" => Ok(Self::Tombstoned),
            other => Err(crate::shared::Error::invalid(format!(
                "unknown artifact state `{other}`"
            ))),
        }
    }
}

/// 已上传对象的定位与完整性信息。对象一旦发布不可覆盖；正常读取凭 size/checksum
/// 走块缓存，不需要逐文件 HEAD。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredObject {
    pub key: ObjectKey,
    pub size_bytes: u64,
    pub checksum: ObjectChecksum,
    pub etag: Option<String>,
}

/// Segment 下的单个物理文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub role: ArtifactRole,

    pub artifact_type: ArtifactTypeId,
    pub format_version: u32,

    pub object: StoredObject,
    /// 派生 Artifact（索引、统计）指向其源主数据；用于 rebuild 与一致性校验。
    pub source_artifact_id: Option<ArtifactId>,
    /// 构建派生 Artifact 时读取的源对象校验和；源对象内容变化时必须禁用派生产物。
    #[serde(default)]
    pub source_checksum: Option<ObjectChecksum>,
    /// 构建时源数据的 schema 指纹；查询侧不一致时禁用该 Artifact。
    pub schema_fingerprint: Option<SchemaFingerprint>,

    pub state: ArtifactState,
    pub failure_reason: Option<String>,
}

impl Artifact {
    /// 该 Artifact 是否可参与查询裁剪 / 读取。
    pub fn is_ready(&self) -> bool {
        self.state == ArtifactState::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_permits_only_documented_transitions() {
        use ArtifactState::*;
        assert!(Pending.can_transition_to(Ready));
        assert!(Pending.can_transition_to(Failed));
        assert!(Failed.can_transition_to(Pending));
        assert!(Ready.can_transition_to(Tombstoned));
        assert!(Pending.can_transition_to(Tombstoned));

        assert!(!Ready.can_transition_to(Pending));
        assert!(!Ready.can_transition_to(Failed));
        assert!(!Tombstoned.can_transition_to(Pending));
        assert!(!Tombstoned.can_transition_to(Ready));
        assert!(!Failed.can_transition_to(Ready));
    }

    #[test]
    fn role_and_state_roundtrip_str() {
        for role in [
            ArtifactRole::PrimaryData,
            ArtifactRole::Index,
            ArtifactRole::Statistics,
            ArtifactRole::Dictionary,
        ] {
            assert_eq!(role.as_str().parse::<ArtifactRole>().unwrap(), role);
        }
        for state in [
            ArtifactState::Pending,
            ArtifactState::Ready,
            ArtifactState::Failed,
            ArtifactState::Tombstoned,
        ] {
            assert_eq!(state.as_str().parse::<ArtifactState>().unwrap(), state);
        }
    }
}
