// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! StreamTypeRegistry：可扩展类型与处理能力的注册中心。
//!
//! 公共层不得对 signal 类型做 `match`；写入、查询、索引能力一律经注册分派。
//! 遇到未注册类型时元数据可以安全保留，读写返回明确错误，通用任务（GC、
//! 对象检查）仍可根据 Artifact 元数据运行——绝不把未知类型默认为 logs。
//!
//! 启动后不可变，可被各处直接缓存。

use std::{collections::BTreeMap, sync::OnceLock};

use super::{
    dataset::{IndexPolicy, PartitionPolicy, PhysicalDatasetSpec, StoragePolicy},
    type_id::{ArtifactTypeId, DatasetTypeId, IndexTypeId, StreamTypeId, WalCodecId, builtin},
};
use crate::{
    domain::stream::StreamType,
    shared::{Error, Result},
};

/// 一种物理数据集类型的静态描述。
#[derive(Debug, Clone)]
pub struct DatasetTypeDescriptor {
    pub id: DatasetTypeId,
    pub version: u32,
    /// 主数据格式；Reader 按 artifact_type + format_version 选择，不看路径后缀。
    pub primary_artifact: ArtifactTypeId,
    /// 该数据集默认构建的索引。
    pub indexers: Vec<IndexTypeId>,
    pub wal_codec: WalCodecId,
    pub default_partition_policy: PartitionPolicy,
    pub default_storage_policy: StoragePolicy,
    /// 是否在 stream 建立时立即建集（raw 数据集）；派生摘要由其生产者按需建。
    pub provision_eagerly: bool,
    /// 该 stream 的默认写入目标。持久化类型必须且只能注册一个。
    pub primary: bool,
    /// 普通逻辑查询是否透明合并该数据集。
    pub query_by_default: bool,
}

impl DatasetTypeDescriptor {
    /// 按描述符默认值展开为建集规格。
    pub fn to_spec(&self) -> PhysicalDatasetSpec {
        PhysicalDatasetSpec {
            dataset_type: self.id.clone(),
            dataset_type_version: self.version,
            wal_codec: self.wal_codec.clone(),
            partition_policy: self.default_partition_policy.clone(),
            storage_policy: self.default_storage_policy.clone(),
            index_policy: IndexPolicy {
                indexers: self.indexers.clone(),
            },
        }
    }
}

/// 一种信号类型的静态描述。
#[derive(Debug, Clone)]
pub struct StreamTypeDescriptor {
    pub id: StreamTypeId,
    pub version: u32,
    /// 允许的物理数据集集合；空集 = 该类型不落盘（如 extend 表）。
    pub datasets: Vec<DatasetTypeDescriptor>,
}

impl StreamTypeDescriptor {
    pub fn persists_data(&self) -> bool {
        !self.datasets.is_empty()
    }
}

/// 类型注册中心。bootstrap 阶段构建一次，之后只读。
#[derive(Debug, Default)]
pub struct StreamTypeRegistry {
    types: BTreeMap<String, StreamTypeDescriptor>,
}

impl StreamTypeRegistry {
    pub fn new(descriptors: Vec<StreamTypeDescriptor>) -> Result<Self> {
        let mut types = BTreeMap::new();
        let mut dataset_owners = BTreeMap::new();
        for descriptor in descriptors {
            let primary_count = descriptor
                .datasets
                .iter()
                .filter(|dataset| dataset.primary)
                .count();
            if (!descriptor.datasets.is_empty() && primary_count != 1)
                || (descriptor.datasets.is_empty() && primary_count != 0)
                || descriptor
                    .datasets
                    .iter()
                    .any(|dataset| dataset.primary && !dataset.query_by_default)
            {
                return Err(Error::invalid(format!(
                    "stream type `{}` must register exactly one query-visible primary dataset",
                    descriptor.id
                )));
            }
            for dataset in &descriptor.datasets {
                if let Some(owner) =
                    dataset_owners.insert(dataset.id.as_str().to_owned(), descriptor.id)
                {
                    return Err(Error::invalid(format!(
                        "dataset type `{}` registered by both `{}` and `{}`",
                        dataset.id, owner, descriptor.id
                    )));
                }
            }
            if types
                .insert(descriptor.id.as_str().to_owned(), descriptor)
                .is_some()
            {
                return Err(Error::invalid("duplicate stream type descriptor"));
            }
        }
        Ok(Self { types })
    }

    /// 未注册时返回明确错误；调用方不得回退到任何默认类型。
    pub fn stream_type(&self, stream_type: &StreamTypeId) -> Result<&StreamTypeDescriptor> {
        self.types
            .get(stream_type.as_str())
            .ok_or_else(|| Error::invalid(format!("unsupported stream type `{stream_type}`")))
    }

