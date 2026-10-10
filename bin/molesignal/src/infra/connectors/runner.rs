// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ConnectorRunner。
//!
//! 周期调度 pull 类 connector（当前：CloudWatch Logs）：
//! - 每个 tick 从 `ConnectorRepository::list_enabled` 拉所有 enabled connector；
//! - 对本节点获准运行的 pull 类调 `PullConnector::run_once`（拉 `[last_run, now]` 窗口事件），
//!   把事件经 [`IntakeService`] 写入 connector 的 `target_stream`，成功后 `touch_last_run`；
//! - push 类不进 runner，由 HTTP layer 直接 dispatch。
//!
//! **同一时刻只能有一个节点拉取同一个 connector**，否则外部源会被重复拉取、事件重复写入。多个
//! alert 节点按 connector id 分担：开跑前先过 [`RunGate`]（归属 + 运行租约）。`last_run_at`
//! 是拉取窗口的游标（写入失败时不推进，下个 tick 重试同一窗口），所以认领后必须重读 connector
//! 再拉，不能用列表时的旧快照。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use super::{
    ConnectorKind, PullConnector, PullContext,
    cloudwatch_logs::CloudWatchLogsConnector,
    repo::{Connector, ConnectorRepository},
};
use crate::{
    app::intake::IntakeService,
    domain::{
        intake::IntakeBatch,
        run_claim::{OpenGate, RunGate},
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "connector_pull";

/// 按 connector 配置构造拉取端；生产用 [`build_pull`]，测试注入假的。
type PullFactory = Arc<dyn Fn(&Connector) -> Box<dyn PullConnector> + Send + Sync>;

pub struct ConnectorRunner {
    repo: Arc<dyn ConnectorRepository>,
    intake: Arc<IntakeService>,
    pull_interval: Duration,
    /// 多个 alert 节点分担 connector 时的准入；默认每个 pull 类 connector 都拉。
    gate: Arc<dyn RunGate>,
    pull_factory: PullFactory,
}

impl ConnectorRunner {
    pub fn new(
        repo: Arc<dyn ConnectorRepository>,
        intake: Arc<IntakeService>,
        pull_interval_secs: u64,
    ) -> Self {
        Self {
            repo,
            intake,
            pull_interval: Duration::from_secs(pull_interval_secs.max(5)),
            gate: Arc::new(OpenGate),
            pull_factory: Arc::new(build_pull),
        }
    }

    /// 与其余 alert 节点分担 connector：每个 pull 类 connector 开拉前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
    }

    pub fn spawn(self) -> JoinHandle<()> {
        let interval = self.pull_interval;

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);

            loop {
                tick.tick().await;
                self.tick_once().await;
            }
        })
    }

    /// 单次 tick：返回本节点拉取的 connector 数。
    pub async fn tick_once(&self) -> usize {
        let connectors = match self.repo.list_enabled().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "connector_runner: list_enabled failed");
                return 0;
            }
        };

        let mut pulled = 0;

        for c in connectors {
            if !is_pull(&c) {
                continue; // push / egress 类不进 runner
            }

            let permit = match self.gate.admit(RUN_KIND, c.id.as_str()).await {
                Ok(Some(permit)) => permit,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(error = %e, connector_id = %c.id.0, "connector admission failed");
                    continue;
                }
            };

            // 认领后重读：游标 `last_run_at` 可能刚被另一个节点推进。
            match self.repo.get(&c.org_id, &c.id).await {
                Ok(fresh) if fresh.enabled => {
                    self.pull_once(&fresh).await;
                    pulled += 1;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, connector_id = %c.id.0, "connector reload failed");
                }
            }

            if let Err(e) = permit.release().await {
                tracing::warn!(error = %e, connector_id = %c.id.0, "connector claim release failed");
            }
        }

        pulled
    }

    async fn pull_once(&self, c: &Connector) {
        let pull = (self.pull_factory)(c);

        let ctx = PullContext {
            org_id: c.org_id.clone(),
            connector_id: c.id.clone(),
            last_run_at_micros: c.last_run_at.map(|t| t.0),
        };

        match pull.run_once(&ctx).await {
            Ok(events) => {
                if !events.is_empty() {
                    let batch = IntakeBatch {
                        batch_id: Id::new(),
                        org_id: c.org_id.clone(),
                        stream: target_stream(c),
                        stream_type: StreamType::LOGS,
                        events,
                        received_at: TimestampMicros::now(),
                    };

                    if let Err(e) = self.intake.intake(batch).await {
                        // 不推进 last_run → 下个 tick 重试同窗口，不丢数据。
                        tracing::warn!(error = %e, connector_id = %c.id.0, "connector intake failed");
                        return;
                    }
                }

                let _ = self
                    .repo
                    .touch_last_run(&c.id, TimestampMicros::now())
                    .await;
            }
            Err(e) => {
                tracing::warn!(error = %e, connector_id = %c.id.0, "connector pull failed");
            }
        }
    }
}

