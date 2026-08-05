// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace-ID affinity 与有界、确定性的分布式 tail-sampler 核心。
//!
//! 本模块不做网络 I/O。producer transport 只需把 [`TraceCandidate`] 发送给
//! [`rendezvous_owner`] 选出的节点；owner 在单个同步临界区内去重、绑定策略版本、
//! 聚合并决策。所有入口均为非阻塞内存操作。

use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::shared::{
    time::TimestampMicros,
    trace_metrics,
    trace_normalization::{
        CanonicalSpan, PartialReason, SamplingReason, TraceLimits, sanitize_and_limit_span,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceRuntimePolicy {
    pub version: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ratio")]
    pub normal_sample_ratio: f64,
    #[serde(default = "default_decision_window_ms")]
    pub decision_window_ms: u64,
    #[serde(default = "default_root_grace_ms")]
    pub root_grace_ms: u64,
    #[serde(default = "default_decision_cache_ms")]
    pub decision_cache_ms: u64,
    #[serde(default = "default_max_traces")]
    pub max_traces: usize,
    #[serde(default = "default_memory_bytes")]
    pub memory_bytes: usize,
    #[serde(default = "default_max_spans")]
    pub max_spans_per_trace: usize,
    #[serde(default)]
    pub slow_thresholds_ms: SlowThresholds,
    #[serde(default)]
    pub route_slow_thresholds_ms: BTreeMap<String, u64>,
    #[serde(default)]
    pub rules: Vec<SamplingRule>,
}

impl Default for TraceRuntimePolicy {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            normal_sample_ratio: default_ratio(),
            decision_window_ms: default_decision_window_ms(),
            root_grace_ms: default_root_grace_ms(),
            decision_cache_ms: default_decision_cache_ms(),
            max_traces: default_max_traces(),
            memory_bytes: default_memory_bytes(),
            max_spans_per_trace: default_max_spans(),
            slow_thresholds_ms: SlowThresholds::default(),
            route_slow_thresholds_ms: BTreeMap::new(),
            rules: Vec::new(),
        }
    }
}

impl TraceRuntimePolicy {
    pub fn validate(&self) -> Result<(), String> {
        if !self.normal_sample_ratio.is_finite() || !(0.0..=1.0).contains(&self.normal_sample_ratio)
        {
            return Err("normal_sample_ratio must be between 0 and 1".into());
        }
        if !(5_000..=120_000).contains(&self.decision_window_ms) {
            return Err("decision_window_ms must be between 5000 and 120000".into());
        }
        if self.root_grace_ms == 0
            || self.decision_cache_ms < self.decision_window_ms
            || self.max_traces == 0
            || self.memory_bytes == 0
            || self.max_spans_per_trace == 0
        {
            return Err("Trace policy limits and timeouts must be greater than zero".into());
        }
        self.slow_thresholds_ms.validate()?;
        for (route, threshold) in &self.route_slow_thresholds_ms {
            if route.is_empty() || *threshold == 0 {
                return Err("route slow-threshold entries must be non-empty/non-zero".into());
            }
        }
        for rule in &self.rules {
            rule.validate()?;
        }
        Ok(())
    }
}

