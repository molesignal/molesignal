// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ConnectorRunner。
//!
//! 周期调度 pull 类 connector（当前：CloudWatch Logs）：
//! - 每个 tick 从 `ConnectorRepository::list_enabled` 拉所有 enabled connector；
//! - 对 pull 类调 `PullConnector::run_once`（拉 `[last_run, now]` 窗口事件），
//!   把事件经 [`IngestService`] 写入 connector 的 `target_stream`，成功后 `touch_last_run`；
//! - push 类不进 runner，由 HTTP layer 直接 dispatch。
//!
//! **单点运行**：应作为单例起（如 alert_manager 角色），避免多节点重复拉取同一外部源。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::{
    ConnectorKind, PullConnector, PullContext,
    cloudwatch_logs::CloudWatchLogsConnector,
    repo::{Connector, ConnectorRepository},
};
use crate::{
    app::ingestion::IngestService,
    domain::{ingestion::IngestBatch, stream::StreamType},
    shared::{ids::Id, time::TimestampMicros},
};

pub struct ConnectorRunner {
    repo: Arc<dyn ConnectorRepository>,
    ingest: Arc<IngestService>,
    pull_interval: Duration,
}

impl ConnectorRunner {
    pub fn new(
        repo: Arc<dyn ConnectorRepository>,
        ingest: Arc<IngestService>,
        pull_interval_secs: u64,
    ) -> Self {
        Self {
            repo,
            ingest,
            pull_interval: Duration::from_secs(pull_interval_secs.max(5)),
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        let repo = self.repo.clone();
        let ingest = self.ingest.clone();
        let interval = self.pull_interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                let connectors = match repo.list_enabled().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(error = %e, "connector_runner: list_enabled failed");
                        continue;
                    }
                };
                for c in connectors {
                    let Ok(kind) = c.kind.parse::<ConnectorKind>() else {
                        continue;
                    };
                    if !matches!(kind, ConnectorKind::AwsCloudwatchLogs) {
                        continue; // push / egress 类不进 runner
                    }
                    let pull = build_pull(&c);
                    let ctx = PullContext {
                        org_id: c.org_id.clone(),
                        connector_id: c.id.clone(),
                        last_run_at_micros: c.last_run_at.map(|t| t.0),
                    };
                    match pull.run_once(&ctx).await {
                        Ok(events) => {
                            if !events.is_empty() {
                                let batch = IngestBatch {
                                    batch_id: Id::new(),
                                    org_id: c.org_id.clone(),
                                    stream: target_stream(&c),
                                    stream_type: StreamType::Logs,
                                    events,
                                    received_at: TimestampMicros::now(),
                                };
                                if let Err(e) = ingest.ingest(batch).await {
                                    // 不推进 last_run → 下个 tick 重试同窗口，不丢数据。
                                    tracing::warn!(error = %e, connector_id = %c.id.0, "connector ingest failed");
                                    continue;
                                }
                            }
                            let _ = repo.touch_last_run(&c.id, TimestampMicros::now()).await;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, connector_id = %c.id.0, "connector pull failed");
                        }
                    }
                }
            }
        })
    }
}

fn cfg_str(c: &Connector, key: &str) -> String {
    c.config_json
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn target_stream(c: &Connector) -> String {
    let t = cfg_str(c, "target_stream");
    if t.trim().is_empty() {
        "default".to_string()
    } else {
        t
    }
}

fn build_pull(c: &Connector) -> Box<dyn PullConnector> {
    let session_token = cfg_str(c, "session_token");
    Box::new(CloudWatchLogsConnector {
        region: cfg_str(c, "region"),
        log_group: cfg_str(c, "log_group"),
        target_stream: target_stream(c),
        access_key: cfg_str(c, "access_key"),
        secret_key: cfg_str(c, "secret_key"),
        session_token: (!session_token.is_empty()).then_some(session_token),
    })
}