    pub fn dataset_type(
        &self,
        stream_type: &StreamTypeId,
        dataset_type: &DatasetTypeId,
    ) -> Result<&DatasetTypeDescriptor> {
        self.stream_type(stream_type)?
            .datasets
            .iter()
            .find(|d| d.id == *dataset_type)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "unsupported dataset type `{dataset_type}` for stream type `{stream_type}`"
                ))
            })
    }

    /// stream 建立时应立即建的物理数据集规格。
    pub fn eager_dataset_specs(
        &self,
        stream_type: &StreamTypeId,
    ) -> Result<Vec<PhysicalDatasetSpec>> {
        Ok(self
            .stream_type(stream_type)?
            .datasets
            .iter()
            .filter(|d| d.provision_eagerly)
            .map(DatasetTypeDescriptor::to_spec)
            .collect())
    }

    pub fn primary_dataset_type(&self, stream_type: &StreamTypeId) -> Result<DatasetTypeId> {
        self.stream_type(stream_type)?
            .datasets
            .iter()
            .find(|dataset| dataset.primary)
            .map(|dataset| dataset.id.clone())
            .ok_or_else(|| Error::invalid(format!("{stream_type} has no physical dataset")))
    }

    pub fn logical_query_dataset_types(
        &self,
        stream_type: &StreamTypeId,
    ) -> Result<Vec<DatasetTypeId>> {
        let datasets = self
            .stream_type(stream_type)?
            .datasets
            .iter()
            .filter(|dataset| dataset.query_by_default)
            .map(|dataset| dataset.id.clone())
            .collect::<Vec<_>>();
        if datasets.is_empty() {
            return Err(Error::invalid(format!(
                "{stream_type} has no queryable physical dataset"
            )));
        }
        Ok(datasets)
    }

    pub fn iter(&self) -> impl Iterator<Item = &StreamTypeDescriptor> {
        self.types.values()
    }
}

/// 当前内置信号的主写入数据集。调用方将返回的开放 ID 直接贯穿 WAL、Buffer 和
/// Catalog；不再经过封闭的“raw/summary”枚举。
pub fn primary_dataset_type(stream_type: StreamType) -> Result<DatasetTypeId> {
    builtin_registry_ref().primary_dataset_type(&stream_type)
}

/// 重建一个用户可见逻辑 stream 时透明读取的开放 Dataset 类型。
///
/// Metrics 的历史 samples 可由 compactor 原子替换为 rollup；其余派生数据集是
/// 专用读模型，不混入通用查询。
pub fn logical_query_dataset_types(stream_type: StreamType) -> Result<Vec<DatasetTypeId>> {
    builtin_registry_ref().logical_query_dataset_types(&stream_type)
}

fn parquet_dataset(
    id: &'static str,
    with_tantivy: bool,
    provision_eagerly: bool,
    primary: bool,
    query_by_default: bool,
) -> DatasetTypeDescriptor {
    DatasetTypeDescriptor {
        id: DatasetTypeId::builtin(id),
        version: 1,
        primary_artifact: ArtifactTypeId::builtin(builtin::ARTIFACT_PARQUET),
        indexers: if with_tantivy {
            vec![IndexTypeId::builtin(builtin::INDEX_TANTIVY)]
        } else {
            vec![]
        },
        wal_codec: WalCodecId::builtin(builtin::WAL_CODEC_ROW_BATCH),
        default_partition_policy: PartitionPolicy::default(),
        default_storage_policy: StoragePolicy::default(),
        provision_eagerly,
        primary,
        query_by_default,
    }
}

/// 内置信号类型注册表。新增信号只需在各自模块注册新的描述符并在 bootstrap
/// 装配，不改这里以外的公共代码。
pub fn builtin_registry() -> StreamTypeRegistry {
    let descriptors = vec![
        StreamTypeDescriptor {
            id: StreamTypeId::builtin(builtin::STREAM_LOGS),
            version: 1,
            datasets: vec![
                parquet_dataset(builtin::DATASET_LOG_RECORDS, true, true, true, true),
                parquet_dataset(
                    builtin::DATASET_RUM_SESSION_SUMMARY,
                    false,
                    false,
                    false,
                    false,
                ),
                parquet_dataset(
                    builtin::DATASET_RUM_ACTION_SUMMARY,
                    false,
                    false,
                    false,
                    false,
                ),
                parquet_dataset(
                    builtin::DATASET_RUM_ERROR_SUMMARY,
                    false,
                    false,
                    false,
                    false,
                ),
            ],
        },
        StreamTypeDescriptor {
            id: StreamTypeId::builtin(builtin::STREAM_METRICS),
            version: 1,
            datasets: vec![
                parquet_dataset(builtin::DATASET_METRIC_SAMPLES, false, true, true, true),
                parquet_dataset(builtin::DATASET_METRIC_ROLLUP, false, false, false, true),
                parquet_dataset(builtin::DATASET_METRIC_CATALOG, false, false, false, false),
            ],
        },
        StreamTypeDescriptor {
            id: StreamTypeId::builtin(builtin::STREAM_TRACES),
            version: 1,
            datasets: vec![
                parquet_dataset(builtin::DATASET_TRACE_SPANS, true, true, true, true),
                parquet_dataset(builtin::DATASET_TRACE_SUMMARY, false, false, false, false),
            ],
        },
        StreamTypeDescriptor {
            id: StreamTypeId::builtin(builtin::STREAM_PROFILES),
            version: 1,
            datasets: vec![parquet_dataset(
                builtin::DATASET_PROFILE_SAMPLES,
                false,
                true,
                true,
                true,
            )],
        },
        // extend 表：静态 KV 直接进内存表，不落盘、无物理数据集。
        StreamTypeDescriptor {
            id: StreamTypeId::builtin(builtin::STREAM_EXTEND),
            version: 1,
            datasets: vec![],
        },
    ];
    StreamTypeRegistry::new(descriptors).expect("builtin descriptors must be consistent")
}

