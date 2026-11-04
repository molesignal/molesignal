// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ConnectorRunner。
//!
//! 周期调度 pull 类 connector（CloudWatch Logs、Kafka 主题、S3 对象）：
//! - 每个 tick 从 `ConnectorRepository::list_enabled` 拉所有 enabled connector；
//! - 对本节点获准运行的 pull 类调 `PullConnector::run_once`（从上次读到的地方拉一批事件），
//!   把事件经 [`IntakeService`] 写入 connector 的 `target_stream`，成功后才推进读取位点；来源说还有
//!   更多时接着拉，一个 tick 最多拉 [`PULL_BUDGET`] 这么久；
//! - push 类不进 runner，由 HTTP layer 直接 dispatch。
//!
//! **同一时刻只能有一个节点拉取同一个 connector**，否则外部源会被重复拉取、事件重复写入。多个
//! alert 节点按 connector id 分担：开跑前先过 [`RunGate`]（归属 + 运行租约）。`last_run_at` 和
//! `read_state` 是读取位点（读取或写入失败时不推进，下个 tick 重试同一段，所以写入是至少一次），
//! 认领后必须重读 connector 再拉，不能用列表时的旧快照。失败的原因记在 `read_state` 里给页面看。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio::task::JoinHandle;

use super::{
    ConnectorKind, PullBatch, PullConnector, PullContext,
    aws::{CredentialSource, credential_provider},
    cloudwatch_logs::CloudWatchLogsConnector,
    config::aws::AwsAuth,
    repo::{Connector, ConnectorRepository},
    sources::{kafka::KafkaSource, s3::S3Source},
};
use crate::{
    app::intake::{
        IntakeService,
        usage::{IntakeReceipt, IntakeUsageMeter},
    },
    domain::{
        intake::{
            IntakeBatch, RawEvent,
            usage::{event_bytes, source::IntakeCredential},
        },
        run_claim::{OpenGate, RunGate},
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "connector_pull";

/// 一个 tick 里一个 connector 连续拉取最多多久（运行租约是 10 分钟）。
const PULL_BUDGET: Duration = Duration::from_secs(120);

/// 一次写入最多多少条：一个大对象读出来的事件分几批写。
const MAX_EVENTS_PER_WRITE: usize = 10_000;

/// 失败原因最多留多少字。
const MAX_ERROR_CHARS: usize = 500;

/// 按 connector 配置构造拉取端；生产用 [`build_pull`]，测试注入假的。
type PullFactory = Arc<dyn Fn(&Connector) -> Box<dyn PullConnector> + Send + Sync>;

pub struct ConnectorRunner {
    repo: Arc<dyn ConnectorRepository>,
    intake: Arc<IntakeService>,
    pull_interval: Duration,
    /// 多个 alert 节点分担 connector 时的准入；默认每个 pull 类 connector 都拉。
    gate: Arc<dyn RunGate>,
    pull_factory: PullFactory,
    /// Counts what the connectors bring in, for the usage page. Tests go without.
    usage_meter: Option<Arc<IntakeUsageMeter>>,
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
            usage_meter: None,
        }
    }

    /// 与其余 alert 节点分担 connector：每个 pull 类 connector 开拉前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
    }

    pub fn with_usage_meter(mut self, meter: Arc<IntakeUsageMeter>) -> Self {
        self.usage_meter = Some(meter);
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
        let started = Instant::now();
        let mut state = ReadState::of(&c.read_state);
        let mut last_run_at = c.last_run_at;

        loop {
            let ctx = PullContext {
                org_id: c.org_id.clone(),
                connector_id: c.id.clone(),
                last_run_at_micros: last_run_at.map(|t| t.0),
                position: state.position.clone(),
            };

            let batch = match pull.run_once(&ctx).await {
                Ok(batch) => batch,
                Err(e) => {
                    tracing::warn!(error = %e, connector_id = %c.id.0, "connector pull failed");
                    self.save_failure(c, &mut state, &e.to_string()).await;
                    return;
                }
            };

            let PullBatch {
                events,
                position,
                more,
                warning,
            } = batch;

            // 不推进位点 → 下个 tick 重试同一段，不丢数据。
            if let Err(e) = self.write(c, events).await {
                tracing::warn!(error = %e, connector_id = %c.id.0, "connector intake failed");
                self.save_failure(c, &mut state, &e.to_string()).await;
                return;
            }

            let now = TimestampMicros::now();
            last_run_at = Some(now);
            state.succeeded(position, warning, now);

            if let Err(e) = self
                .repo
                .save_read_state(&c.id, Some(now), &state.to_value())
                .await
            {
                // 位点没存下：下次从旧位点再读一遍，事件可能重复，但不丢。
                tracing::warn!(error = %e, connector_id = %c.id.0, "connector read state not saved");
                return;
            }

            if !more || started.elapsed() >= PULL_BUDGET {
                return;
            }
        }
    }

    /// 把一批事件写进 connector 的目标流，分批写；有一批失败就是这次拉取失败。
    async fn write(&self, c: &Connector, events: Vec<RawEvent>) -> Result<()> {
        let stream = target_stream(c);
        let mut events = events;

        while !events.is_empty() {
            let rest = events.split_off(events.len().min(MAX_EVENTS_PER_WRITE));
            let chunk = std::mem::replace(&mut events, rest);

            // A pull has no request to measure: its events are measured themselves.
            let receipt = self.usage_meter.as_ref().map(|_| {
                let bytes = chunk
                    .iter()
                    .filter_map(|event| event_bytes(event).ok())
                    .sum::<u64>();

                IntakeReceipt::of_events(
                    IntakeCredential::connector(&c.id),
                    StreamType::LOGS,
                    &stream,
                    bytes,
                    &chunk,
                )
            });

            let batch = IntakeBatch {
                batch_id: Id::new(),
                org_id: c.org_id.clone(),
                stream: stream.clone(),
                stream_type: StreamType::LOGS,
                events: chunk,
                received_at: TimestampMicros::now(),
            };

            let accepted = self.intake.intake(batch).await?.accepted as u64;

            if let (Some(meter), Some(receipt)) = (&self.usage_meter, receipt) {
                meter.record(&c.org_id, receipt, accepted);
            }
        }

        Ok(())
    }

    async fn save_failure(&self, c: &Connector, state: &mut ReadState, reason: &str) {
        state.failed(reason, TimestampMicros::now());

        if let Err(e) = self
            .repo
            .save_read_state(&c.id, None, &state.to_value())
            .await
        {
            tracing::warn!(error = %e, connector_id = %c.id.0, "connector read state not saved");
        }
    }
}

