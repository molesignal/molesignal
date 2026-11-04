// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 拨测结果保留：compactor 主节点每天清理一轮。
//!
//! 先删过期的附件（附件有自己的保留天数），再删早于保留期的结果和它们的附件，最后删已结束、
//! 不再有结果引用的任务。保留天数在平台设置「数据面 → 保留期」修改
//! （`instance_settings.synthetic_result_retention_days`，默认 30 天，0 = 永久保留），每轮
//! 现读。行先删，对象后删：删不掉的对象只占空间，不会留下指向空处的记录。

use std::{sync::Arc, time::Duration};

use object_store::{ObjectStore, ObjectStoreExt as _, path::Path as ObjectPath};
use tokio::task::JoinHandle;

use crate::{
    app::cluster::WorkOwnership,
    domain::{iam::InstanceSettingsRepository, synthetics::SyntheticRepository},
    shared::{Result, drain::DrainController, time::TimestampMicros},
};

/// 两轮清理的间隔。
const SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// 启动后先等一会儿再清理，避开启动时的回放与预热。
const STARTUP_DELAY: Duration = Duration::from_secs(5 * 60);
/// 非主节点多久后再检查一次自己是否已成为主节点。
const NOT_PRIMARY_RECHECK: Duration = Duration::from_secs(60);
/// 清理失败后多久重试，不必等满一天。
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(60 * 60);
const BATCH_SIZE: u32 = 1_000;
/// 两批之间歇一下，不和在线请求抢数据库。
const BATCH_PAUSE: Duration = Duration::from_millis(100);

/// 一轮清理删掉的东西。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub artifacts: u64,
    pub results: u64,
    pub tasks: u64,
}

pub struct SyntheticRetentionWorker {
    repository: Arc<dyn SyntheticRepository>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    objects: Arc<dyn ObjectStore>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
}

impl SyntheticRetentionWorker {
    pub fn new(
        repository: Arc<dyn SyntheticRepository>,
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        objects: Arc<dyn ObjectStore>,
        ownership: Arc<WorkOwnership>,
        drain: Arc<DrainController>,
    ) -> Self {
        Self {
            repository,
            instance_settings,
            objects,
            ownership,
            drain,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            tokio::time::sleep(STARTUP_DELAY).await;

            while !self.drain.is_draining() {
                // 非主节点隔一会儿再看一眼：接任主节点后不必等满一天。
                let delay = if !self.ownership.is_primary().await {
                    NOT_PRIMARY_RECHECK
                } else {
                    match self.sweep_once().await {
                        Ok(swept) => {
                            if swept != Swept::default() {
                                tracing::info!(?swept, "expired synthetic results deleted");
                            }

                            SWEEP_INTERVAL
                        }
                        Err(error) => {
                            tracing::warn!(%error, "synthetic retention sweep failed");
                            RETRY_AFTER_ERROR
                        }
                    }
                };

                tokio::time::sleep(delay).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.synthetic_retention",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "synthetic_retention")
    )]
    async fn sweep_once(&self) -> Result<Swept> {
        let now = TimestampMicros::now();
        let mut swept = Swept::default();

        loop {
            let keys = self
                .repository
                .purge_expired_artifacts(now, BATCH_SIZE)
                .await?;

            swept.artifacts += keys.len() as u64;
            let done = keys.len() < BATCH_SIZE as usize;
            self.delete_objects(keys).await;

            if done || !self.pause().await {
                break;
            }
        }

        let settings = self.instance_settings.get().await?;

        let Some(before) = settings.synthetic_result_retention_cutoff(now) else {
            return Ok(swept);
        };

        loop {
            let purged = self.repository.purge_results(before, BATCH_SIZE).await?;
            swept.results += purged.deleted;
            self.delete_objects(purged.object_keys).await;

            if purged.deleted < u64::from(BATCH_SIZE) || !self.pause().await {
                break;
            }
        }

        loop {
            let deleted = self
                .repository
                .purge_finished_tasks(before, BATCH_SIZE)
                .await?;

            swept.tasks += deleted;

            if deleted < u64::from(BATCH_SIZE) || !self.pause().await {
                break;
            }
        }

        Ok(swept)
    }

    /// 两批之间歇一下；节点退役时停在批与批之间，剩下的留给接任的主节点。
    async fn pause(&self) -> bool {
        if self.drain.is_draining() {
            return false;
        }

        tokio::time::sleep(BATCH_PAUSE).await;
        true
    }

    async fn delete_objects(&self, keys: Vec<String>) {
        for key in keys {
            let Ok(path) = ObjectPath::parse(&key) else {
                continue;
            };

            match self.objects.delete(&path).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => {}
                Err(error) => tracing::debug!(%key, %error, "synthetic artifact object kept"),
            }
        }
    }
}
