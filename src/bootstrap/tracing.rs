// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Self-telemetry streams 与异步 runtime 的启动装配。

use std::sync::Arc;

use super::{core::Core, iam::IamRuntime, query::QueryRuntime, storage::StorageRuntime};
use crate::{
    api::AppState,
    app::{
        apm::{ApmQueryConfig, ApmQueryService, ApmRuntime},
        trace::{
            SelfIngestTraceSink, TracePipeline, TracePipelineConfig, TraceSink,
            TraceSinkWorkerConfig,
            candidate_router::{TraceCandidateRouter, TraceCandidateRouterConfig},
            export::ExternalOtlpTraceSink,
        },
    },
    config::{SelfCollectSettings, Settings},
    domain::{
        iam::{OrganizationRepository, SYSTEM_ORG_SLUG},
        stream::{
            MOLESIGNAL_SYSTEM_STREAM, Retention, Schema, StreamDefinition, StreamRepository,
            StreamType,
        },
    },
    infra::{
        apm::PgApmRepository,
        rum::replay::{PgRumReplayMetaRepository, RumReplayWriter},
        traces::{PgServiceGraphRepository, ServiceGraphRepository},
    },
    shared::{
        Error, Result,
        ids::Id,
        tail_sampling::{TailSampler, TraceRuntimePolicy},
        time::TimestampMicros,
        trace_normalization::TraceLimits,
    },
};

pub(super) struct TracingRuntime {
    pub(super) policy_loaded: bool,
    pub(super) tail_sampler: Arc<TailSampler>,
    pub(super) candidates: Arc<TraceCandidateRouter>,
    pub(super) pipeline: Arc<TracePipeline>,
    pub(super) cluster_token: Option<Arc<str>>,
    pub(super) self_telemetry_profiles_enabled: bool,
    pub(super) self_telemetry_org_id: Option<Id>,
    pub(super) service_graph: Arc<dyn ServiceGraphRepository>,
    pub(super) rum_replay: Arc<RumReplayWriter>,
    pub(super) apm_query: Arc<ApmQueryService>,
    pub(super) apm_runtime: Option<Arc<ApmRuntime>>,
}