/// `connectors.read_state`：读到哪了（各来源自己的格式）、最近一次失败和最近一次跳过了什么。
#[derive(Debug, Clone, Default, PartialEq)]
struct ReadState {
    position: Value,
    last_error: Option<Note>,
    last_warning: Option<Note>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Note {
    at_time: i64,
    text: String,
}

impl ReadState {
    fn of(value: &Value) -> Self {
        let note = |key: &str| {
            value
                .get(key)
                .cloned()
                .and_then(|note| serde_json::from_value(note).ok())
        };

        Self {
            position: value.get("position").cloned().unwrap_or(Value::Null),
            last_error: note("lastError"),
            last_warning: note("lastWarning"),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "position": self.position,
            "lastError": self.last_error,
            "lastWarning": self.last_warning,
        })
    }

    fn succeeded(&mut self, position: Option<Value>, warning: Option<String>, at: TimestampMicros) {
        if let Some(position) = position {
            self.position = position;
        }

        self.last_error = None;

        if let Some(warning) = warning {
            self.last_warning = Some(Note::new(&warning, at));
        }
    }

    fn failed(&mut self, reason: &str, at: TimestampMicros) {
        self.last_error = Some(Note::new(reason, at));
    }
}

impl Note {
    fn new(text: &str, at: TimestampMicros) -> Self {
        Self {
            at_time: at.0,
            text: text.chars().take(MAX_ERROR_CHARS).collect(),
        }
    }
}