impl From<&crate::config::TraceSettings> for TraceRuntimePolicy {
    fn from(settings: &crate::config::TraceSettings) -> Self {
        Self {
            version: 1,
            enabled: settings.effective_enabled(),
            normal_sample_ratio: settings.effective_normal_sample_ratio(),
            decision_window_ms: settings.decision_window_secs.saturating_mul(1_000),
            root_grace_ms: settings.root_grace_millis,
            decision_cache_ms: settings.decision_cache_secs.saturating_mul(1_000),
            max_traces: settings.tail_max_traces,
            memory_bytes: settings.tail_memory_bytes,
            max_spans_per_trace: settings.max_spans_per_trace,
            slow_thresholds_ms: SlowThresholds {
                http: settings.slow_thresholds.http_ms,
                query: settings.slow_thresholds.query_ms,
                batch_ingest: settings.slow_thresholds.batch_ingest_ms,
                database: settings.slow_thresholds.database_ms,
                object_store: settings.slow_thresholds.object_store_ms,
                external: settings.slow_thresholds.external_ms,
                background: settings.slow_thresholds.background_ms,
                other: settings.slow_thresholds.http_ms,
            },
            route_slow_thresholds_ms: BTreeMap::new(),
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlowThresholds {
    #[serde(default = "default_http_ms")]
    pub http: u64,
    #[serde(default = "default_query_ms")]
    pub query: u64,
    #[serde(default = "default_ingest_ms")]
    pub batch_ingest: u64,
    #[serde(default = "default_database_ms")]
    pub database: u64,
    #[serde(default = "default_object_store_ms")]
    pub object_store: u64,
    #[serde(default = "default_external_ms")]
    pub external: u64,
    #[serde(default = "default_background_ms")]
    pub background: u64,
    #[serde(default = "default_http_ms")]
    pub other: u64,
}

impl Default for SlowThresholds {
    fn default() -> Self {
        Self {
            http: default_http_ms(),
            query: default_query_ms(),
            batch_ingest: default_ingest_ms(),
            database: default_database_ms(),
            object_store: default_object_store_ms(),
            external: default_external_ms(),
            background: default_background_ms(),
            other: default_http_ms(),
        }
    }
}

impl SlowThresholds {
    fn validate(&self) -> Result<(), String> {
        if [
            self.http,
            self.query,
            self.batch_ingest,
            self.database,
            self.object_store,
            self.external,
            self.background,
            self.other,
        ]
        .contains(&0)
        {
            return Err("slow thresholds must be greater than zero".into());
        }
        Ok(())
    }

    fn for_span(&self, span: &CanonicalSpan) -> u64 {
        match span_category(span) {
            SpanCategory::Http | SpanCategory::Rpc => self.http,
            SpanCategory::Query => self.query,
            SpanCategory::BatchIngest => self.batch_ingest,
            SpanCategory::Database => self.database,
            SpanCategory::ObjectStore => self.object_store,
            SpanCategory::External => self.external,
            SpanCategory::Background => self.background,
            SpanCategory::Other => self.other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplingRule {
    pub name: String,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub minimum_duration_ms: Option<u64>,
    #[serde(default)]
    pub require_error: bool,
    pub action: SamplingRuleAction,
}

impl SamplingRule {
    fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("sampling rule name must not be empty".into());
        }
        if matches!(self.minimum_duration_ms, Some(0)) {
            return Err("sampling rule duration must be non-zero".into());
        }
        if let SamplingRuleAction::Ratio(ratio) = self.action
            && (!ratio.is_finite() || !(0.0..=1.0).contains(&ratio))
        {
            return Err("sampling rule ratio must be between 0 and 1".into());
        }
        Ok(())
    }

    fn matches(&self, span: &CanonicalSpan) -> bool {
        if let Some(service_name) = &self.service_name
            && span
                .resource
                .attributes
                .get("service.name")
                .and_then(Value::as_str)
                != Some(service_name)
        {
            return false;
        }
        if let Some(route) = &self.route
            && span.attributes.get("http.route").and_then(Value::as_str) != Some(route)
        {
            return false;
        }
        if let Some(operation) = &self.operation
            && span.name != *operation
        {
            return false;
        }
        if let Some(duration) = self.minimum_duration_ms
            && span.duration_ns < duration.saturating_mul(1_000_000)
        {
            return false;
        }
        !self.require_error || span_is_error(span)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "ratio")]
pub enum SamplingRuleAction {
    Keep,
    Drop,
    Ratio(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceKeep {
    None,
    TrustedInternal,
    DebugToken,
}

#[derive(Debug, Clone)]
pub struct TraceCandidate {
    pub org_id: String,
    /// `None` 表示 MoleSignal 自身 Trace，落 `_sys/_molesignal`；
    /// `Some` 表示公共 OTLP 的租户 stream。
    pub stream: Option<String>,
    pub span: CanonicalSpan,
    pub force_keep: ForceKeep,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TraceKey {
    org_id: String,
    trace_id: String,
}

#[derive(Debug, Clone)]
pub struct DecidedTrace {
    pub org_id: String,
    pub stream: Option<String>,
    pub trace_id: String,
    pub policy_version: u64,
    pub kept: bool,
    pub reason: SamplingReason,
    pub spans: Vec<CanonicalSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateDisposition {
    Accepted,
    IdenticalDuplicate,
    ConflictingDuplicate,
    LateKept,
    LateDropped,
}

#[derive(Debug, Default)]
pub struct SamplerOutput {
    pub disposition: Option<CandidateDisposition>,
    pub decided: Vec<DecidedTrace>,
}

#[derive(Debug)]
struct PendingTrace {
    policy: Arc<TraceRuntimePolicy>,
    stream: Option<String>,
    spans: Vec<CanonicalSpan>,
    digests: HashMap<String, [u8; 32]>,
    first_seen_ms: u64,
    root_ended_ms: Option<u64>,
    estimated_bytes: usize,
    force_keep: ForceKeep,
}

impl PendingTrace {
    fn saw_error(&self) -> bool {
        self.spans.iter().any(span_is_error)
    }

    fn saw_slow(&self) -> bool {
        self.spans
            .iter()
            .any(|span| span_is_slow(span, &self.policy))
    }

    fn priority(&self) -> u8 {
        if self.force_keep != ForceKeep::None || self.saw_error() || self.saw_slow() {
            1
        } else {
            0
        }
    }
}

#[derive(Debug)]
struct CachedDecision {
    kept: bool,
    stream: Option<String>,
    reason: SamplingReason,
    policy_version: u64,
    expires_at_ms: u64,
    digests: HashMap<String, [u8; 32]>,
}

#[derive(Default)]
struct SamplerState {
    traces: HashMap<TraceKey, PendingTrace>,
    decisions: HashMap<TraceKey, CachedDecision>,
    estimated_bytes: usize,
}

#[derive(Debug, Default)]
pub struct TailSamplerMetrics {
    pub accepted: AtomicU64,
    pub kept: AtomicU64,
    pub dropped: AtomicU64,
    pub duplicates: AtomicU64,
    pub conflicts: AtomicU64,
    pub late_kept: AtomicU64,
    pub late_dropped: AtomicU64,
    pub pressure_decisions: AtomicU64,
    pub unresolved_loss: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TailSamplerMetricSnapshot {
    pub accepted: u64,
    pub kept: u64,
    pub dropped: u64,
    pub duplicates: u64,
    pub conflicts: u64,
    pub late_kept: u64,
    pub late_dropped: u64,
    pub pressure_decisions: u64,
    pub unresolved_loss: u64,
    pub pending_traces: usize,
    pub pending_bytes: usize,
    pub capacity_traces: usize,
    pub capacity_bytes: usize,
}

pub struct TailSampler {
    deployment_force_disabled: bool,
    policy: RwLock<Arc<TraceRuntimePolicy>>,
    state: Mutex<SamplerState>,
    metrics: TailSamplerMetrics,
    span_limits: TraceLimits,
}

impl TailSampler {
    pub fn new(
        policy: TraceRuntimePolicy,
        deployment_force_disabled: bool,
        span_limits: TraceLimits,
    ) -> Result<Self, String> {
        policy.validate()?;
        Ok(Self {
            deployment_force_disabled,
            policy: RwLock::new(Arc::new(policy)),
            state: Mutex::new(SamplerState::default()),
            metrics: TailSamplerMetrics::default(),
            span_limits,
        })
    }

    /// 原子发布新策略；已存在的 Trace 继续持有其创建时 Arc。
    pub fn publish_policy(&self, policy: TraceRuntimePolicy) -> Result<(), String> {
        policy.validate()?;
        *self.policy.write() = Arc::new(policy);
        Ok(())
    }

    pub fn active_policy(&self) -> Arc<TraceRuntimePolicy> {
        self.policy.read().clone()
    }

    pub fn deployment_force_disabled(&self) -> bool {
        self.deployment_force_disabled
    }

    pub fn effective_enabled(&self) -> bool {
        !self.deployment_force_disabled && self.policy.read().enabled
    }

    pub fn accept(&self, candidate: TraceCandidate) -> SamplerOutput {
        self.accept_at(candidate, now_millis())
    }

    pub fn accept_at(&self, mut candidate: TraceCandidate, now_ms: u64) -> SamplerOutput {
        sanitize_and_limit_span(&mut candidate.span, self.span_limits);
        trace_metrics::record_spans("candidate", "accepted", 1);
        if candidate.span.partial {
            trace_metrics::record_spans("candidate", "partial", 1);
        }
        let key = TraceKey {
            org_id: candidate.org_id.clone(),
            trace_id: candidate.span.trace_id.clone(),
        };
        let span_id = candidate.span.span_id.clone();
        let digest = candidate.span.content_digest();
        let size = estimated_span_bytes(&candidate.span);
        let mut state = self.state.lock();
        purge_expired_decisions(&mut state, now_ms);
        let mut output = SamplerOutput::default();

        if let Some(cached) = state.decisions.get_mut(&key) {
            output.disposition = Some(match cached.digests.get(&span_id) {
                Some(previous) if previous == &digest => {
                    self.metrics.duplicates.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "duplicate", 1);
                    CandidateDisposition::IdenticalDuplicate
                }
                Some(_) => {
                    self.metrics.conflicts.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "conflict", 1);
                    CandidateDisposition::ConflictingDuplicate
                }
                None if cached.kept => {
                    cached.digests.insert(span_id, digest);
                    candidate.span.late = true;
                    candidate.span.sampling_reason = cached.reason;
                    self.metrics.late_kept.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "late", 1);
                    output.decided.push(DecidedTrace {
                        org_id: key.org_id.clone(),
                        stream: cached.stream.clone(),
                        trace_id: key.trace_id.clone(),
                        policy_version: cached.policy_version,
                        kept: true,
                        reason: cached.reason,
                        spans: vec![candidate.span],
                    });
                    CandidateDisposition::LateKept
                }
                None => {
                    cached.digests.insert(span_id, digest);
                    self.metrics.late_dropped.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "late", 1);
                    CandidateDisposition::LateDropped
                }
            });
            self.observe_cache(&state);
            return output;
        }

        if let Some(pending) = state.traces.get_mut(&key) {
            if pending.stream != candidate.stream {
                self.metrics.conflicts.fetch_add(1, Ordering::Relaxed);
                output.disposition = Some(CandidateDisposition::ConflictingDuplicate);
                return output;
            }
            output.disposition = Some(match pending.digests.get(&span_id) {
                Some(previous) if previous == &digest => {
                    self.metrics.duplicates.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "duplicate", 1);
                    CandidateDisposition::IdenticalDuplicate
                }
                Some(_) => {
                    if let Some(original) = pending
                        .spans
                        .iter_mut()
                        .find(|span| span.span_id == span_id)
                    {
                        original.conflict = true;
                    }
                    self.metrics.conflicts.fetch_add(1, Ordering::Relaxed);
                    trace_metrics::record_spans("sampler", "conflict", 1);
                    CandidateDisposition::ConflictingDuplicate
                }
                None => {
                    pending.digests.insert(span_id, digest);
                    pending.estimated_bytes = pending.estimated_bytes.saturating_add(size);
                    pending.force_keep = stronger_force(pending.force_keep, candidate.force_keep);
                    if candidate.span.parent_span_id.is_none() {
                        pending.root_ended_ms.get_or_insert(now_ms);
                    }
                    pending.spans.push(candidate.span);
                    state.estimated_bytes = state.estimated_bytes.saturating_add(size);
                    self.metrics.accepted.fetch_add(1, Ordering::Relaxed);
                    CandidateDisposition::Accepted
                }
            });
        } else {
            let policy = self.policy.read().clone();
            let mut pending = PendingTrace {
                policy,
                stream: candidate.stream,
                spans: vec![candidate.span],
                digests: HashMap::from([(span_id, digest)]),
                first_seen_ms: now_ms,
                root_ended_ms: None,
                estimated_bytes: size,
                force_keep: candidate.force_keep,
            };
            if pending.spans[0].parent_span_id.is_none() {
                pending.root_ended_ms = Some(now_ms);
            }
            state.estimated_bytes = state.estimated_bytes.saturating_add(size);
            state.traces.insert(key, pending);
            self.metrics.accepted.fetch_add(1, Ordering::Relaxed);
            output.disposition = Some(CandidateDisposition::Accepted);
        }

        self.resolve_pressure(&mut state, now_ms, &mut output.decided);
        self.resolve_due(&mut state, now_ms, &mut output.decided);
        self.observe_cache(&state);
        output
    }

    pub fn tick(&self) -> Vec<DecidedTrace> {
        self.tick_at(now_millis())
    }

    pub fn tick_at(&self, now_ms: u64) -> Vec<DecidedTrace> {
        let mut state = self.state.lock();
        purge_expired_decisions(&mut state, now_ms);
        let mut decided = Vec::new();
        self.resolve_due(&mut state, now_ms, &mut decided);
        self.observe_cache(&state);
        decided
    }

    pub fn flush(&self) -> Vec<DecidedTrace> {
        let mut state = self.state.lock();
        let keys: Vec<_> = state.traces.keys().cloned().collect();
        let now_ms = now_millis();
        let decided = keys
            .into_iter()
            .filter_map(|key| {
                remove_and_decide(
                    &mut state,
                    &key,
                    now_ms,
                    None,
                    self.deployment_force_disabled,
                    &self.metrics,
                )
            })
            .collect();
        self.observe_cache(&state);
        decided
    }

    /// owner 退出/崩溃估算：不复制、不写 Trace WAL，明确丢掉一个窗口的未决量。
    pub fn abandon_unresolved(&self) -> usize {
        let mut state = self.state.lock();
        let count = state.traces.len();
        state.traces.clear();
        state.estimated_bytes = 0;
        self.metrics
            .unresolved_loss
            .fetch_add(count as u64, Ordering::Relaxed);
        self.observe_cache(&state);
        count
    }

    pub fn metrics(&self) -> TailSamplerMetricSnapshot {
        let state = self.state.lock();
        let policy = self.policy.read();
        trace_metrics::set_tail_cache(
            state.traces.len(),
            state.estimated_bytes,
            policy.max_traces,
            policy.memory_bytes,
        );
        TailSamplerMetricSnapshot {
            accepted: self.metrics.accepted.load(Ordering::Relaxed),
            kept: self.metrics.kept.load(Ordering::Relaxed),
            dropped: self.metrics.dropped.load(Ordering::Relaxed),
            duplicates: self.metrics.duplicates.load(Ordering::Relaxed),
            conflicts: self.metrics.conflicts.load(Ordering::Relaxed),
            late_kept: self.metrics.late_kept.load(Ordering::Relaxed),
            late_dropped: self.metrics.late_dropped.load(Ordering::Relaxed),
            pressure_decisions: self.metrics.pressure_decisions.load(Ordering::Relaxed),
            unresolved_loss: self.metrics.unresolved_loss.load(Ordering::Relaxed),
            pending_traces: state.traces.len(),
            pending_bytes: state.estimated_bytes,
            capacity_traces: policy.max_traces,
            capacity_bytes: policy.memory_bytes,
        }
    }

    fn observe_cache(&self, state: &SamplerState) {
        let policy = self.policy.read();
        trace_metrics::set_tail_cache(
            state.traces.len(),
            state.estimated_bytes,
            policy.max_traces,
            policy.memory_bytes,
        );
    }

    fn resolve_due(&self, state: &mut SamplerState, now_ms: u64, output: &mut Vec<DecidedTrace>) {
        let due: Vec<_> = state
            .traces
            .iter()
            .filter(|(_, pending)| {
                now_ms
                    >= pending
                        .first_seen_ms
                        .saturating_add(pending.policy.decision_window_ms)
                    || pending.root_ended_ms.is_some_and(|root_end| {
                        now_ms >= root_end.saturating_add(pending.policy.root_grace_ms)
                    })
            })
            .map(|(key, _)| key.clone())
            .collect();
        output.extend(due.into_iter().filter_map(|key| {
            remove_and_decide(
                state,
                &key,
                now_ms,
                None,
                self.deployment_force_disabled,
                &self.metrics,
            )
        }));
    }

    fn resolve_pressure(
        &self,
        state: &mut SamplerState,
        now_ms: u64,
        output: &mut Vec<DecidedTrace>,
    ) {
        loop {
            let active = self.policy.read();
            let over = state.traces.len() > active.max_traces
                || state.estimated_bytes > active.memory_bytes;
            drop(active);
            if !over {
                break;
            }
            let victim = state
                .traces
                .iter()
                .min_by_key(|(_, pending)| (pending.priority(), pending.first_seen_ms))
                .map(|(key, pending)| (key.clone(), pending.priority()));
            let Some((key, priority)) = victim else {
                break;
            };
            let override_reason = (priority == 0).then_some(SamplingReason::PressureRatio);
            if let Some(decided) = remove_and_decide(
                state,
                &key,
                now_ms,
                override_reason,
                self.deployment_force_disabled,
                &self.metrics,
            ) {
                self.metrics
                    .pressure_decisions
                    .fetch_add(1, Ordering::Relaxed);
                output.push(decided);
            }
        }
    }
}

fn remove_and_decide(
    state: &mut SamplerState,
    key: &TraceKey,
    now_ms: u64,
    override_reason: Option<SamplingReason>,
    deployment_force_disabled: bool,
    metrics: &TailSamplerMetrics,
) -> Option<DecidedTrace> {
    let mut pending = state.traces.remove(key)?;
    let span_count = pending.spans.len() as u64;
    let decision_latency =
        std::time::Duration::from_millis(now_ms.saturating_sub(pending.first_seen_ms));
    state.estimated_bytes = state
        .estimated_bytes
        .saturating_sub(pending.estimated_bytes);
    let (kept, mut reason) = decide(&pending, deployment_force_disabled);
    let kept = if override_reason == Some(SamplingReason::PressureRatio) {
        deterministic_keep(&key.trace_id, pending.policy.normal_sample_ratio)
    } else {
        kept
    };
    if override_reason == Some(SamplingReason::PressureRatio) {
        reason = if kept {
            SamplingReason::PressureRatio
        } else {
            SamplingReason::PressureDrop
        };
    }

    if kept {
        enforce_span_cap(&mut pending.spans, pending.policy.max_spans_per_trace);
        for span in &mut pending.spans {
            span.sampling_reason = reason;
        }
        metrics.kept.fetch_add(1, Ordering::Relaxed);
    } else {
        pending.spans.clear();
        metrics.dropped.fetch_add(1, Ordering::Relaxed);
    }
    trace_metrics::record_decision(kept, reason.as_str(), span_count, decision_latency);
    state.decisions.insert(
        key.clone(),
        CachedDecision {
            kept,
            stream: pending.stream.clone(),
            reason,
            policy_version: pending.policy.version,
            expires_at_ms: now_ms.saturating_add(pending.policy.decision_cache_ms),
            digests: pending.digests,
        },
    );
    Some(DecidedTrace {
        org_id: key.org_id.clone(),
        stream: pending.stream,
        trace_id: key.trace_id.clone(),
        policy_version: pending.policy.version,
        kept,
        reason,
        spans: pending.spans,
    })
}

fn decide(pending: &PendingTrace, deployment_force_disabled: bool) -> (bool, SamplingReason) {
    if deployment_force_disabled || !pending.policy.enabled {
        return (false, SamplingReason::Disabled);
    }
    match pending.force_keep {
        ForceKeep::TrustedInternal => return (true, SamplingReason::TrustedForced),
        ForceKeep::DebugToken => return (true, SamplingReason::DebugForced),
        ForceKeep::None => {}
    }
    if pending.saw_error() {
        return (true, SamplingReason::Error);
    }
    if pending.saw_slow() {
        return (true, SamplingReason::Slow);
    }
    for rule in &pending.policy.rules {
        if pending.spans.iter().any(|span| rule.matches(span)) {
            return match rule.action {
                SamplingRuleAction::Keep => (true, SamplingReason::Rule),
                SamplingRuleAction::Drop => (false, SamplingReason::Rule),
                SamplingRuleAction::Ratio(ratio) => (
                    deterministic_keep(&pending.spans[0].trace_id, ratio),
                    SamplingReason::Rule,
                ),
            };
        }
    }
    (
        deterministic_keep(
            &pending.spans[0].trace_id,
            pending.policy.normal_sample_ratio,
        ),
        SamplingReason::Ratio,
    )
}

fn span_is_error(span: &CanonicalSpan) -> bool {
    span.status_code.eq_ignore_ascii_case("error")
        || span.attributes.contains_key("error.type")
        || span.attributes.get("error").and_then(Value::as_bool) == Some(true)
}

fn span_is_slow(span: &CanonicalSpan, policy: &TraceRuntimePolicy) -> bool {
    let route_override = span
        .attributes
        .get("http.route")
        .and_then(Value::as_str)
        .and_then(|route| policy.route_slow_thresholds_ms.get(route))
        .copied();
    let threshold_ms = route_override.unwrap_or_else(|| policy.slow_thresholds_ms.for_span(span));
    span.duration_ns >= threshold_ms.saturating_mul(1_000_000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanCategory {
    Http,
    Rpc,
    Query,
    BatchIngest,
    Database,
    ObjectStore,
    External,
    Background,
    Other,
}

fn span_category(span: &CanonicalSpan) -> SpanCategory {
    match span
        .attributes
        .get("molesignal.span.category")
        .and_then(Value::as_str)
    {
        Some("http") => SpanCategory::Http,
        Some("rpc") => SpanCategory::Rpc,
        Some("query") => SpanCategory::Query,
        Some("batch_ingest") => SpanCategory::BatchIngest,
        Some("database") => SpanCategory::Database,
        Some("object_store") => SpanCategory::ObjectStore,
        Some("external") => SpanCategory::External,
        Some("background") => SpanCategory::Background,
        _ if span.attributes.contains_key("db.system")
            || span.attributes.contains_key("db.system.name") =>
        {
            SpanCategory::Database
        }
        _ if span.attributes.contains_key("molesignal.object.operation")
            || span.name.starts_with("object_store.") =>
        {
            SpanCategory::ObjectStore
        }
        _ if span.attributes.contains_key("http.route") => SpanCategory::Http,
        _ if span.attributes.contains_key("rpc.system") => SpanCategory::Rpc,
        _ => SpanCategory::Other,
    }
}

fn enforce_span_cap(spans: &mut Vec<CanonicalSpan>, cap: usize) {
    if spans.len() <= cap {
        return;
    }
    spans.sort_by_key(|span| {
        (
            Reverse(span_is_error(span)),
            Reverse(span.duration_ns),
            span.start_time_unix_nano,
        )
    });
    let real_cap = cap.saturating_sub(1);
    let excess = spans.split_off(real_cap);
    let aggregate_count = excess.len();
    let aggregate_duration = excess
        .iter()
        .fold(0_u64, |total, span| total.saturating_add(span.duration_ns));
    let mut operation_counts: BTreeMap<String, u64> = BTreeMap::new();
    for span in &excess {
        *operation_counts.entry(span.name.clone()).or_default() += 1;
    }
    let template = excess.first().expect("excess is non-empty");
    let mut aggregate = CanonicalSpan::new(
        template.trace_id.clone(),
        hex::encode(&blake3::hash(template.trace_id.as_bytes()).as_bytes()[..8]),
        "molesignal.trace.excess".into(),
        0,
        excess
            .iter()
            .map(|span| span.start_time_unix_nano)
            .min()
            .unwrap_or(0),
        excess
            .iter()
            .map(|span| span.end_time_unix_nano)
            .max()
            .unwrap_or(0),
    );
    aggregate.duration_ns = aggregate_duration;
    aggregate.attributes.insert(
        "molesignal.trace.excess_span_count".into(),
        json!(aggregate_count),
    );
    aggregate.attributes.insert(
        "molesignal.trace.excess_duration_ns".into(),
        json!(aggregate_duration),
    );
    aggregate.attributes.insert(
        "molesignal.trace.excess_operation_counts".into(),
        serde_json::to_value(operation_counts).unwrap_or(Value::Null),
    );
    aggregate.mark_partial(PartialReason::SpanLimit);
    for span in spans.iter_mut() {
        span.mark_partial(PartialReason::SpanLimit);
    }
    spans.push(aggregate);
}

fn purge_expired_decisions(state: &mut SamplerState, now_ms: u64) {
    state
        .decisions
        .retain(|_, decision| decision.expires_at_ms > now_ms);
}

fn estimated_span_bytes(span: &CanonicalSpan) -> usize {
    serde_json::to_vec(span).map_or(512, |bytes| bytes.len().max(128))
}

fn stronger_force(left: ForceKeep, right: ForceKeep) -> ForceKeep {
    match (left, right) {
        (ForceKeep::DebugToken, _) | (_, ForceKeep::DebugToken) => ForceKeep::DebugToken,
        (ForceKeep::TrustedInternal, _) | (_, ForceKeep::TrustedInternal) => {
            ForceKeep::TrustedInternal
        }
        _ => ForceKeep::None,
    }
}

pub fn deterministic_keep(trace_id: &str, ratio: f64) -> bool {
    if ratio <= 0.0 {
        return false;
    }
    if ratio >= 1.0 {
        return true;
    }
    let digest = blake3::hash(trace_id.as_bytes());
    let value = u64::from_be_bytes(digest.as_bytes()[..8].try_into().unwrap());
    (value as f64) / (u64::MAX as f64) < ratio
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplerNode {
    pub node_id: String,
    pub endpoint: String,
    pub healthy: bool,
}

/// Highest-random-weight rendezvous hashing；与 `(org_id, stream)` WAL 分片无关。
pub fn rendezvous_owner<'a>(trace_id: &str, nodes: &'a [SamplerNode]) -> Option<&'a SamplerNode> {
    nodes.iter().filter(|node| node.healthy).max_by_key(|node| {
        let mut hasher = blake3::Hasher::new();
        hasher.update(trace_id.as_bytes());
        hasher.update(&[0]);
        hasher.update(node.node_id.as_bytes());
        u128::from_be_bytes(hasher.finalize().as_bytes()[..16].try_into().unwrap())
    })
}

fn now_millis() -> u64 {
    (TimestampMicros::now().0.max(0) as u64) / 1_000
}

fn default_true() -> bool {
    true
}
fn default_ratio() -> f64 {
    0.1
}
fn default_decision_window_ms() -> u64 {
    30_000
}
fn default_root_grace_ms() -> u64 {
    1_000
}
fn default_decision_cache_ms() -> u64 {
    300_000
}
fn default_max_traces() -> usize {
    10_000
}
fn default_memory_bytes() -> usize {
    256 * 1024 * 1024
}
fn default_max_spans() -> usize {
    1_000
}
fn default_http_ms() -> u64 {
    1_000
}
fn default_query_ms() -> u64 {
    5_000
}
fn default_ingest_ms() -> u64 {
    2_000
}
fn default_database_ms() -> u64 {
    200
}
fn default_object_store_ms() -> u64 {
    500
}
fn default_external_ms() -> u64 {
    1_000
}
fn default_background_ms() -> u64 {
    30_000
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::trace_fixtures;

    fn policy(ratio: f64) -> TraceRuntimePolicy {
        TraceRuntimePolicy {
            normal_sample_ratio: ratio,
            decision_window_ms: 5_000,
            root_grace_ms: 1_000,
            decision_cache_ms: 10_000,
            ..TraceRuntimePolicy::default()
        }
    }

    fn sampler(ratio: f64) -> TailSampler {
        TailSampler::new(policy(ratio), false, TraceLimits::default()).unwrap()
    }

    fn candidate(span: CanonicalSpan) -> TraceCandidate {
        TraceCandidate {
            org_id: "org".into(),
            stream: None,
            span,
            force_keep: ForceKeep::None,
        }
    }

    #[test]
    fn rendezvous_affinity_is_stable_and_distributed() {
        let nodes = vec![
            SamplerNode {
                node_id: "a".into(),
                endpoint: "a:1".into(),
                healthy: true,
            },
            SamplerNode {
                node_id: "b".into(),
                endpoint: "b:1".into(),
                healthy: true,
            },
            SamplerNode {
                node_id: "c".into(),
                endpoint: "c:1".into(),
                healthy: true,
            },
        ];
        let first = rendezvous_owner("trace-1", &nodes).unwrap().node_id.clone();
        assert_eq!(rendezvous_owner("trace-1", &nodes).unwrap().node_id, first);
        let owners: std::collections::HashSet<_> = (0..100)
            .map(|i| {
                rendezvous_owner(&format!("trace-{i}"), &nodes)
                    .unwrap()
                    .node_id
                    .clone()
            })
            .collect();
        assert!(owners.len() > 1);
    }

    #[test]
    fn decision_order_is_force_error_slow_rule_ratio() {
        let sampler_instance = sampler(0.0);

        let mut forced = trace_fixtures::canonical_http_trace().remove(0);
        forced.status_code = "ERROR".into();
        let mut forced_candidate = candidate(forced);
        forced_candidate.force_keep = ForceKeep::TrustedInternal;
        sampler_instance.accept_at(forced_candidate, 0);
        let decided = sampler_instance.tick_at(1_000);
        assert_eq!(decided[0].reason, SamplingReason::TrustedForced);

        let sampler_instance = sampler(0.0);
        let error = trace_fixtures::canonical_error_trace().remove(0);
        sampler_instance.accept_at(candidate(error), 0);
        let decided = sampler_instance.tick_at(1_000);
        assert_eq!(decided[0].reason, SamplingReason::Error);

        let sampler_instance = sampler(0.0);
        let slow = trace_fixtures::canonical_slow_trace().remove(0);
        sampler_instance.accept_at(candidate(slow), 0);
        let decided = sampler_instance.tick_at(1_000);
        assert_eq!(decided[0].reason, SamplingReason::Slow);
    }

    #[test]
    fn deterministic_ratio_is_stable() {
        for ratio in [0.0, 0.1, 0.5, 1.0] {
            assert_eq!(
                deterministic_keep("0123456789abcdef0123456789abcdef", ratio),
                deterministic_keep("0123456789abcdef0123456789abcdef", ratio)
            );
        }
    }

    #[test]
    fn root_grace_and_policy_binding_are_deterministic() {
        let sampler = sampler(1.0);
        let root = trace_fixtures::canonical_http_trace().remove(0);
        let trace_id = root.trace_id.clone();
        sampler.accept_at(candidate(root), 100);
        let mut next = policy(0.0);
        next.version = 2;
        sampler.publish_policy(next).unwrap();
        assert!(sampler.tick_at(1_099).is_empty());
        let decided = sampler.tick_at(1_100);
        assert!(decided[0].kept);
        assert_eq!(decided[0].policy_version, 1);

        let mut new_root = trace_fixtures::canonical_http_trace().remove(0);
        new_root.trace_id = format!("f{}", &trace_id[1..]);
        sampler.accept_at(candidate(new_root), 2_000);
        let decided = sampler.tick_at(3_000);
        assert!(!decided[0].kept);
        assert_eq!(decided[0].policy_version, 2);
    }

    #[test]
    fn duplicate_conflict_and_late_drop_are_separate() {
        let sampler = sampler(0.0);
        let span = trace_fixtures::canonical_http_trace().remove(0);
        assert_eq!(
            sampler.accept_at(candidate(span.clone()), 0).disposition,
            Some(CandidateDisposition::Accepted)
        );
        assert_eq!(
            sampler.accept_at(candidate(span.clone()), 10).disposition,
            Some(CandidateDisposition::IdenticalDuplicate)
        );
        let mut conflict = span.clone();
        conflict.name = "changed".into();
        assert_eq!(
            sampler.accept_at(candidate(conflict), 20).disposition,
            Some(CandidateDisposition::ConflictingDuplicate)
        );
        sampler.tick_at(1_000);
        let mut late = span;
        late.span_id = "ffffffffffffffff".into();
        assert_eq!(
            sampler.accept_at(candidate(late), 1_100).disposition,
            Some(CandidateDisposition::LateDropped)
        );
    }

    #[test]
    fn span_cap_keeps_error_and_aggregates_excess() {
        let mut policy = policy(1.0);
        policy.max_spans_per_trace = 3;
        let sampler = TailSampler::new(policy, false, TraceLimits::default()).unwrap();
        let mut spans = trace_fixtures::canonical_high_fanout_trace(12);
        spans[8].status_code = "ERROR".into();
        for span in spans {
            sampler.accept_at(candidate(span), 0);
        }
        let decided = sampler.tick_at(1_000);
        assert_eq!(decided[0].spans.len(), 3);
        assert!(decided[0].spans.iter().any(span_is_error));
        let aggregate = decided[0]
            .spans
            .iter()
            .find(|span| span.name == "molesignal.trace.excess")
            .unwrap();
        assert!(
            aggregate
                .partial_reasons
                .contains(&PartialReason::SpanLimit)
        );
    }

    #[test]
    fn pressure_decides_normal_trace_without_blocking_error_trace() {
        let mut policy = policy(0.0);
        policy.max_traces = 1;
        let sampler = TailSampler::new(policy, false, TraceLimits::default()).unwrap();
        let normal = trace_fixtures::canonical_http_trace().remove(0);
        sampler.accept_at(candidate(normal), 0);
        let error = trace_fixtures::canonical_error_trace().remove(0);
        let output = sampler.accept_at(candidate(error), 1);
        assert!(
            output
                .decided
                .iter()
                .any(|trace| trace.reason == SamplingReason::PressureDrop)
        );
        assert_eq!(sampler.metrics().pending_traces, 1);
    }

    #[test]
    fn child_slow_span_keeps_whole_trace() {
        let sampler = sampler(0.0);
        let mut spans = trace_fixtures::canonical_http_trace();
        let mut child = spans[0].clone();
        child.span_id = "2222222222222222".into();
        child.parent_span_id = Some(spans[0].span_id.clone());
        child.duration_ns = 2_000_000_000;
        child
            .attributes
            .insert("molesignal.span.category".into(), json!("object_store"));
        spans.push(child);
        for span in spans {
            sampler.accept_at(candidate(span), 0);
        }
        let decided = sampler.tick_at(1_000);
        assert_eq!(decided[0].reason, SamplingReason::Slow);
        assert_eq!(decided[0].spans.len(), 3);
    }

    #[test]
    fn deployment_force_disable_wins_over_debug_force() {
        let sampler = TailSampler::new(policy(1.0), true, TraceLimits::default()).unwrap();
        let mut candidate = candidate(trace_fixtures::canonical_error_trace().remove(0));
        candidate.force_keep = ForceKeep::DebugToken;
        sampler.accept_at(candidate, 0);
        let decided = sampler.tick_at(1_000);
        assert!(!decided[0].kept);
        assert_eq!(decided[0].reason, SamplingReason::Disabled);
    }

    #[test]
    fn owner_loss_reports_unresolved_estimate() {
        let sampler = sampler(1.0);
        sampler.accept_at(
            candidate(trace_fixtures::canonical_http_trace().remove(0)),
            0,
        );
        assert_eq!(sampler.abandon_unresolved(), 1);
        assert_eq!(sampler.metrics().unresolved_loss, 1);
    }
}
