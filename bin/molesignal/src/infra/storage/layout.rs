// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! StorageLayout：对象存储 key 的唯一生成入口。
//!
//! 路径只含系统生成的稳定 ID，不含用户输入的 stream name，也不依赖 signal
//! 类型；后缀只用于人工诊断，读取判定一律走 Catalog 的
//! `artifact_type + format_version`。业务代码禁止自行 `format!` 拼接对象路径。
//!
//! ```text
//! v1/artifacts/{org}/{dataset}/p-{partition_start_secs}-{shard:02}/{segment}/{artifact}{suffix}
//! v1/manifests/{org}/{dataset}/p-{partition_start_secs}-{shard:02}/{generation}.parquet
//! ```

use crate::{
    domain::storage::{
        ArtifactId, ArtifactTypeId, ObjectKey, Partition, PhysicalDatasetId, SegmentId, type_id,
    },
    shared::ids::Id,
};

/// `v1` 布局。升级布局时新增版本前缀，绝不复用旧前缀。
pub struct StorageLayout;

impl StorageLayout {
    /// Artifact 数据对象 key。
    pub fn artifact_key(
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        partition: &Partition,
        segment_id: &SegmentId,
        artifact_id: &ArtifactId,
        artifact_type: &ArtifactTypeId,
    ) -> ObjectKey {
        ObjectKey::from_string(format!(
            "v1/artifacts/{}/{}/{}/{}/{}{}",
            organization_id.as_str(),
            dataset_id.as_str(),
            Self::partition_id(partition),
            segment_id.as_str(),
            artifact_id.as_str(),
            Self::diagnostic_suffix(artifact_type),
        ))
    }

    /// 封存分区 Manifest 对象 key；generation 单调递增，指针切换后旧代延迟 GC。
    pub fn manifest_key(
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        partition: &Partition,
        generation: u64,
    ) -> ObjectKey {
        ObjectKey::from_string(format!(
            "v1/manifests/{}/{}/{}/{generation}.parquet",
            organization_id.as_str(),
            dataset_id.as_str(),
            Self::partition_id(partition),
        ))
    }

    /// Reconciler 做 orphan 扫描的合法前缀。object store 里还有 Catalog 之外的
    /// 合法对象（原始归档、会话回放等），孤儿判定只允许在这两个前缀内进行。
    pub fn catalog_scan_prefixes() -> [&'static str; 2] {
        ["v1/artifacts/", "v1/manifests/"]
    }

    pub fn catalog_scan_prefixes_for(organization_id: &Id) -> [String; 2] {
        [
            format!("v1/artifacts/{}/", organization_id.as_str()),
            format!("v1/manifests/{}/", organization_id.as_str()),
        ]
    }

    /// `p-{partition_start_secs}-{shard:02}`；秒粒度足够表达小时/天分区桶，
    /// 负数时间戳（epoch 前）保留符号。
    fn partition_id(partition: &Partition) -> String {
        format!(
            "p-{}-{:02}",
            partition.start_micros.div_euclid(1_000_000),
            partition.shard
        )
    }

    /// 诊断后缀。不参与读取判断；未知类型不加后缀。
    fn diagnostic_suffix(artifact_type: &ArtifactTypeId) -> &'static str {
        match artifact_type.as_str() {
            type_id::builtin::ARTIFACT_PARQUET => ".parquet",
            type_id::builtin::ARTIFACT_TANTIVY => ".ttv",
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partition() -> Partition {
        Partition {
            start_micros: 1_755_302_400_000_000,
            end_micros: 1_755_306_000_000_000,
            shard: 0,
        }
    }

    #[test]
    fn artifact_key_contains_only_stable_ids() {
        let key = StorageLayout::artifact_key(
            &Id::from_string("org1"),
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            &SegmentId::from_string("seg1"),
            &ArtifactId::from_string("art1"),
            &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
        );
        assert_eq!(
            key.as_str(),
            "v1/artifacts/org1/ds1/p-1755302400-00/seg1/art1.parquet"
        );
    }

    #[test]
    fn tantivy_and_unknown_suffixes() {
        let ttv = StorageLayout::artifact_key(
            &Id::from_string("o"),
            &PhysicalDatasetId::from_string("d"),
            &partition(),
            &SegmentId::from_string("s"),
            &ArtifactId::from_string("a"),
            &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY),
        );
        assert!(ttv.as_str().ends_with("/a.ttv"));
        let custom = StorageLayout::artifact_key(
            &Id::from_string("o"),
            &PhysicalDatasetId::from_string("d"),
            &partition(),
            &SegmentId::from_string("s"),
            &ArtifactId::from_string("a"),
            &ArtifactTypeId::new("vendor.bloom").unwrap(),
        );
        assert!(custom.as_str().ends_with("/a"));
    }

    #[test]
    fn manifest_key_is_generation_scoped() {
        let key = StorageLayout::manifest_key(
            &Id::from_string("org1"),
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            7,
        );
        assert_eq!(
            key.as_str(),
            "v1/manifests/org1/ds1/p-1755302400-00/7.parquet"
        );
    }

    #[test]
    fn negative_partition_start_keeps_sign() {
        let key = StorageLayout::manifest_key(
            &Id::from_string("o"),
            &PhysicalDatasetId::from_string("d"),
            &Partition {
                start_micros: -3_600_000_000,
                end_micros: 0,
                shard: 3,
            },
            1,
        );
        assert!(key.as_str().contains("/p--3600-03/"));
    }
}
