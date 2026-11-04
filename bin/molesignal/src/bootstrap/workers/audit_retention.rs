// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 审计记录保留：compactor 主节点每天按实例设置的保留天数分批删除过期的审计记录。
//!
//! 保留天数在平台设置的「审计保留」页修改（`instance_settings.audit_retention_days`，默认
//! 90 天，0 = 永久保留），每轮清理时现读，改动在下一轮生效。组织审计与平台审计在同一张表
//! 里，一起清理。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::cluster::WorkOwnership,
    domain::iam::InstanceSettingsRepository,
    infra::persistence::repositories::audit_events::AuditEventRepository,
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
const BATCH_SIZE: i64 = 5_000;
/// 两批之间歇一下，不和在线请求抢数据库。
const BATCH_PAUSE: Duration = Duration::from_millis(100);

pub struct AuditRetentionWorker {
    audit_events: Arc<dyn AuditEventRepository>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    ownership: Arc<WorkOwnership>,
    drain: Arc<DrainController>,
    batch_size: i64,
    batch_pause: Duration,
}

impl AuditRetentionWorker {
    pub fn new(
        audit_events: Arc<dyn AuditEventRepository>,
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        ownership: Arc<WorkOwnership>,
        drain: Arc<DrainController>,
    ) -> Self {
        Self {
            audit_events,
            instance_settings,
            ownership,
            drain,
            batch_size: BATCH_SIZE,
            batch_pause: BATCH_PAUSE,
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
                        Ok(deleted) => {
                            if deleted > 0 {
                                tracing::info!(deleted, "expired audit events deleted");
                            }

                            SWEEP_INTERVAL
                        }
                        Err(error) => {
                            tracing::warn!(%error, "audit retention sweep failed");
                            RETRY_AFTER_ERROR
                        }
                    }
                };

                tokio::time::sleep(delay).await;
            }
        })
    }

    /// 删除过了保留期的审计记录，返回删除条数；永久保留时什么也不删。
    #[tracing::instrument(
        name = "worker.audit_retention",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "audit_retention")
    )]
    async fn sweep_once(&self) -> Result<u64> {
        let settings = self.instance_settings.get().await?;

        let Some(before) = settings.audit_retention_cutoff(TimestampMicros::now()) else {
            return Ok(0);
        };

        let mut deleted = 0;

        loop {
            let batch = self
                .audit_events
                .delete_before(before, self.batch_size)
                .await?;

            deleted += batch;

            // 节点退役时停在批与批之间，剩下的留给接任的主节点。
            if batch < self.batch_size as u64 || self.drain.is_draining() {
                return Ok(deleted);
            }

            tokio::time::sleep(self.batch_pause).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use parking_lot::Mutex;

    use super::*;
    use crate::{
        app::cluster::PeerRole,
        domain::iam::{ClientIpResolverSettings, InstanceSettings},
        infra::persistence::repositories::audit_events::{
            AuditEvent, AuditQuery, AuditRangeSummary,
        },
        shared::ids::Id,
    };

    const DAY: i64 = 86_400 * 1_000_000;

    /// 只记时间戳的审计表。
    #[derive(Default)]
    struct Events {
        ts: Mutex<Vec<i64>>,
        deletes: Mutex<usize>,
    }

    #[async_trait]
    impl AuditEventRepository for Events {
        async fn record(&self, event: AuditEvent) -> Result<()> {
            self.ts.lock().push(event.ts.0);
            Ok(())
        }

        async fn list_recent(&self, _org_id: &Id, _limit: i64) -> Result<Vec<AuditEvent>> {
            Ok(Vec::new())
        }

        async fn query(&self, _org_id: &Id, _q: &AuditQuery) -> Result<Vec<AuditEvent>> {
            Ok(Vec::new())
        }

        async fn delete_before(&self, before: TimestampMicros, limit: i64) -> Result<u64> {
            *self.deletes.lock() += 1;
            let mut ts = self.ts.lock();
            ts.sort_unstable();

            let expired = ts.iter().take_while(|value| **value < before.0).count();
            let removed = expired.min(limit as usize);
            ts.drain(..removed);
            Ok(removed as u64)
        }

        async fn summarize_range(
            &self,
            _from: Option<TimestampMicros>,
            _before: TimestampMicros,
        ) -> Result<AuditRangeSummary> {
            Ok(AuditRangeSummary::default())
        }
    }

    struct Settings(u32);

    #[async_trait]
    impl InstanceSettingsRepository for Settings {
        async fn get(&self) -> Result<InstanceSettings> {
            Ok(InstanceSettings {
                signup_enabled: false,
                signup_require_approval: true,
                service_graph_source: "intake".into(),
                federation_cluster_id: String::new(),
                federation_drain_interval_secs: 10,
                federation_push_batch_size: 100,
                federation_seen_events_ttl_secs: 604_800,
                federation_gossip_interval_secs: 60,
                rum_client_ip_resolver: ClientIpResolverSettings::default(),
                audit_retention_days: self.0,
                default_retention_days: 30,
                synthetic_result_retention_days: 30,
                max_active_series_per_org: 200_000,
                max_active_series_per_metric: 100_000,
                apm_limits: Default::default(),
                alert_evaluation_delay_secs: 60,
                intake_usage_retention_days: 90,
                intake_usage_max_services_per_org: 1000,
                signin_limits: Default::default(),
                updated_at: TimestampMicros(0),
            })
        }

        async fn update(&self, settings: InstanceSettings) -> Result<InstanceSettings> {
            Ok(settings)
        }
    }

    /// 时间戳为「距今多少天」的一组记录。
    fn events_aged(days: &[i64]) -> Arc<Events> {
        let now = TimestampMicros::now().0;

        Arc::new(Events {
            ts: Mutex::new(days.iter().map(|age| now - age * DAY).collect()),
            deletes: Mutex::new(0),
        })
    }

    fn worker(events: Arc<Events>, retention_days: u32, batch_size: i64) -> AuditRetentionWorker {
        let mut worker = AuditRetentionWorker::new(
            events,
            Arc::new(Settings(retention_days)),
            WorkOwnership::solo(PeerRole::Compactor),
            Arc::new(DrainController::new()),
        );

        worker.batch_size = batch_size;
        worker.batch_pause = Duration::ZERO;
        worker
    }

    #[tokio::test]
    async fn records_older_than_the_retention_days_are_deleted_in_batches() {
        let events = events_aged(&[400, 200, 120, 91, 89, 1]);

        let deleted = worker(events.clone(), 90, 2).sweep_once().await.unwrap();
        assert_eq!(deleted, 4);
        assert_eq!(events.ts.lock().len(), 2);
        // 删满两批后再试一批，删不满一批就停。
        assert_eq!(*events.deletes.lock(), 3);
    }

    #[tokio::test]
    async fn zero_days_keeps_every_record() {
        let events = events_aged(&[4000, 400, 1]);

        let deleted = worker(events.clone(), 0, 2).sweep_once().await.unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(events.ts.lock().len(), 3);
        assert_eq!(*events.deletes.lock(), 0);
    }

    #[tokio::test]
    async fn a_draining_node_stops_between_batches() {
        let events = events_aged(&[400, 300, 200, 100]);
        let worker = worker(events.clone(), 90, 2);
        assert!(worker.drain.begin_drain());

        assert_eq!(worker.sweep_once().await.unwrap(), 2);
        assert_eq!(events.ts.lock().len(), 2);
    }
}