/// 是否由 runner 周期拉取（push / egress 类不是）。
fn is_pull(c: &Connector) -> bool {
    matches!(
        c.kind.parse::<ConnectorKind>(),
        Ok(ConnectorKind::AwsCloudwatchLogs | ConnectorKind::KafkaSource | ConnectorKind::S3Source)
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
    let t = cfg_str(c, "targetStream");

    if t.trim().is_empty() {
        "default".to_string()
    } else {
        t
    }
}

fn build_pull(c: &Connector) -> Box<dyn PullConnector> {
    match c.kind.parse::<ConnectorKind>() {
        Ok(ConnectorKind::KafkaSource) => Box::new(KafkaSource::new(c)),
        Ok(ConnectorKind::S3Source) => Box::new(S3Source::new(c)),
        _ => Box::new(cloudwatch(c, CredentialSource::Kept)),
    }
}

/// The CloudWatch Logs puller `c` describes. A tick keeps the credentials of the last one while
/// the settings stay the same; a connection test asks for new ones.
pub fn cloudwatch(c: &Connector, source: CredentialSource) -> CloudWatchLogsConnector {
    let region = cfg_str(c, "region");

    let credentials = AwsAuth::parse(&c.config_json)
        .and_then(|auth| credential_provider(c, &auth, &region, source))
        .map_err(|e| e.to_string());

    CloudWatchLogsConnector {
        log_group: cfg_str(c, "logGroup"),
        target_stream: target_stream(c),
        region,
        credentials,
    }
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
        domain::{
            run_claim::MemoryRunClaims,
            stream::{Schema, StreamDefinition, StreamRepository},
        },
        infra::intake_sink::MemoryIntakeSink,
        shared::Error,
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
        async fn save_read_state(
            &self,
            _id: &Id,
            succeeded_at: Option<TimestampMicros>,
            read_state: &Value,
        ) -> Result<()> {
            let mut fresh = self.fresh.lock().unwrap();
            fresh.read_state = read_state.clone();

            if let Some(at) = succeeded_at {
                fresh.last_run_at = Some(at);
                self.touches.fetch_add(1, Ordering::SeqCst);
            }

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

    /// 第一次写入时才建流，和真实的写入路径一样。
    struct OnDemandStreams;

    #[async_trait]
    impl StreamRepository for OnDemandStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            Ok(def)
        }
        async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
            Ok(())
        }
        async fn get(
            &self,
            _org_id: &Id,
            _name: &str,
            _stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            Err(Error::not_found("stream"))
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(Vec::new())
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    /// 记录每次拉取使用的窗口起点，不产生事件。
    #[derive(Default)]
    struct Pulls(Mutex<Vec<Option<i64>>>);

    struct RecordingPull(Arc<Pulls>);

    #[async_trait]
    impl PullConnector for RecordingPull {
        async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
            self.0.0.lock().unwrap().push(ctx.last_run_at_micros);
            Ok(PullBatch::default())
        }
    }

    /// 按给定的结果依次回答，记下每次看到的位点。
    struct Scripted {
        answers: Mutex<Vec<Result<PullBatch>>>,
        positions: Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl PullConnector for Arc<Scripted> {
        async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
            self.positions.lock().unwrap().push(ctx.position.clone());
            self.answers.lock().unwrap().remove(0)
        }
    }

    fn event() -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(1),
            fields: serde_json::Map::from_iter([("message".to_string(), json!("hello"))]),
        }
    }

    fn scripted(
        repo: Arc<Cursor>,
        answers: Vec<Result<PullBatch>>,
        sink: Arc<MemoryIntakeSink>,
    ) -> (ConnectorRunner, Arc<Scripted>) {
        let intake = Arc::new(IntakeService::new(sink, Arc::new(OnDemandStreams)));
        let script = Arc::new(Scripted {
            answers: Mutex::new(answers),
            positions: Mutex::new(Vec::new()),
        });
        let mut runner = ConnectorRunner::new(repo, intake, 30);
        let pull = script.clone();

        runner.pull_factory =
            Arc::new(move |_: &Connector| -> Box<dyn PullConnector> { Box::new(pull.clone()) });

        (runner, script)
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
            read_state: serde_json::Value::Null,
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
    async fn a_position_is_kept_once_its_events_are_written_and_a_pull_that_has_more_goes_on() {
        let repo = Arc::new(Cursor {
            snapshot: connector(None),
            fresh: Mutex::new(connector(None)),
            touches: AtomicUsize::new(0),
        });
        let sink = Arc::new(MemoryIntakeSink::default());

        let (runner, script) = scripted(
            repo.clone(),
            vec![
                Ok(PullBatch {
                    events: vec![event()],
                    position: Some(json!({ "offset": 1 })),
                    more: true,
                    warning: None,
                }),
                Ok(PullBatch {
                    events: vec![event()],
                    position: Some(json!({ "offset": 2 })),
                    more: false,
                    warning: Some("skipped a big object".into()),
                }),
            ],
            sink.clone(),
        );

        assert_eq!(runner.tick_once().await, 1);

        assert_eq!(
            *script.positions.lock().unwrap(),
            [Value::Null, json!({ "offset": 1 })]
        );

        let state = repo.fresh.lock().unwrap().read_state.clone();

        assert_eq!(state["position"], json!({ "offset": 2 }));
        assert_eq!(state["lastError"], Value::Null);
        assert_eq!(state["lastWarning"]["text"], "skipped a big object");
        assert_eq!(sink.drain().len(), 2);
    }

    #[tokio::test]
    async fn a_failed_pull_keeps_its_position_says_why_and_the_next_one_clears_it() {
        let mut held = connector(None);
        held.read_state = json!({ "position": { "offset": 7 } });

        let repo = Arc::new(Cursor {
            snapshot: held.clone(),
            fresh: Mutex::new(held),
            touches: AtomicUsize::new(0),
        });

        let (runner, script) = scripted(
            repo.clone(),
            vec![
                Err(crate::shared::Error::unavailable("the broker is down")),
                Ok(PullBatch {
                    position: Some(json!({ "offset": 8 })),
                    ..PullBatch::default()
                }),
            ],
            Arc::new(MemoryIntakeSink::default()),
        );

        runner.tick_once().await;

        let failed = repo.fresh.lock().unwrap().read_state.clone();

        assert_eq!(failed["position"], json!({ "offset": 7 }));
        assert!(
            failed["lastError"]["text"]
                .as_str()
                .unwrap()
                .contains("the broker is down")
        );
        assert_eq!(repo.touches.load(Ordering::SeqCst), 0);

        runner.tick_once().await;

        let recovered = repo.fresh.lock().unwrap().read_state.clone();

        assert_eq!(recovered["position"], json!({ "offset": 8 }));
        assert_eq!(recovered["lastError"], Value::Null);
        assert_eq!(
            script.positions.lock().unwrap().last().unwrap(),
            &json!({ "offset": 7 })
        );
    }

    #[test]
    fn a_read_state_reads_back_as_it_was_written() {
        let mut state = ReadState::default();
        state.succeeded(Some(json!({ "a": 1 })), None, TimestampMicros(5));
        state.failed(&"x".repeat(MAX_ERROR_CHARS + 9), TimestampMicros(6));

        let back = ReadState::of(&state.to_value());

        assert_eq!(back, state);
        assert_eq!(
            back.last_error.unwrap().text.chars().count(),
            MAX_ERROR_CHARS
        );
        assert_eq!(ReadState::of(&json!({})), ReadState::default());
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