fn builtin_registry_ref() -> &'static StreamTypeRegistry {
    static REGISTRY: OnceLock<StreamTypeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(builtin_registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_resolves_all_builtin_dataset_types() {
        let registry = builtin_registry();
        for (stream_type, dataset_type) in [
            (StreamType::LOGS, builtin::DATASET_LOG_RECORDS),
            (StreamType::LOGS, builtin::DATASET_RUM_SESSION_SUMMARY),
            (StreamType::LOGS, builtin::DATASET_RUM_ACTION_SUMMARY),
            (StreamType::LOGS, builtin::DATASET_RUM_ERROR_SUMMARY),
            (StreamType::METRICS, builtin::DATASET_METRIC_SAMPLES),
            (StreamType::METRICS, builtin::DATASET_METRIC_ROLLUP),
            (StreamType::METRICS, builtin::DATASET_METRIC_CATALOG),
            (StreamType::TRACES, builtin::DATASET_TRACE_SPANS),
            (StreamType::TRACES, builtin::DATASET_TRACE_SUMMARY),
            (StreamType::PROFILES, builtin::DATASET_PROFILE_SAMPLES),
        ] {
            let stream_type_id = stream_type;
            let dataset_type_id = DatasetTypeId::builtin(dataset_type);
            registry
                .dataset_type(&stream_type_id, &dataset_type_id)
                .unwrap_or_else(|e| panic!("{stream_type:?}/{dataset_type}: {e}"));
        }
    }

    #[test]
    fn logical_query_types_include_metric_rollup_only_for_metrics() {
        assert_eq!(
            logical_query_dataset_types(StreamType::METRICS).unwrap(),
            vec![
                DatasetTypeId::builtin(builtin::DATASET_METRIC_SAMPLES),
                DatasetTypeId::builtin(builtin::DATASET_METRIC_ROLLUP),
            ]
        );
        assert_eq!(
            logical_query_dataset_types(StreamType::LOGS).unwrap(),
            vec![DatasetTypeId::builtin(builtin::DATASET_LOG_RECORDS)]
        );
    }

    #[test]
    fn extend_streams_have_no_datasets() {
        let registry = builtin_registry();
        let extend = registry
            .stream_type(&StreamTypeId::builtin(builtin::STREAM_EXTEND))
            .unwrap();
        assert!(!extend.persists_data());
        assert!(registry.eager_dataset_specs(&extend.id).unwrap().is_empty());
    }

    #[test]
    fn unknown_type_is_an_explicit_error_not_a_default() {
        let registry = builtin_registry();
        let unknown = StreamTypeId::new("vendor.mystery").unwrap();
        assert!(registry.stream_type(&unknown).is_err());
    }

    #[test]
    fn duplicate_dataset_type_across_streams_is_rejected() {
        let duplicated = vec![
            StreamTypeDescriptor {
                id: StreamTypeId::builtin(builtin::STREAM_LOGS),
                version: 1,
                datasets: vec![parquet_dataset(
                    builtin::DATASET_LOG_RECORDS,
                    false,
                    true,
                    true,
                    true,
                )],
            },
            StreamTypeDescriptor {
                id: StreamTypeId::builtin(builtin::STREAM_TRACES),
                version: 1,
                datasets: vec![parquet_dataset(
                    builtin::DATASET_LOG_RECORDS,
                    false,
                    true,
                    true,
                    true,
                )],
            },
        ];
        assert!(StreamTypeRegistry::new(duplicated).is_err());
    }

    #[test]
    fn eager_specs_only_include_raw_datasets() {
        let registry = builtin_registry();
        let specs = registry
            .eager_dataset_specs(&StreamTypeId::builtin(builtin::STREAM_METRICS))
            .unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(
            specs[0].dataset_type.as_str(),
            builtin::DATASET_METRIC_SAMPLES
        );
    }
}