impl TracingRuntime {
    pub(super) async fn build(
        settings: &Settings,
        core: &Core,
        query: &QueryRuntime,
        storage: &StorageRuntime,
        iam: &IamRuntime,
    ) -> Result<Self> {
        let configured_policy = TraceRuntimePolicy::from(&settings.telemetry.trace);
        let (active_policy, policy_loaded) = match core.trace_policies.active().await {
            Ok(Some(persisted)) => match persisted.policy.validate() {
                Ok(()) => (persisted.policy, true),
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "persisted Trace policy is invalid; using deployment/code defaults"
                    );
                    (configured_policy.clone(), false)
                }
            },
            Ok(None) => {
                let bootstrap_actor = Id("system-bootstrap".into());
                match core
                    .trace_policies
                    .publish(
                        &core.system_org.id,
                        configured_policy.clone(),
                        &bootstrap_actor,
                    )
                    .await
                {
                    Ok(persisted) => (persisted.policy, true),
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "Trace policy bootstrap failed; continuing with in-memory defaults"
                        );
                        (configured_policy.clone(), false)
                    }
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Trace policy load failed; continuing with in-memory defaults"
                );
                (configured_policy, false)
            }
        };
        crate::shared::trace_metrics::set_system_load("trace_policy", policy_loaded);

        let trace_limits = TraceLimits {
            max_attributes_per_span: settings.telemetry.trace.max_attributes_per_span,
            max_events_per_span: settings.telemetry.trace.max_events_per_span,
            max_links_per_span: settings.telemetry.trace.max_links_per_span,
            max_string_bytes: settings.telemetry.trace.max_string_bytes,
            max_spans_per_trace: settings.telemetry.trace.max_spans_per_trace,
        };
        let tail_sampler = Arc::new(
            TailSampler::new(
                active_policy,
                settings.telemetry.trace.force_disabled,
                trace_limits,
            )
            .map_err(Error::invalid)?,
        );
        let self_trace_sink: Option<Arc<dyn TraceSink>> =
            settings.telemetry.self_collect.enabled.then(|| {
                Arc::new(SelfIngestTraceSink::new(
                    storage.ingestion.clone(),
                    core.system_org.id.clone(),
                )) as Arc<dyn TraceSink>
            });
        let mut local_trace_endpoints = vec![
            format!(
                "http://{}:{}",
                settings.otlp_grpc.bind, settings.otlp_grpc.port
            ),
            format!("http://{}:{}", settings.grpc.bind, settings.grpc.port),
            format!("http://{}:{}", settings.http.bind, settings.http.port),
        ];
        if !settings.http.external_url.trim().is_empty() {
            local_trace_endpoints.push(settings.http.external_url.clone());
        }
        let external_trace_sink: Option<Arc<dyn TraceSink>> =
            ExternalOtlpTraceSink::new(&settings.telemetry.trace.external, &local_trace_endpoints)
                .map_err(Error::invalid)?
                .map(|sink| sink as Arc<dyn TraceSink>);
        let apm_repository = Arc::new(PgApmRepository::new(core.pool.clone()));
        let apm_query = Arc::new(ApmQueryService::new(
            apm_repository.clone() as Arc<dyn crate::domain::apm::ApmQueryRepository>,
            ApmQueryConfig::from_settings(&settings.apm),
        ));
        let owns_trace_candidates = core.roles.run_ingester || core.roles.run_querier;
        let runs_apm_rollup = core.roles.run_alert_manager;
        let apm_runtime = (owns_trace_candidates || runs_apm_rollup)
            .then(|| {
                ApmRuntime::start(
                    core.node_id.clone(),
                    apm_repository.clone(),
                    &settings.apm,
                    owns_trace_candidates,
                    runs_apm_rollup,
                )
            })
            .transpose()?;
        let pipeline = TracePipeline::start_with_apm(
            tail_sampler.clone(),
            apm_runtime.as_ref().and_then(|runtime| runtime.projector()),
            self_trace_sink,
            external_trace_sink,
            TracePipelineConfig {
                candidate_capacity: settings.telemetry.trace.tail_max_traces.max(1),
                decision_tick: std::time::Duration::from_millis(100),
                shutdown_timeout: std::time::Duration::from_secs(
                    settings.telemetry.trace.shutdown_timeout_secs,
                ),
                self_ingest: TraceSinkWorkerConfig {
                    queue_capacity: settings.telemetry.self_collect.queue_capacity,
                    batch_size: settings.telemetry.self_collect.batch_max_events,
                    batch_delay: std::time::Duration::from_millis(
                        settings.telemetry.self_collect.batch_max_delay_ms,
                    ),
                    export_timeout: std::time::Duration::from_secs(
                        settings.telemetry.self_collect.flush_timeout_secs,
                    ),
                    ..TraceSinkWorkerConfig::default()
                },
                external: TraceSinkWorkerConfig {
                    queue_capacity: settings.telemetry.trace.external.queue_capacity,
                    batch_size: settings.telemetry.trace.external.batch_size,
                    export_timeout: std::time::Duration::from_millis(
                        settings.telemetry.trace.external.timeout_ms,
                    ),
                    ..TraceSinkWorkerConfig::default()
                },
            },
            trace_limits,
        )
        .map_err(Error::invalid)?;
        let cluster_token =
            crate::app::self_telemetry::configured_cluster_token().map(Arc::<str>::from);
        let candidates = TraceCandidateRouter::start(
            core.registry.clone(),
            core.node_id.clone(),
            settings.cluster.advertise_addr.clone(),
            core.roles.run_ingester || core.roles.run_querier,
            cluster_token.clone(),
            pipeline.clone(),
            TraceCandidateRouterConfig {
                queue_capacity: settings.telemetry.trace.tail_max_traces.max(1),
                shutdown_timeout: std::time::Duration::from_secs(
                    settings.telemetry.trace.shutdown_timeout_secs,
                ),
                ..TraceCandidateRouterConfig::default()
            },
            trace_limits,
        )
        .map_err(Error::invalid)?;

        let trace_capture_enabled = settings.telemetry.trace.effective_enabled();
        let self_telemetry_org_id = prepare_self_telemetry_streams(
            core.orgs.as_ref(),
            core.streams.as_ref(),
            &settings.telemetry.self_collect,
            trace_capture_enabled,
        )
        .await?;

        let service_graph: Arc<dyn ServiceGraphRepository> =
            Arc::new(PgServiceGraphRepository::new(core.pool.clone()));
        let _service_graph_flush =
            crate::bootstrap::workers::service_graph::flush::ServiceGraphFlusher::new(
                storage.service_graph_aggregator.clone(),
                service_graph.clone(),
                iam.instance_settings.clone(),
                crate::bootstrap::workers::service_graph::flush::ServiceGraphFlushConfig::default(),
            )
            .spawn();
        let _service_graph_recompute = core.roles.run_alert_manager.then(|| {
            crate::bootstrap::workers::service_graph::recompute::ServiceGraphRecomputer::new(
                query.query.clone(),
                core.orgs.clone() as Arc<dyn OrganizationRepository>,
                core.streams.clone() as Arc<dyn StreamRepository>,
                service_graph.clone(),
                iam.instance_settings.clone(),
                crate::bootstrap::workers::service_graph::recompute::ServiceGraphRecomputeConfig::default(),
            )
            .spawn()
        });

        let rum_replay_meta = Arc::new(PgRumReplayMetaRepository::new(core.pool.clone()));
        let rum_replay = Arc::new(RumReplayWriter::new(core.store.clone(), rum_replay_meta));
        let _rum_replay_retention = core.roles.run_compactor.then(|| {
            crate::bootstrap::workers::rum_replay_retention::spawn(
                rum_replay.clone(),
                settings.compactor.retention_days,
                core.drain_controller.clone(),
            )
        });

        Ok(Self {
            policy_loaded,
            tail_sampler,
            candidates,
            pipeline,
            cluster_token,
            self_telemetry_profiles_enabled: settings.telemetry.self_collect.enabled,
            self_telemetry_org_id,
            service_graph,
            rum_replay,
            apm_query,
            apm_runtime,
        })
    }
}

