// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled-pipeline 执行编排（pipeline-runs-and-backfill）。
//!
//! 端到端链路：**读源 stream → 逐步处理 → 写目标 stream（标准 intake）→ connector egress → 死信**。
//! 「读源」一环由调用方（持 [`crate::app::query::QueryService`]，app 层）完成后，把各源读到的行
//! 交给 [`PipelineOutput::run`]；本模块承担「处理 + 写目标 + egress + 死信」，因此可脱离 HTTP /
//! 调度时序单测。定时运行（[`BootstrapPipelineExecutor`]）与 backfill worker（[`super::search_jobs`]）
//! 共用这一核心。
//!
//! 与 [`crate::infra::pipeline::exec`] 的分工：后者是纯计算（解析 steps + 串行 VRL），本模块
//! 把它接到 [`IntakeService`] 与 connector egress 上。写目标走 intake 而不是直接写 WAL，是因为
//! 目标流不一定已经存在、也不一定已有输出里的全部字段：建流、schema 演化与类型校验都在 intake 里。
//!
//! 每条读到的事件最后落在三者之一：写出（去了它该去的每个地方）、丢弃（步骤 `abort` 或去重）、
//! 失败（步骤出错、路由回源、被某个目标流拒收或没能投递到 connector）。失败的事件在配置了死信
//! stream 时连同原因写进去。

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::{
    app::{intake::IntakeService, query::QueryService},
    domain::{
        pipeline::{realtime::is_realtime, scheduled::ScheduledConfig},
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    infra::{
        connectors::{ConnectorDispatcher, ConnectorRepository},
        pipeline::{
            PipelineExecutor, ScheduledPipeline, ScheduledPipelineRunner,
            exec::{
                parse_signal_type,
                processing::{FailedEvent, ProcessedEvent, SourceEvent, process_rows},
            },
            scheduled::{
                RunCounts, RunReport,
                state::{SourceResult, Window},
            },
        },
        runtime::VrlRuntime,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod dead_letter;
mod delivery;
mod streams;

use self::dead_letter::DeadLetter;

/// The key under which a source's events that failed a step count as handled in a retried window.
const TRANSFORM_FAILURES: &str = "transform:failures";

/// What was read from one source, in the order read.
#[derive(Debug, Clone, Default)]
pub struct SourceRows {
    pub source: String,
    pub rows: Vec<Value>,
    /// Destinations that already took these rows in an earlier attempt at the same window.
    pub done: BTreeSet<String>,
}

/// The pipeline a batch of rows goes through.
pub struct PipelineSpec<'a> {
    pub org_id: &'a Id,
    pub name: &'a str,
    pub stream_type: StreamType,
    pub function_steps: &'a Value,
    pub config: &'a ScheduledConfig,
}

/// What became of one source's rows.
#[derive(Debug, Default)]
pub struct SourceWrite {
    /// Destinations that took them, in this attempt or an earlier one.
    pub done: BTreeSet<String>,
    /// A write of them that failed.
    pub failure: Option<WriteFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFailure {
    pub message: String,
    /// Writing the same again would fail the same way.
    pub permanent: bool,
}

/// What a run has found out so far about the events it processed and the sources they came from.
struct Ledger {
    /// The sources, in the order their rows were given.
    names: Vec<String>,
    sources: Vec<SourceWrite>,
    /// Per processed event: something it was meant for did not take it.
    failed: Vec<bool>,
    /// Kept only when the pipeline has a dead letter stream.
    dead_letters: Option<Vec<DeadLetter>>,
    errors: Vec<String>,
}

impl Ledger {
    fn dead_letter(&mut self, letter: impl FnOnce() -> DeadLetter) {
        if let Some(letters) = &mut self.dead_letters {
            letters.push(letter());
        }
    }
}

/// 一次执行的产出：统计 + 变换后的事件（供 monitor NDJSON 复用）。
#[derive(Debug)]
pub struct PipelineRunOutcome {
    pub counts: RunCounts,
    /// The events that came through the steps, as written.
    pub transformed: Vec<Value>,
    /// 非致命问题（事件处理失败、目标流拒收、connector 投递失败、死信写不进去），同类合并成一行。
    pub errors: Vec<String>,
    /// Per source, in the order given.
    pub sources: Vec<SourceWrite>,
    /// A stream that took none of the events it was given while no dead letter stream kept them:
    /// the run wrote nothing there and must not be reported as a success.
    pub failure: Option<String>,
}

/// `QueryResult` 的列式行（`Vec<Vec<Value>>` + 列名）→ 每行一个 JSON 对象（列名→值）。
pub fn rows_to_objects(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::with_capacity(result.columns.len());

            for (i, col) in result.columns.iter().enumerate() {
                obj.insert(col.clone(), row.get(i).cloned().unwrap_or(Value::Null));
            }

            Value::Object(obj)
        })
        .collect()
}

/// Where a pipeline's events go: streams through the standard intake, connectors through egress.
#[derive(Clone)]
pub struct PipelineOutput {
    pub intake: Arc<IntakeService>,
    pub connectors: Arc<dyn ConnectorRepository>,
    pub dispatcher: Arc<dyn ConnectorDispatcher>,
    /// The wait before the first redelivery to a connector; it doubles for each one after.
    pub delivery_backoff: Duration,
}

impl PipelineOutput {
    pub fn new(
        intake: Arc<IntakeService>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
    ) -> Self {
        Self {
            intake,
            connectors,
            dispatcher,
            delivery_backoff: Duration::from_secs(1),
        }
    }

    /// Runs `pipeline` on what was read from its sources and sends the events on.
    ///
    /// 失败语义：任一步 VRL **编译**失败 = pipeline 配置错误 → 整批失败（返 `Err`）。其余问题都按
    /// 事件或按目的地记：单条事件失败、被拒收、投递不成功进 `errors`（和死信）；写某个目标流本身
    /// 出错（存储不可用等）记在涉及的源上，留给调用方决定是否重试。
    #[tracing::instrument(
        name = "pipeline.run",
        skip_all,
        fields(otel.kind = "internal", molesignal.stream.type = ?pipeline.stream_type)
    )]
    pub async fn run(
        &self,
        vrl: &VrlRuntime,
        pipeline: &PipelineSpec<'_>,
        input: Vec<SourceRows>,
    ) -> Result<PipelineRunOutcome> {
        if is_realtime(pipeline.function_steps) {
            return Err(Error::invalid(
                "realtime pipelines cannot run as scheduled jobs",
            ));
        }

        let config = pipeline.config;

        let mut ledger = Ledger {
            names: input.iter().map(|rows| rows.source.clone()).collect(),
            sources: input
                .iter()
                .map(|rows| SourceWrite {
                    done: rows.done.clone(),
                    failure: None,
                })
                .collect(),
            failed: Vec::new(),
            dead_letters: config.dead_letter_stream.as_ref().map(|_| Vec::new()),
            errors: Vec::new(),
        };

        let events: Vec<SourceEvent> = input
            .into_iter()
            .enumerate()
            .flat_map(|(source, rows)| {
                rows.rows
                    .into_iter()
                    .map(move |event| SourceEvent { source, event })
            })
            .collect();

        let scanned = events.len();

        let processed = process_rows(
            vrl,
            pipeline.function_steps,
            &config.sinks[0],
            &ledger.names,
            events,
            ledger.dead_letters.is_some(),
        )?;

        let mut dropped = processed.dropped;
        let mut failures = processed.failed;
        let mut rows = Vec::with_capacity(processed.rows.len());

        // Routing by a field can name a stream the pipeline reads only at run time (a fixed one
        // is refused when saved); writing it there would feed the pipeline its own output.
        for row in processed.rows {
            if config.sources.contains(&row.destination) {
                failures.push(FailedEvent {
                    source: row.source,
                    reason: format!(
                        "routed to `{}`, a stream the pipeline reads",
                        row.destination
                    ),
                    event: row.event,
                });
            } else {
                rows.push(row);
            }
        }

        if let Some(field) = &config.dedupe_field {
            let before = rows.len();
            dedupe(&mut rows, field);
            dropped += before - rows.len();
        }

        ledger.failed = vec![false; rows.len()];
        ledger.errors = summaries(
            failures.iter().map(|failure| failure.reason.as_str()),
            |count, reason| format!("{count} event(s) failed: {reason}"),
        );

        // Events that failed a step are dead letters once per window: a retried window that
        // already wrote them leaves them out.
        for failure in &failures {
            if !ledger.sources[failure.source]
                .done
                .contains(TRANSFORM_FAILURES)
            {
                let source = ledger.names[failure.source].clone();
                ledger.dead_letter(|| {
                    DeadLetter::transform(source, failure.event.clone(), &failure.reason)
                });
            }
        }

        let broken = streams::write(self, pipeline, &rows, &mut ledger).await;
        delivery::deliver(self, pipeline, &rows, &mut ledger).await;

        let letters = ledger.dead_letters.take().unwrap_or_default();
        let dead_lettered = dead_letter::write(self, pipeline, letters, &mut ledger.errors).await;
        let had_failures: BTreeSet<usize> = failures.iter().map(|failure| failure.source).collect();

        // A source's failed events are handled once they reached the dead letter stream, or when
        // there is none for them to reach.
        for (index, source) in ledger.sources.iter_mut().enumerate() {
            if dead_lettered || !had_failures.contains(&index) {
                source.done.insert(TRANSFORM_FAILURES.to_owned());
            }
        }

        let failed_rows = ledger.failed.iter().filter(|failed| **failed).count();

        let counts = RunCounts {
            scanned: count(scanned),
            written: count(rows.len() - failed_rows),
            dropped: count(dropped),
            failed: count(failures.len() + failed_rows),
        };

        let failure =
            (config.dead_letter_stream.is_none() && !broken.is_empty()).then(|| broken.join("; "));

        Ok(PipelineRunOutcome {
            counts,
            transformed: rows.into_iter().map(|row| row.event).collect(),
            errors: ledger.errors,
            sources: ledger.sources,
            failure,
        })
    }
}