/// 是否由 runner 周期拉取（push / egress 类不是）。
fn is_pull(c: &Connector) -> bool {
    matches!(
        c.kind.parse::<ConnectorKind>(),
        Ok(ConnectorKind::AwsCloudwatchLogs)
    )
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

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{
        app::cluster::{PeerRole, ShardedRunGate, WorkOwnership, ownership::test_support::Nodes},
        bootstrap::test_support::FixedStreams,
        domain::{intake::RawEvent, run_claim::MemoryRunClaims},
        infra::intake_sink::MemoryIntakeSink,
        shared::Result,
    };

    /// 只记录游标推进的 connector 仓储：`list_enabled` 返回调用方给定的（可能过期的）快照，
    /// `get` 返回最新的一份。
    struct Cursor {
        snapshot: Connector,
        fresh: Mutex<Connector>,
        touches: AtomicUsize,
    }

    #[async_trait]
    impl ConnectorRepository for Cursor {
        async fn create(&self, _c: Connector) -> Result<Connector> {
            unimplemented!()
        }
        async fn update(&self, _c: Connector) -> Result<Connector> {
            unimplemented!()
        }
        async fn touch_last_run(&self, _id: &Id, ts: TimestampMicros) -> Result<()> {
            self.fresh.lock().unwrap().last_run_at = Some(ts);
            self.touches.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn get(&self, _org_id: &Id, _id: &Id) -> Result<Connector> {
            Ok(self.fresh.lock().unwrap().clone())
        }
        async fn delete(&self, _org_id: &Id, _id: &Id) -> Result<()> {
            unimplemented!()
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<Connector>> {
            unimplemented!()
        }
        async fn list_enabled(&self) -> Result<Vec<Connector>> {
            Ok(vec![self.snapshot.clone()])
        }
        async fn find_for_push(&self, _kind: &str, _push_token: &str) -> Result<Option<Connector>> {
            unimplemented!()
        }
    }

    /// 记录每次拉取使用的窗口起点，不产生事件。
    #[derive(Default)]
    struct Pulls(Mutex<Vec<Option<i64>>>);

    struct RecordingPull(Arc<Pulls>);

    #[async_trait]
    impl PullConnector for RecordingPull {
        async fn run_once(&self, ctx: &PullContext) -> Result<Vec<RawEvent>> {
            self.0.0.lock().unwrap().push(ctx.last_run_at_micros);
            Ok(Vec::new())
        }
    }

    fn connector(last_run_at: Option<i64>) -> Connector {
        Connector {
            id: Id::from_string("cw-1"),
            org_id: Id::from_string("org-1"),
            name: "cloudwatch".into(),
            kind: "aws_cloudwatch_logs".into(),
            config_json: serde_json::json!({}),
            enabled: true,
            last_run_at: last_run_at.map(TimestampMicros),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn runner(repo: Arc<Cursor>, gate: Arc<dyn RunGate>, pulls: &Arc<Pulls>) -> ConnectorRunner {
        let intake = Arc::new(IntakeService::new(
            Arc::new(MemoryIntakeSink::default()),
            Arc::new(FixedStreams(Vec::new())),
        ));
        let pulls = pulls.clone();
        let mut runner = ConnectorRunner::new(repo, intake, 30).with_gate(gate);
        runner.pull_factory = Arc::new(move |_: &Connector| -> Box<dyn PullConnector> {
            Box::new(RecordingPull(pulls.clone()))
        });
        runner
    }

    fn sharded(
        nodes: &Arc<Nodes>,
        node_id: &str,
        claims: &Arc<MemoryRunClaims>,
    ) -> Arc<dyn RunGate> {
        let ownership = WorkOwnership::new(
            nodes.clone(),
            node_id,
            PeerRole::Alert,
            std::time::Duration::ZERO,
        );
        Arc::new(ShardedRunGate::new(ownership, claims.clone(), node_id))
    }

    #[tokio::test]
    async fn a_pull_connector_is_pulled_from_the_cursor_and_the_cursor_advances() {
        let repo = Arc::new(Cursor {
            snapshot: connector(Some(7)),
            fresh: Mutex::new(connector(Some(7))),
            touches: AtomicUsize::new(0),
        });
        let pulls = Arc::new(Pulls::default());
        let runner = runner(repo.clone(), Arc::new(OpenGate), &pulls);

        assert_eq!(runner.tick_once().await, 1);
        assert_eq!(*pulls.0.lock().unwrap(), [Some(7)]);
        assert_eq!(repo.touches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_window_starts_at_the_fresh_cursor_not_the_listed_snapshot() {
        // 列表之后另一个节点已经把游标推进到 99：这一轮必须从 99 起拉。
        let repo = Arc::new(Cursor {
            snapshot: connector(Some(7)),
            fresh: Mutex::new(connector(Some(99))),
            touches: AtomicUsize::new(0),
        });
        let pulls = Arc::new(Pulls::default());
        let runner = runner(repo, Arc::new(OpenGate), &pulls);

        runner.tick_once().await;
        assert_eq!(*pulls.0.lock().unwrap(), [Some(99)]);
    }

    #[tokio::test]
    async fn a_connector_held_by_another_node_is_not_pulled() {
        let claims = Arc::new(MemoryRunClaims::default());
        let a = sharded(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let b = sharded(&Nodes::of(&[("b", PeerRole::Alert)]), "b", &claims);

        // a 正在拉取（租约未释放）：b 虽然也认为自己拥有它，这一轮不能再拉。
        let _held = a
            .admit(RUN_KIND, "cw-1")
            .await
            .unwrap()
            .expect("a claims it");
        let repo = Arc::new(Cursor {
            snapshot: connector(None),
            fresh: Mutex::new(connector(None)),
            touches: AtomicUsize::new(0),
        });
        let pulls = Arc::new(Pulls::default());
        let runner = runner(repo.clone(), b, &pulls);

        assert_eq!(runner.tick_once().await, 0);
        assert!(pulls.0.lock().unwrap().is_empty());
        assert_eq!(repo.touches.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn the_claim_is_released_after_the_pull_so_the_next_tick_can_run() {
        let claims = Arc::new(MemoryRunClaims::default());
        let gate = sharded(&Nodes::of(&[("a", PeerRole::Alert)]), "a", &claims);
        let repo = Arc::new(Cursor {
            snapshot: connector(None),
            fresh: Mutex::new(connector(None)),
            touches: AtomicUsize::new(0),
        });
        let pulls = Arc::new(Pulls::default());
        let runner = runner(repo, gate, &pulls);

        assert_eq!(runner.tick_once().await, 1);
        assert_eq!(runner.tick_once().await, 1);
        assert_eq!(pulls.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn push_connectors_never_reach_the_runner() {
        let mut push = connector(None);
        push.kind = "s3".into();
        let repo = Arc::new(Cursor {
            snapshot: push.clone(),
            fresh: Mutex::new(push),
            touches: AtomicUsize::new(0),
        });
        let pulls = Arc::new(Pulls::default());
        let runner = runner(repo, Arc::new(OpenGate), &pulls);

        assert_eq!(runner.tick_once().await, 0);
        assert!(pulls.0.lock().unwrap().is_empty());
    }
}
