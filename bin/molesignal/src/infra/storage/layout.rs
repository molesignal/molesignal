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
//! search_jobs/v1/{org}/{job}-{attempt}.ndjson
//! wal/v1/{org}/{dataset}/{writer_epoch:020}/{sequence:020}.wal
//! ```
//!
//! Search Job 的结果对象不进 Catalog：任务行上记着自己的 key，读取、重试和清理都按那个 key
//! 进行，孤儿扫描与按组织限定的只读视图都不覆盖 `search_jobs/`。

use domain::storage::{
    ArtifactId, ArtifactTypeId, ObjectKey, Partition, PhysicalDatasetId, SegmentId, StreamTypeId,
    type_id,
};
use kernel::ids::Id;

/// `v1` 布局。按组织、信号类型和数据集组织对象。
pub struct StorageLayout;

impl StorageLayout {
    /// Artifact 数据对象的 key 前缀。
    pub const ARTIFACT_PREFIX: &'static str = "artifacts/v1/";

    /// 封存分区 Manifest 对象的 key 前缀。
    pub const MANIFEST_PREFIX: &'static str = "manifests/v1/";

    /// Search Job 结果对象的 key 前缀。
    pub const SEARCH_JOB_PREFIX: &'static str = "search_jobs/v1/";

    /// The prefix scheduled reports are exported under, one directory per organization.
    pub const REPORT_PREFIX: &'static str = "reports/";

    /// The prefix of WAL objects: data acknowledged to clients and not yet flushed. They are not
    /// in the catalog; `wal_streams` names them, and the orphan scan never looks here.
    pub const WAL_PREFIX: &'static str = "wal/v1/";

    /// Where one writer epoch keeps the WAL of one dataset. Zero-padded numbers list in order.
    pub fn wal_stream_prefix(
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: u64,
    ) -> String {
        format!(
            "{}{}/{}/{writer_epoch:020}/",
            Self::WAL_PREFIX,
            organization_id.as_str(),
            dataset_id.as_str()
        )
    }

    /// The WAL object holding sequence `sequence` of a writer epoch's dataset WAL.
    pub fn wal_object_key(
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: u64,
        sequence: u64,
    ) -> ObjectKey {
        ObjectKey::from_string(format!(
            "{}{sequence:020}.wal",
            Self::wal_stream_prefix(organization_id, dataset_id, writer_epoch)
        ))
    }

    /// The sequence of a WAL object key, or `None` for another key.
    pub fn wal_object_sequence(key: &str) -> Option<u64> {
        key.rsplit('/').next()?.strip_suffix(".wal")?.parse().ok()
    }

    /// `key` 是否是 Manifest 对象。Manifest 靠单次条件写实现版本协议。
    pub fn is_manifest_key(key: &str) -> bool {
        key.starts_with(Self::MANIFEST_PREFIX)
    }

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

    /// 封存分区 Manifest 对象 key；generation 单调递增，指针切换后旧代延迟 GC。同一时刻开始的
    /// 分区可以长短不同（小数据流按天合并后，一天落在从零点开始的一个分区里），key 里带上分区
    /// 的秒数。
    pub fn manifest_key(
        organization_id: &Id,
        stream_type: StreamTypeId,
        dataset_id: &PhysicalDatasetId,
        partition: &Partition,
        generation: u64,
    ) -> ObjectKey {
        let seconds = partition
            .end_micros
            .saturating_sub(partition.start_micros)
            .div_euclid(1_000_000);

        ObjectKey::from_string(format!(
            "{}{}/{}/{}/{}-{seconds}s/{generation}.parquet",
            Self::MANIFEST_PREFIX,
            organization_id.as_str(),
            stream_type.external_name(),
            dataset_id.as_str(),
            Self::partition_id(partition),
        ))
    }

    /// Search Job 结果对象 key。`attempt` 随重试递增，上一次执行迟到的结果落在另一个 key 上，
    /// 不会盖掉这一次的。
    pub fn search_job_result_key(organization_id: &Id, job_id: &Id, attempt: i32) -> ObjectKey {
        ObjectKey::from_string(format!(
            "{}{}/{}-{attempt}.ndjson",
            Self::SEARCH_JOB_PREFIX,
            organization_id.as_str(),
            job_id.as_str(),
        ))
    }

    /// Where an `s3` report recipient writes `file_name`: under the organization's own prefix,
    /// in the path the target names after `s3://`. Empty, `.` and `..` segments are dropped, so
    /// no target reaches outside that prefix.
    pub fn report_export_key(organization_id: &Id, target: &str, file_name: &str) -> ObjectKey {
        let path = match target.get(..5) {
            Some(scheme) if scheme.eq_ignore_ascii_case("s3://") => &target[5..],
            _ => target,
        };

        let path = path
            .split('/')
            .filter(|segment| !matches!(*segment, "" | "." | ".."))
            .chain(std::iter::once(file_name))
            .collect::<Vec<_>>()
            .join("/");

        ObjectKey::from_string(format!(
            "{}{}/{path}",
            Self::REPORT_PREFIX,
            organization_id.as_str()
        ))
    }

