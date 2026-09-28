// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! StorageLayout：对象存储 key 的唯一生成入口。
//!
//! 路径只含系统生成的稳定 ID，不含用户输入的 stream name，信号类型使用经过校验的类型标识；后缀只用于人工诊断，读取判定一律走 Catalog 的
//! `artifact_type + format_version`。业务代码禁止自行 `format!` 拼接对象路径。
//!
//! ```text
//! artifacts/v1/{org}/{type}/{dataset}/p-{partition_start_secs}-{shard:02}/{segment}/{artifact}{suffix}
//! manifests/v1/{org}/{type}/{dataset}/p-{partition_start_secs}-{shard:02}/{generation}.parquet
//! ```

use crate::{
    domain::storage::{
        ArtifactId, ArtifactTypeId, ObjectKey, Partition, PhysicalDatasetId, SegmentId,
        StreamTypeId, type_id,
    },
    shared::ids::Id,
};

/// `v1` 布局。按组织、信号类型和数据集组织对象。
pub struct StorageLayout;

impl StorageLayout {
    /// Artifact 数据对象 key。
    pub fn artifact_key(
        organization_id: &Id,
        stream_type: StreamTypeId,
        dataset_id: &PhysicalDatasetId,
        partition: &Partition,
        segment_id: &SegmentId,
        artifact_id: &ArtifactId,
        artifact_type: &ArtifactTypeId,
    ) -> ObjectKey {
        ObjectKey::from_string(format!(
            "artifacts/v1/{}/{}/{}/{}/{}/{}{}",
            organization_id.as_str(),
            stream_type.external_name(),
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
        stream_type: StreamTypeId,
        dataset_id: &PhysicalDatasetId,
        partition: &Partition,
        generation: u64,
    ) -> ObjectKey {
        ObjectKey::from_string(format!(
            "manifests/v1/{}/{}/{}/{}/{generation}.parquet",
            organization_id.as_str(),
            stream_type.external_name(),
            dataset_id.as_str(),
            Self::partition_id(partition),
        ))
    }

    /// Reconciler 做 orphan 扫描的合法前缀。object store 里还有 Catalog 之外的
    /// 合法对象（原始归档、会话回放等），孤儿判定只允许在这两个前缀内进行。
    pub fn catalog_scan_prefixes() -> [&'static str; 2] {
        ["artifacts/v1/", "manifests/v1/"]
    }

    pub fn catalog_scan_prefixes_for(organization_id: &Id) -> [String; 2] {
        [
            format!("artifacts/v1/{}/", organization_id.as_str()),
            format!("manifests/v1/{}/", organization_id.as_str()),
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
            crate::domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            &SegmentId::from_string("seg1"),
            &ArtifactId::from_string("art1"),
            &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
        );
        assert_eq!(
            key.as_str(),
            "artifacts/v1/org1/logs/ds1/p-1755302400-00/seg1/art1.parquet"
        );
    }

    #[test]
    fn tantivy_and_unknown_suffixes() {
        let ttv = StorageLayout::artifact_key(
            &Id::from_string("o"),
            crate::domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("d"),
            &partition(),
            &SegmentId::from_string("s"),
            &ArtifactId::from_string("a"),
            &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY),
        );
        assert!(ttv.as_str().ends_with("/a.ttv"));
        let custom = StorageLayout::artifact_key(
            &Id::from_string("o"),
            crate::domain::storage::StreamTypeId::LOGS,
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
            crate::domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            7,
        );
        assert_eq!(
            key.as_str(),
            "manifests/v1/org1/logs/ds1/p-1755302400-00/7.parquet"
        );
    }

    #[test]
    fn negative_partition_start_keeps_sign() {
        let key = StorageLayout::manifest_key(
            &Id::from_string("o"),
            crate::domain::storage::StreamTypeId::LOGS,
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
    #[test]
    fn catalog_scan_excludes_specialized_blobs() {
        let prefixes = StorageLayout::catalog_scan_prefixes();
        assert_eq!(prefixes, ["artifacts/v1/", "manifests/v1/"]);
        for key in [
            "blobs/v1/org/profiles/api/cpu/20260927/id.pprof.zst",
            "blobs/v1/org/rum/app/session/0000000001-hash.ndjson.zst",
        ] {
            assert!(prefixes.iter().all(|prefix| !key.starts_with(prefix)));
        }
    }

    #[test]
    fn signal_paths_remain_within_the_organization_scan_prefix() {
        let org = Id::from_string("org");
        let dataset = PhysicalDatasetId::from_string("dataset");
        for signal in [
            StreamTypeId::LOGS,
            StreamTypeId::METRICS,
            StreamTypeId::TRACES,
            StreamTypeId::PROFILES,
            StreamTypeId::new("vendor.events").unwrap(),
        ] {
            let artifact = StorageLayout::artifact_key(
                &org,
                signal,
                &dataset,
                &partition(),
                &SegmentId::from_string("segment"),
                &ArtifactId::from_string("data"),
                &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
            );
            let manifest = StorageLayout::manifest_key(&org, signal, &dataset, &partition(), 1);
            assert!(artifact.as_str().starts_with(&format!(
                "artifacts/v1/org/{}/dataset/",
                signal.external_name()
            )));
            assert!(manifest.as_str().starts_with(&format!(
                "manifests/v1/org/{}/dataset/",
                signal.external_name()
            )));
            let prefixes = StorageLayout::catalog_scan_prefixes_for(&org);
            assert!(artifact.as_str().starts_with(&prefixes[0]));
            assert!(manifest.as_str().starts_with(&prefixes[1]));
            assert!(!artifact.as_str().starts_with(
                &StorageLayout::catalog_scan_prefixes_for(&Id::from_string("other"))[0]
            ));
        }
    }
}