/// `_sys`、typed `_molesignal` streams 与 ingestion 均准备完成后，才把启动早期
/// bounded callback queues 绑定到异步 runtime。
pub fn activate_self_telemetry(
    state: &mut AppState,
    settings: &Settings,
    hub: Arc<crate::shared::self_telemetry::SelfTelemetryHub>,
) -> Result<()> {
    let self_telemetry_enabled = settings.telemetry.self_collect.enabled;
    let trace_capture_enabled = settings.telemetry.trace.effective_enabled();
    if !self_telemetry_enabled && !trace_capture_enabled {
        hub.stop_accepting();
        return Ok(());
    }
    let org_id = state
        .telemetry
        .self_telemetry_org_id
        .clone()
        .ok_or_else(|| Error::internal("self telemetry system organization was not resolved"))?;
    if org_id != state.iam.system_org_id {
        return Err(Error::internal(
            "self telemetry target does not match the `_sys` organization",
        ));
    }
    hub.resource().set_node_id(state.cluster.node_id.clone());
    state.telemetry.self_telemetry_resource = Some(hub.resource().clone());

    let has_local_ingester = settings
        .node
        .roles
        .contains(&crate::config::Role::Standalone)
        || settings.node.roles.contains(&crate::config::Role::Ingester);
    let profile_context =
        self_telemetry_enabled.then(|| crate::app::self_telemetry::SelfProfileContext {
            profiling: state.telemetry.profiling_service.clone(),
            storage: state.telemetry.profile_storage.clone(),
        });
    let runtime = if has_local_ingester {
        crate::app::self_telemetry::SelfTelemetryRuntime::start_local_with_trace_candidates(
            hub,
            org_id,
            settings.telemetry.self_collect.clone(),
            state.ingestion.clone(),
            profile_context,
            state.telemetry.trace_candidates.clone(),
        )
    } else {
        let token = state
            .telemetry
            .self_telemetry_cluster_token
            .as_deref()
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "{} must be set when split-role self ingestion is enabled",
                    crate::app::self_telemetry::CLUSTER_TOKEN_ENV
                ))
            })?;
        crate::app::self_telemetry::SelfTelemetryRuntime::start_remote_with_trace_candidates(
            hub,
            org_id,
            settings.telemetry.self_collect.clone(),
            state.cluster.registry.clone(),
            token,
            profile_context,
            state.telemetry.trace_candidates.clone(),
        )?
    };
    state.telemetry.self_telemetry_runtime = Some(runtime);
    tracing::info!(
        delivery = if has_local_ingester {
            "local"
        } else {
            "cluster"
        },
        "self telemetry runtime activated"
    );
    Ok(())
}