/// Keeps the first event of every value of `field`; events without it are all kept. A field is
/// looked up as a literal key first and then as a dotted path, as routing does.
fn dedupe(rows: &mut Vec<ProcessedEvent>, field: &str) {
    let mut seen = BTreeSet::new();

    rows.retain(|row| {
        let value = row
            .event
            .get(field)
            .or_else(|| field.split('.').try_fold(&row.event, |v, key| v.get(key)));

        match value.filter(|value| !value.is_null()) {
            Some(value) => seen.insert(value.to_string()),
            None => true,
        }
    });
}

/// 同类问题按原因归并后，最多逐条列出这么多种原因。
const MAX_REASONS: usize = 5;

/// 同一种原因只报一条并带上条数：与已有字段类型冲突时通常整批事件的原因都一样，逐条列出会
/// 灌出成千上万行。`line` 把条数和原因写成一句话；超出的原因合并成最后一句。
fn summaries<'a>(
    reasons: impl Iterator<Item = &'a str>,
    line: impl Fn(usize, &str) -> String,
) -> Vec<String> {
    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();

    for reason in reasons {
        *by_reason.entry(reason).or_default() += 1;
    }

    let mut reasons: Vec<_> = by_reason.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let rest: Vec<_> = reasons.split_off(reasons.len().min(MAX_REASONS));

    let mut lines: Vec<String> = reasons
        .into_iter()
        .map(|(reason, count)| line(count, reason))
        .collect();

    if !rest.is_empty() {
        let count = rest.iter().map(|(_, count)| count).sum();
        lines.push(line(count, &format!("{} more reason(s)", rest.len())));
    }

    lines
}