    /// Reconciler 做 orphan 扫描的合法前缀。object store 里还有 Catalog 之外的
    /// 合法对象（原始归档、会话回放、Search Job 结果等），孤儿判定只允许在这两个前缀内进行。
    pub fn catalog_scan_prefixes() -> [&'static str; 2] {
        [Self::ARTIFACT_PREFIX, Self::MANIFEST_PREFIX]
    }

    pub fn catalog_scan_prefixes_for(organization_id: &Id) -> [String; 2] {
        [
            format!("artifacts/v1/{}/", organization_id.as_str()),
            format!("manifests/v1/{}/", organization_id.as_str()),
        ]
    }

    /// Every prefix an organization's objects are written under, so that a deleted
    /// organization's purge can remove them. Keys made outside this file:
    ///
    /// - `blobs/v1/{org}/`: RUM session replays (`infra::rum::replay`) and profile archives
    ///   (`profiles::archive_object_key`).
    /// - `agent_skills/v1/{org}/`: skill packages (`app::skills`).
    /// - `agent/chat/v1/{org}/`: archived chat transcripts.
    /// - `investigations/{org}/`, `debug-artifacts/{org}/`, `resource-shares/{org}/` and
    ///   `status-page-logos/{org}/`: investigation blobs, source maps and symbols, share
    ///   snapshots, status page logos.
    /// - `{org}/synthetics/`: synthetic check artifacts (`app::synthetics::artifact_object_key`).
    ///
    /// A new kind of object keyed by organization needs its prefix here.
    pub fn organization_prefixes(organization_id: &Id) -> Vec<String> {
        let org = organization_id.as_str();

        vec![
            format!("{}{org}/", Self::ARTIFACT_PREFIX),
            format!("{}{org}/", Self::MANIFEST_PREFIX),
            format!("{}{org}/", Self::SEARCH_JOB_PREFIX),
            format!("{}{org}/", Self::REPORT_PREFIX),
            format!("{}{org}/", Self::WAL_PREFIX),
            format!("blobs/v1/{org}/"),
            format!("agent_skills/v1/{org}/"),
            format!("agent/chat/v1/{org}/"),
            format!("investigations/{org}/"),
            format!("debug-artifacts/{org}/"),
            format!("resource-shares/{org}/"),
            format!("status-page-logos/{org}/"),
            format!("{org}/synthetics/"),
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
            domain::storage::StreamTypeId::LOGS,
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
            domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("d"),
            &partition(),
            &SegmentId::from_string("s"),
            &ArtifactId::from_string("a"),
            &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_TANTIVY),
        );
        assert!(ttv.as_str().ends_with("/a.ttv"));
        let custom = StorageLayout::artifact_key(
            &Id::from_string("o"),
            domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("d"),
            &partition(),
            &SegmentId::from_string("s"),
            &ArtifactId::from_string("a"),
            &ArtifactTypeId::new("vendor.bloom").unwrap(),
        );
        assert!(custom.as_str().ends_with("/a"));
    }

    #[test]
    fn manifest_keys_carry_the_manifest_prefix_and_artifact_keys_do_not() {
        let org = Id::from_string("org1");
        let manifest = StorageLayout::manifest_key(
            &org,
            StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            3,
        );
        assert!(StorageLayout::is_manifest_key(manifest.as_str()));
        assert!(!StorageLayout::is_manifest_key(
            "artifacts/v1/org1/logs/ds1/p-0-00/seg/a.parquet"
        ));
        assert!(!StorageLayout::is_manifest_key("tenant/other.parquet"));
    }

    #[test]
    fn manifest_key_is_generation_scoped() {
        let key = StorageLayout::manifest_key(
            &Id::from_string("org1"),
            domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("ds1"),
            &partition(),
            7,
        );
        assert_eq!(
            key.as_str(),
            "manifests/v1/org1/logs/ds1/p-1755302400-00-3600s/7.parquet"
        );
    }

    #[test]
    fn partitions_starting_together_have_their_own_manifest_keys() {
        let key = |end_micros| {
            StorageLayout::manifest_key(
                &Id::from_string("org1"),
                domain::storage::StreamTypeId::LOGS,
                &PhysicalDatasetId::from_string("ds1"),
                &Partition {
                    end_micros,
                    ..partition()
                },
                1,
            )
        };

        let day = partition().start_micros + 24 * 3_600_000_000;

        assert_ne!(key(partition().end_micros), key(day));
        assert!(
            key(day)
                .as_str()
                .ends_with("/p-1755302400-00-86400s/1.parquet")
        );
    }

    #[test]
    fn negative_partition_start_keeps_sign() {
        let key = StorageLayout::manifest_key(
            &Id::from_string("o"),
            domain::storage::StreamTypeId::LOGS,
            &PhysicalDatasetId::from_string("d"),
            &Partition {
                start_micros: -3_600_000_000,
                end_micros: 0,
                shard: 3,
            },
            1,
        );
        assert!(key.as_str().contains("/p--3600-03-3600s/"));
    }

    #[test]
    fn search_job_result_key_follows_the_category_version_organization_order() {
        let key = StorageLayout::search_job_result_key(
            &Id::from_string("system_4ddfcd4b594f8156d9ab8a2cdf799c2b"),
            &Id::from_string("job1"),
            2,
        );
        assert_eq!(
            key.as_str(),
            "search_jobs/v1/system_4ddfcd4b594f8156d9ab8a2cdf799c2b/job1-2.ndjson"
        );
    }

    #[test]
    fn search_job_results_of_one_job_differ_by_attempt_and_organization() {
        let org = Id::from_string("org1");
        let job = Id::from_string("job1");
        let first = StorageLayout::search_job_result_key(&org, &job, 0);
        let retried = StorageLayout::search_job_result_key(&org, &job, 1);
        let other = StorageLayout::search_job_result_key(&Id::from_string("org2"), &job, 0);

        assert_ne!(first, retried);
        assert_ne!(first, other);
        assert!(first.as_str().starts_with("search_jobs/v1/org1/"));
        assert!(other.as_str().starts_with("search_jobs/v1/org2/"));
    }

    #[test]
    fn search_job_results_are_outside_the_catalog_scan() {
        let org = Id::from_string("org1");
        let key = StorageLayout::search_job_result_key(&org, &Id::from_string("job1"), 0);

        // 孤儿扫描把前缀内 Catalog 不认识的对象当垃圾回收；任务结果只记在任务行上。
        for prefix in StorageLayout::catalog_scan_prefixes() {
            assert!(!key.as_str().starts_with(prefix), "{prefix}");
        }

        for prefix in StorageLayout::catalog_scan_prefixes_for(&org) {
            assert!(!key.as_str().starts_with(&prefix), "{prefix}");
        }

        // 桶的顶层只有布局自己的目录，没有以组织 ID 命名的目录。
        assert!(key.as_str().starts_with(StorageLayout::SEARCH_JOB_PREFIX));
        assert!(!key.as_str().starts_with("org1/"));
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
    fn wal_object_keys_list_in_sequence_order_and_read_back() {
        let org = Id::from_string("org");
        let dataset = PhysicalDatasetId::from_string("dataset");
        let key = StorageLayout::wal_object_key(&org, &dataset, 7, 3).0;

        assert_eq!(
            key,
            "wal/v1/org/dataset/00000000000000000007/00000000000000000003.wal"
        );
        assert!(key.starts_with(&StorageLayout::wal_stream_prefix(&org, &dataset, 7)));
        assert_eq!(StorageLayout::wal_object_sequence(&key), Some(3));
        assert_eq!(StorageLayout::wal_object_sequence("wal/v1/org/x.tmp"), None);
        assert!(
            StorageLayout::wal_object_key(&org, &dataset, 7, 9).0
                < StorageLayout::wal_object_key(&org, &dataset, 7, 10).0
        );
        assert!(
            StorageLayout::catalog_scan_prefixes()
                .iter()
                .all(|prefix| !key.starts_with(prefix))
        );
    }

    #[test]
    fn an_organization_purge_reaches_the_keys_made_for_it_and_no_others() {
        let org = Id::from_string("org");
        let dataset = PhysicalDatasetId::from_string("dataset");

        let keys = [
            StorageLayout::artifact_key(
                &org,
                StreamTypeId::LOGS,
                &dataset,
                &partition(),
                &SegmentId::from_string("segment"),
                &ArtifactId::from_string("data"),
                &ArtifactTypeId::builtin(type_id::builtin::ARTIFACT_PARQUET),
            )
            .0,
            StorageLayout::manifest_key(&org, StreamTypeId::LOGS, &dataset, &partition(), 1).0,
            StorageLayout::search_job_result_key(&org, &Id::from_string("job"), 0).0,
            StorageLayout::report_export_key(&org, "s3://bucket/weekly", "r1-20261110.pdf").0,
            StorageLayout::wal_object_key(&org, &dataset, 7, 3).0,
            profiles::archive_object_key(
                &org,
                "api",
                "cpu",
                1_755_302_400_000_000,
                &Id::from_string("profile"),
            ),
        ];

        let own = StorageLayout::organization_prefixes(&org);
        let other = StorageLayout::organization_prefixes(&Id::from_string("org2"));

        for key in &keys {
            assert!(
                own.iter().any(|prefix| key.starts_with(prefix.as_str())),
                "{key}"
            );
            assert!(
                other.iter().all(|prefix| !key.starts_with(prefix.as_str())),
                "{key}"
            );
        }
    }

    #[test]
    fn report_exports_stay_inside_their_organization() {
        let org = Id::from_string("org");

        for (target, key) in [
            ("s3://bucket/weekly/", "reports/org/bucket/weekly/r1.pdf"),
            ("S3://weekly", "reports/org/weekly/r1.pdf"),
            ("weekly", "reports/org/weekly/r1.pdf"),
            (
                "s3://../../artifacts/v1/other",
                "reports/org/artifacts/v1/other/r1.pdf",
            ),
            ("s3://", "reports/org/r1.pdf"),
        ] {
            assert_eq!(
                StorageLayout::report_export_key(&org, target, "r1.pdf").as_str(),
                key,
                "{target}"
            );
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