pub(super) async fn prepare_self_telemetry_streams(
    orgs: &dyn OrganizationRepository,
    streams: &dyn StreamRepository,
    settings: &SelfCollectSettings,
    trace_capture_enabled: bool,
) -> Result<Option<Id>> {
    if !settings.enabled && !trace_capture_enabled {
        return Ok(None);
    }

    let org = orgs.get_by_slug(SYSTEM_ORG_SLUG).await.map_err(|error| {
        if matches!(error, Error::NotFound(_)) {
            Error::internal("system organization was not prepared")
        } else {
            error
        }
    })?;
    org.validate_system_invariants().map_err(|error| {
        Error::internal(format!(
            "invalid self telemetry system organization: {error}"
        ))
    })?;
    if !org.system {
        return Err(Error::internal(
            "self telemetry target must be the `_sys` system organization",
        ));
    }

    let enabled_types = [
        (
            settings.enabled,
            StreamType::Logs,
            settings.logs_retention_days,
        ),
        (
            settings.enabled && settings.metrics_enabled,
            StreamType::Metrics,
            settings.metrics_retention_days,
        ),
        (
            settings.enabled && trace_capture_enabled,
            StreamType::Traces,
            settings.traces_retention_days,
        ),
        (
            settings.enabled,
            StreamType::Profiles,
            settings.profiles_retention_days,
        ),
    ];
    for (_, stream_type, retention_days) in
        enabled_types.into_iter().filter(|(enabled, _, _)| *enabled)
    {
        let retention = Some(Retention {
            days: retention_days,
        });
        match streams
            .get(&org.id, MOLESIGNAL_SYSTEM_STREAM, stream_type)
            .await
        {
            Ok(existing) => {
                if existing.retention.map(|value| value.days) != Some(retention_days) {
                    streams.update_retention(&existing.id, retention).await?;
                }
            }
            Err(Error::NotFound(_)) => {
                let now = TimestampMicros::now();
                streams
                    .create(StreamDefinition {
                        id: Id::new(),
                        org_id: org.id.clone(),
                        name: MOLESIGNAL_SYSTEM_STREAM.into(),
                        stream_type,
                        schema: if stream_type == StreamType::Traces {
                            crate::shared::trace_normalization::canonical_trace_schema()
                        } else {
                            Schema { fields: Vec::new() }
                        },
                        retention,
                        created_at: now,
                        updated_at: now,
                    })
                    .await?;
            }
            Err(error) => return Err(error),
        }
    }

    tracing::info!(
        org_id = %org.id.0,
        org_slug = %org.slug,
        trace_retention_days = settings.traces_retention_days,
        "self telemetry system streams ready"
    );
    Ok(Some(org.id))
}