fn count(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// [`PipelineExecutor`] 的 bootstrap 实装：按窗口读源（`QueryService`，每个源 `SELECT *`）→
/// 处理 + 写目标 + egress + 死信（[`PipelineOutput::run`]）。
pub struct BootstrapPipelineExecutor {
    query: Arc<QueryService>,
    output: PipelineOutput,
}

impl BootstrapPipelineExecutor {
    pub fn new(
        query: Arc<QueryService>,
        intake: Arc<IntakeService>,
        connectors: Arc<dyn ConnectorRepository>,
        dispatcher: Arc<dyn ConnectorDispatcher>,
    ) -> Self {
        Self {
            query,
            output: PipelineOutput::new(intake, connectors, dispatcher),
        }
    }

    /// One source's window. A stream that has never been written has nothing to read.
    async fn read(
        &self,
        pipeline: &ScheduledPipeline,
        window: &Window,
        stream_type: StreamType,
    ) -> Result<Vec<Value>> {
        let req = QueryRequest {
            org_id: pipeline.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: format!("SELECT * FROM {}", window.source),
            time_range: TimeRange::new(TimestampMicros(window.start), TimestampMicros(window.end)),
            stream: Some(StreamHint {
                name: window.source.clone(),
                stream_type,
            }),
            limit: None,
            federation_clusters: Vec::new(),
        };

        match self.query.run_raw(req).await {
            Ok(result) => Ok(rows_to_objects(&result)),
            Err(Error::NotFound(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

/// Whether trying the same again would fail the same way: the request or the pipeline is wrong,
/// not the storage it reaches.
pub(crate) fn is_configuration_error(error: &Error) -> bool {
    matches!(
        error,
        Error::InvalidArgument(_)
            | Error::Validation { .. }
            | Error::Forbidden(_)
            | Error::Unauthorized(_)
    )
}

#[async_trait]
impl PipelineExecutor for BootstrapPipelineExecutor {
    #[tracing::instrument(
        name = "worker.scheduled_pipeline",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "scheduled_pipeline")
    )]
    async fn execute(&self, pipeline: &ScheduledPipeline, windows: &[Window]) -> Result<RunReport> {
        let config = ScheduledConfig::parse(
            &pipeline.function_steps,
            &pipeline.source_stream,
            &pipeline.target_stream,
        )?;
        let stream_type = parse_signal_type(&pipeline.function_steps);
        let mut results = BTreeMap::new();
        let mut input = Vec::with_capacity(windows.len());

        for window in windows {
            match self.read(pipeline, window, stream_type).await {
                Ok(rows) => input.push(SourceRows {
                    source: window.source.clone(),
                    rows,
                    done: window.done.clone(),
                }),
                Err(e) => {
                    let failed = SourceResult::Failed {
                        done: window.done.clone(),
                        permanent: is_configuration_error(&e),
                        error: format!("read: {e}"),
                    };

                    results.insert(window.source.clone(), failed);
                }
            }
        }

        let names: Vec<String> = input.iter().map(|rows| rows.source.clone()).collect();

        let spec = PipelineSpec {
            org_id: &pipeline.org_id,
            name: &pipeline.name,
            stream_type,
            function_steps: &pipeline.function_steps,
            config: &config,
        };

        let outcome = self.output.run(&VrlRuntime::new(), &spec, input).await?;

        for (source, write) in names.into_iter().zip(outcome.sources) {
            let result = match write.failure {
                None => SourceResult::Done,
                Some(failure) => SourceResult::Failed {
                    done: write.done,
                    permanent: failure.permanent,
                    error: failure.message,
                },
            };

            results.insert(source, result);
        }

        if !outcome.errors.is_empty() {
            tracing::warn!(
                pipeline_id = %pipeline.id.0,
                scanned = outcome.counts.scanned,
                written = outcome.counts.written,
                errors = ?outcome.errors,
                "scheduled pipeline completed with non-fatal errors"
            );
        }

        Ok(RunReport {
            counts: outcome.counts,
            sources: results,
            warnings: outcome.errors,
            failure: outcome.failure,
        })
    }
}

/// 起 cron 轮询循环：每 `poll_secs` 调一次 `runner.tick_once`（runner 内按各 pipeline 的
/// cron、last_run_at 与读取位点判定是否真的 fire）。调用方据角色（alert / standalone）决定是否 spawn。
pub fn spawn_runner(runner: Arc<ScheduledPipelineRunner>, poll_secs: u64) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(poll_secs.max(1)));
        ticker.tick().await;

        loop {
            ticker.tick().await;

            if let Err(e) = runner.tick_once().await {
                tracing::warn!(error = %e, "scheduled_pipelines tick failed");
            }
        }
    })
}

#[cfg(test)]
mod tests;
