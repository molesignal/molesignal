// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bootstrap 总编排入口：把 domain 抽象与 infra 实现绑起来，生成 app service，
//! 再喂给 api 的 `AppState`。
//!
//! 这是整个仓库里唯一允许同时依赖 domain / app / infra / api 的位置。

use std::sync::Arc;

pub use super::tracing::activate_self_telemetry;
use super::{
    alerting::AlertingRuntime,
    core::Core,
    iam::IamRuntime,
    intelligence::{IntelligenceRuntime, build_model_providers},
    license::LicenseRuntime,
    platform::PlatformRuntime,
    query::QueryRuntime,
    storage::StorageRuntime,
    tracing::TracingRuntime,
};
use crate::{
    api::state::{
        AlertingState, AppState, ClusterState, IamState, IntelligenceState, PlatformState,
        StorageState, TelemetryState, TraceSystemLoadHealth,
    },
    app::dashboard::{
        authoring::{DashboardAuthoringService, RuntimeDashboardQueryPreflight},
        contract_registry::{DashboardContractRegistryService, DashboardContractResolver},
    },
    config::Settings,
    domain::stream::StreamRepository,
    infra::{cipher::CipherRootKey, persistence::MetaStore},
    shared::{Error, Result},
};

/// KEK 轮换离线工具：用 `old_key_b64`（旧 KEK，base64 32B）解、当前 `MS_CIPHER_KEY`（新 KEK）
/// 重封 DB 里全部 KEK-sealed 列，返回每表重包行数。**离线运维操作**（停写窗口执行）：
/// 跑完核对各表计数无误后方可下线旧 KEK。字段数据本身不动（只换信封）。
pub async fn rewrap_kek(settings: &Settings, old_key_b64: &str) -> Result<Vec<(String, usize)>> {
    let old = CipherRootKey::from_base64(old_key_b64)
        .map_err(|e| Error::invalid(format!("old KEK invalid: {e}")))?;
    let new = CipherRootKey::from_env()
        .map_err(|e| Error::invalid(format!("new KEK (MS_CIPHER_KEY) invalid: {e}")))?;
    let meta = MetaStore::connect(&settings.store.meta).await?;
    crate::infra::cipher::kek_rewrap::rewrap_all(&meta.pool, &old, &new).await
}

pub async fn build_state(settings: &Settings) -> Result<AppState> {
    let core = Core::build(settings).await?;
    let query_runtime = QueryRuntime::build(settings, &core).await;
    let intelligence_model_providers = build_model_providers(&core);
    let storage_runtime =
        StorageRuntime::build(settings, &core, intelligence_model_providers.clone()).await?;
    let dashboard_contract_registry = Arc::new(DashboardContractRegistryService::new(
        core.dashboard_contracts.clone(),
    ));
    dashboard_contract_registry.publish_builtins().await?;
    let dashboard_contracts: Arc<dyn DashboardContractResolver> = dashboard_contract_registry;
    let alerting_runtime =
        AlertingRuntime::build(settings, &core, &query_runtime, dashboard_contracts.clone())?;
    let license_runtime = LicenseRuntime::build(settings, &core).await;
    let iam_runtime = IamRuntime::build(settings, &core, &license_runtime).await?;
    let tracing_runtime = TracingRuntime::build(
        settings,
        &core,
        &query_runtime,
        &storage_runtime,
        &iam_runtime,
    )
    .await?;
    let platform_runtime =
        PlatformRuntime::build(settings, &core, &query_runtime, &storage_runtime).await?;
    let intelligence_runtime = IntelligenceRuntime::build(
        &core,
        &iam_runtime,
        &license_runtime,
        intelligence_model_providers.clone(),
    );
    let dashboard_authoring = Arc::new(
        DashboardAuthoringService::new(
            core.dashboard_drafts.clone(),
            Arc::new(RuntimeDashboardQueryPreflight::new(
                query_runtime.query.clone(),
                core.streams.clone(),
            )),
            alerting_runtime.dashboard.clone(),
        )
        .with_contract_resolver(dashboard_contracts),
    );
    query_runtime.spawn_federation_workers(settings, &core, &iam_runtime);
    let Core {
        store,
        system_org,
        password_resets,
        iam_platform_administrators,
        license_versions,
        trace_policies,
        trace_debug_tokens,
        teams,
        org_schema_cache,
        streams,
        parquet_file_meta,
        saved_views,
        email_sender,
        cipher_keys,
        field_key_service,
        regex_patterns,
        masking_service,
        cluster_repo,
        registry,
        drain_controller,
        node_id,
        ..
    } = core;
    let QueryRuntime {
        query,
        remote_clusters,
        cluster_event_outbox,
        cluster_resource_version,
        cluster_org_link,
        seen_events,
        federation_cancel,
        cluster_secrets,
        ..
    } = query_runtime;
    let StorageRuntime {
        probe,
        ingestion,
        profile_storage,
        profiling_service,
        prometheus_series_admission,
        functions_js_runtime_enabled,
        usage,
        investigation_blobs,
        ..
    } = storage_runtime;
    let AlertingRuntime {
        alerting,
        notify,
        notify_engine,
        dashboard,
        notify_templates,
        mute_rules,
        incident_groups,
        semantic_groups,
    } = alerting_runtime;
    let LicenseRuntime {
        license,
        holder: license_holder,
        loaded: license_loaded,
    } = license_runtime;
    let IamRuntime {
        iam,
        access: iam_access,
        instance_settings,
        signing_secrets,
        api_tokens,
        user_preferences,
        workspace_preference_defaults,
        invitations,
        roles: iam_roles,
        email_domains,
        audit_events,
        sso_state_store,
        sso_jwks_cache,
        sso_sessions,
        sso_providers,
    } = iam_runtime;
    let TracingRuntime {
        policy_loaded: trace_policy_loaded,
        tail_sampler,
        candidates: trace_candidates,
        pipeline: trace_pipeline,
        cluster_token: trace_cluster_token,
        self_telemetry_profiles_enabled,
        self_telemetry_org_id,
        service_graph: service_graph_repo,
        rum_replay,
        apm_query,
        apm_runtime,
    } = tracing_runtime;
    let PlatformRuntime {
        connectors,
        scheduled_pipelines,
        extend_kv,
        extend_table,
        parquet_disk_cache,
        resource_shares,
        annotations,
        search_jobs,
        functions,
        debug_artifacts,
        log_patterns,
        scheduled_reports,
        report_templates,
        report_renderer,
        report_renderer_base_url,
        file_download_tokens,
        web_search,
        marketplace,
        model_prices,
        domains,
        billing_settings,
        trials,
        billing_enabled,
        billing_state_cache,
        pipeline_runs,
        quotas,
    } = platform_runtime;
    let IntelligenceRuntime {
        chats: intelligence_chats,
        intelligence,
        toolsets: intelligence_toolsets,
        tool_control: intelligence_tool_control,
        prompts: intelligence_prompts,
        chat_archives: intelligence_chat_archives,
        incident_rca,
        slow_queries,
    } = intelligence_runtime;

    Ok(AppState {
        ingestion,
        query,
        dashboard,
        alerting: AlertingState {
            service: alerting,
            notify,
            notify_engine,
            incident_groups,
            semantic_groups,
            templates: notify_templates,
            mute_rules,
        },
        iam: IamState {
            service: iam,
            access: iam_access,
            system_org_id: system_org.id,
            teams,
            platform_administrators: iam_platform_administrators,
            password_resets,
            instance_settings,
            sso_state_store,
            sso_jwks_cache,
            sso_sessions,
            sso_providers,
            email_sender,
            invitations,
            email_domains,
            roles: iam_roles,
            signing_secrets,
            api_tokens,
            user_preferences,
            workspace_preference_defaults,
            audit_events,
        },
        telemetry: TelemetryState {
            streams: streams as Arc<dyn StreamRepository>,
            stream_retention_days: settings.compactor.retention_days.max(1),
            self_telemetry_org_id,
            self_telemetry_runtime: None,
            self_telemetry_resource: None,
            self_telemetry_profiles_enabled,
            self_telemetry_cluster_token: trace_cluster_token,
            profile_storage,
            profiling_service,
            profiling_settings: settings.profiling.clone(),
            prometheus_series_admission,
            probe,
            system_load_health: TraceSystemLoadHealth {
                system_org: true,
                license: license_loaded,
                trace_policy: trace_policy_loaded,
            },
            service_graph: service_graph_repo,
            rum_replay,
            apm_query,
            apm_runtime,
            trace_policies,
            trace_debug_tokens,
            tail_sampler,
            trace_candidates,
            trace_pipeline,
        },
        storage: StorageState {
            object_store: store,
            parquet_file_meta,
            cipher_keys,
            field_keys: field_key_service,
            connectors,
            scheduled_pipelines,
            extend_kv,
            extend_table,
            parquet_disk_cache,
            resource_shares,
            annotations,
            search_jobs,
            functions,
            functions_js_runtime_enabled,
            debug_artifacts,
            log_patterns,
            file_download_tokens,
            web_search,
            investigation_blobs,
            org_schema_cache,
            pipeline_runs,
            regex_patterns,
            masking: masking_service,
        },
        cluster: ClusterState {
            node_id,
            drain: drain_controller,
            registry,
            repository: cluster_repo,
            remote_clusters,
            event_outbox: cluster_event_outbox,
            resource_version: cluster_resource_version,
            org_link: cluster_org_link,
            seen_events,
            federation_cancel,
            secrets: cluster_secrets,
        },
        platform: PlatformState {
            saved_view: saved_views,
            external_url: settings.http.external_url.clone(),
            license,
            license_holder,
            license_versions,
            scheduled_reports,
            report_templates,
            report_renderer,
            report_renderer_base_url,
            marketplace,
            billing_settings,
            usage,
            trials,
            billing_enabled,
            billing_state_cache,
            quotas,
            model_prices,
            domains,
        },
        intelligence: IntelligenceState {
            dashboard_authoring,
            chats: intelligence_chats,
            repository: intelligence,
            toolsets: intelligence_toolsets,
            tool_control: intelligence_tool_control,
            model_providers: intelligence_model_providers,
            prompts: intelligence_prompts,
            chat_archives: intelligence_chat_archives,
            incident_rca,
            slow_queries,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use object_store::memory::InMemory;
    use tempfile::TempDir;

    use crate::{
        bootstrap::{
            license::build_license,
            storage::{build_fsync_policy, build_parquet_disk_cache},
            tracing::prepare_self_telemetry_streams,
        },
        config::{
            CacheSettings, ObjectStoreSettings, SelfCollectSettings, Settings, WalFlushStrategy,
            WalSettings, WalSyncLevel,
        },
        domain::{
            iam::{Organization, OrganizationRepository},
            license::{ActiveLicenseVersion, LicenseVersion, LicenseVersionRepository},
            stream::{
                MOLESIGNAL_SYSTEM_STREAM, Retention, Schema, StreamDefinition, StreamRepository,
                StreamType,
            },
        },
        infra::{segment_wal::FsyncPolicy, storage::object::production::ProductionObjectStore},
        shared::{Error, Result, ids::Id, time::TimestampMicros},
    };

    struct TestLicenseVersions {
        active: Option<ActiveLicenseVersion>,
        fail_load: bool,
    }

    #[async_trait::async_trait]
    impl LicenseVersionRepository for TestLicenseVersions {
        async fn list(&self) -> Result<Vec<LicenseVersion>> {
            Ok(self
                .active
                .as_ref()
                .map(|active| vec![active.version.clone()])
                .unwrap_or_default())
        }

        async fn get(&self, id: &Id) -> Result<LicenseVersion> {
            self.active
                .as_ref()
                .filter(|active| &active.version.id == id)
                .map(|active| active.version.clone())
                .ok_or_else(|| Error::not_found("License version"))
        }

        async fn active(&self) -> Result<Option<ActiveLicenseVersion>> {
            if self.fail_load {
                Err(Error::internal("fixture License store unavailable"))
            } else {
                Ok(self.active.clone())
            }
        }

        async fn insert_and_activate(
            &self,
            _version: LicenseVersion,
            _actor_id: Option<&Id>,
        ) -> Result<ActiveLicenseVersion> {
            Err(Error::internal("not used by License load test"))
        }

        async fn activate(&self, _id: &Id, _actor_id: &Id) -> Result<ActiveLicenseVersion> {
            Err(Error::internal("not used by License load test"))
        }
    }

    struct TestOrganizations {
        org: Option<Organization>,
    }

    #[async_trait::async_trait]
    impl OrganizationRepository for TestOrganizations {
        async fn create(&self, org: Organization) -> Result<Organization> {
            Ok(org)
        }

        async fn get(&self, id: &Id) -> Result<Organization> {
            self.org
                .clone()
                .filter(|org| &org.id == id)
                .ok_or_else(|| Error::not_found("organization"))
        }

        async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
            self.org
                .clone()
                .filter(|org| org.slug == slug)
                .ok_or_else(|| Error::not_found("organization"))
        }

        async fn list(&self) -> Result<Vec<Organization>> {
            Ok(self.org.clone().into_iter().collect())
        }

        async fn update_name(&self, id: &Id, name: String) -> Result<Organization> {
            let mut org = self.get(id).await?;
            org.ensure_mutable()?;
            org.name = name;
            Ok(org)
        }

        async fn set_disabled(&self, id: &Id, disabled: bool) -> Result<Organization> {
            let mut org = self.get(id).await?;
            org.ensure_mutable()?;
            org.disabled = disabled;
            Ok(org)
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestStreams {
        definitions: Mutex<Vec<StreamDefinition>>,
    }

    #[async_trait::async_trait]
    impl StreamRepository for TestStreams {
        async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
            self.definitions.lock().unwrap().push(def.clone());
            Ok(def)
        }

        async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()> {
            let mut definitions = self.definitions.lock().unwrap();
            let definition = definitions
                .iter_mut()
                .find(|definition| &definition.id == id)
                .ok_or_else(|| Error::not_found("stream"))?;
            definition.schema = schema;
            Ok(())
        }

        async fn update_retention(&self, id: &Id, retention: Option<Retention>) -> Result<()> {
            let mut definitions = self.definitions.lock().unwrap();
            let definition = definitions
                .iter_mut()
                .find(|definition| &definition.id == id)
                .ok_or_else(|| Error::not_found("stream"))?;
            definition.retention = retention;
            Ok(())
        }

        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            self.definitions
                .lock()
                .unwrap()
                .iter()
                .find(|definition| {
                    &definition.org_id == org_id
                        && definition.name == name
                        && definition.stream_type == stream_type
                })
                .cloned()
                .ok_or_else(|| Error::not_found("stream"))
        }

        async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(self
                .definitions
                .lock()
                .unwrap()
                .iter()
                .filter(|definition| &definition.org_id == org_id)
                .cloned()
                .collect())
        }

        async fn delete(&self, id: &Id) -> Result<()> {
            self.definitions
                .lock()
                .unwrap()
                .retain(|definition| &definition.id != id);
            Ok(())
        }
    }

    fn self_collect_settings() -> SelfCollectSettings {
        SelfCollectSettings {
            enabled: true,
            retention_days: 3,
            logs_retention_days: 3,
            metrics_retention_days: 3,
            traces_retention_days: 7,
            profiles_retention_days: 3,
            ..SelfCollectSettings::default()
        }
    }

    fn default_org() -> Organization {
        Organization {
            id: Id::from_string("management-org"),
            name: "_sys".into(),
            slug: "_sys".into(),
            system: true,
            disabled: false,
            created_at: TimestampMicros(1),
        }
    }

    #[tokio::test]
    async fn self_telemetry_startup_is_a_noop_when_disabled() {
        let orgs = TestOrganizations { org: None };
        let streams = TestStreams::default();
        let result =
            prepare_self_telemetry_streams(&orgs, &streams, &SelfCollectSettings::default(), false)
                .await
                .unwrap();
        assert!(result.is_none());
        assert!(streams.definitions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn self_telemetry_startup_reports_invariant_failure_for_a_missing_system_org() {
        let error = prepare_self_telemetry_streams(
            &TestOrganizations { org: None },
            &TestStreams::default(),
            &self_collect_settings(),
            true,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Internal(_)));
    }

    #[tokio::test]
    async fn self_telemetry_startup_rejects_a_non_system_target() {
        let mut tenant = default_org();
        tenant.system = false;
        let error = prepare_self_telemetry_streams(
            &TestOrganizations { org: Some(tenant) },
            &TestStreams::default(),
            &self_collect_settings(),
            true,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Internal(_)));
    }

    #[tokio::test]
    async fn self_telemetry_startup_precreates_all_typed_streams_and_updates_retention() {
        let orgs = TestOrganizations {
            org: Some(default_org()),
        };
        let streams = TestStreams::default();
        let org_id =
            prepare_self_telemetry_streams(&orgs, &streams, &self_collect_settings(), true)
                .await
                .unwrap()
                .unwrap();
        let definitions = streams.list(&org_id).await.unwrap();
        assert_eq!(definitions.len(), 4);
        for (stream_type, retention_days) in [
            (StreamType::Logs, 3),
            (StreamType::Metrics, 3),
            (StreamType::Traces, 7),
            (StreamType::Profiles, 3),
        ] {
            let stream = definitions
                .iter()
                .find(|definition| definition.stream_type == stream_type)
                .unwrap();
            assert_eq!(stream.name, MOLESIGNAL_SYSTEM_STREAM);
            assert_eq!(stream.retention.unwrap().days, retention_days);
        }

        let settings = SelfCollectSettings {
            retention_days: 9,
            logs_retention_days: 9,
            metrics_retention_days: 10,
            traces_retention_days: 11,
            profiles_retention_days: 12,
            ..self_collect_settings()
        };
        prepare_self_telemetry_streams(&orgs, &streams, &settings, true)
            .await
            .unwrap();
        let definitions = streams.list(&org_id).await.unwrap();
        for (stream_type, retention_days) in [
            (StreamType::Logs, 9),
            (StreamType::Metrics, 10),
            (StreamType::Traces, 11),
            (StreamType::Profiles, 12),
        ] {
            let stream = definitions
                .iter()
                .find(|definition| definition.stream_type == stream_type)
                .unwrap();
            assert_eq!(stream.retention.unwrap().days, retention_days);
        }
    }

    #[tokio::test]
    async fn self_telemetry_signal_selection_uses_master_metrics_and_trace_policy() {
        let orgs = TestOrganizations {
            org: Some(default_org()),
        };
        let ordinary_streams = TestStreams::default();
        let settings = SelfCollectSettings {
            metrics_enabled: false,
            ..self_collect_settings()
        };
        prepare_self_telemetry_streams(&orgs, &ordinary_streams, &settings, false)
            .await
            .unwrap();
        let ordinary = ordinary_streams
            .list(&default_org().id)
            .await
            .unwrap()
            .into_iter()
            .map(|definition| definition.stream_type)
            .collect::<Vec<_>>();
        assert_eq!(ordinary.len(), 2);
        assert!(ordinary.contains(&StreamType::Logs));
        assert!(ordinary.contains(&StreamType::Profiles));

        let external_only_streams = TestStreams::default();
        let org_id = prepare_self_telemetry_streams(
            &orgs,
            &external_only_streams,
            &SelfCollectSettings::default(),
            true,
        )
        .await
        .unwrap()
        .unwrap();
        let external_only = external_only_streams
            .list(&default_org().id)
            .await
            .unwrap()
            .into_iter()
            .map(|definition| definition.stream_type)
            .collect::<Vec<_>>();
        assert_eq!(org_id, default_org().id);
        assert!(external_only.is_empty());
    }

    #[tokio::test]
    async fn corrupt_persisted_license_degrades_to_community_and_unhealthy_load_state() {
        let version = LicenseVersion {
            id: Id::from_string("license-corrupt"),
            system_org_id: Id::from_string("system-org"),
            signed_package: serde_json::json!({
                "payload_b64": "not-valid-base64!",
                "signature_b64": "not-valid-base64!"
            }),
            payload_digest: "fixture-digest".into(),
            summary: serde_json::json!({}),
            created_by: None,
            created_at: TimestampMicros(1),
        };
        let repository = TestLicenseVersions {
            active: Some(ActiveLicenseVersion {
                version,
                activated_by: None,
                activated_at: TimestampMicros(2),
            }),
            fail_load: false,
        };

        let (license, healthy) = build_license(
            &repository,
            &Id::from_string("system-org"),
            &Settings::default(),
        )
        .await;

        assert!(!healthy);
        assert_eq!(license.edition(), "community");
        assert!(!license.verified());
    }

    #[tokio::test]
    async fn license_store_failure_degrades_to_community_without_blocking_startup() {
        let repository = TestLicenseVersions {
            active: None,
            fail_load: true,
        };

        let (license, healthy) = build_license(
            &repository,
            &Id::from_string("system-org"),
            &Settings::default(),
        )
        .await;

        assert!(!healthy);
        assert_eq!(license.edition(), "community");
    }

    #[test]
    fn build_parquet_disk_cache_default_settings_is_some_and_creates_dir() {
        let tmp = TempDir::new().expect("tmpdir");
        let dir = tmp.path().join("parquet");
        // 默认 [cache.disk_cache] 是 enabled=true / max_size_gb=10；改 dir 到 tmpdir。
        let cache_settings = CacheSettings {
            disk_cache: crate::config::DiskCacheSettings {
                dir: dir.clone(),
                ..Default::default()
            },
            ..Default::default()
        };
        let cache = build_parquet_disk_cache(&cache_settings.disk_cache)
            .expect("must build")
            .expect("default settings must yield Some(cache)");
        assert_eq!(cache.dir(), dir.as_path());
        // ParquetDiskCache::new 应当已经 mkdir -p。
        assert!(dir.exists(), "cache directory should be auto-created");

        // bootstrap 装配：把 cache 喂给 ProductionObjectStore.with_disk_cache，应当持有 Some(_)
        let inner = Arc::new(InMemory::new()) as Arc<dyn object_store::ObjectStore>;
        let wrapped = ProductionObjectStore::wrap(inner, ObjectStoreSettings::default());
        let with_cache = (*wrapped).clone().with_disk_cache(cache);
        assert!(
            with_cache.disk_cache().is_some(),
            "ProductionObjectStore must carry the injected disk_cache"
        );
    }

    #[test]
    fn build_parquet_disk_cache_disabled_returns_none_and_skips_dir() {
        let tmp = TempDir::new().expect("tmpdir");
        let dir = tmp.path().join("never_created");
        let cache_settings = CacheSettings {
            disk_cache: crate::config::DiskCacheSettings {
                dir: dir.clone(),
                max_size_gb: 0, // 0 = 关闭
            },
            ..Default::default()
        };
        let cache =
            build_parquet_disk_cache(&cache_settings.disk_cache).expect("disabled must Ok-return");
        assert!(cache.is_none(), "enabled=false must skip cache");
        assert!(!dir.exists(), "directory must not be created when disabled");
    }

    #[test]
    fn build_parquet_disk_cache_zero_size_is_effectively_disabled() {
        let tmp = TempDir::new().expect("tmpdir");
        let cache_settings = CacheSettings {
            disk_cache: crate::config::DiskCacheSettings {
                dir: tmp.path().join("zero"),
                max_size_gb: 0,
            },
            ..Default::default()
        };
        let cache =
            build_parquet_disk_cache(&cache_settings.disk_cache).expect("zero must Ok-return");
        assert!(cache.is_none(), "max_size_gb=0 must skip cache");
    }

    #[test]
    fn build_fsync_policy_defaults_to_batch_data_50ms_64() {
        let wal = WalSettings::default();
        let p = build_fsync_policy(&wal);
        match p {
            FsyncPolicy::Batch {
                max_pending,
                max_delay_ms,
                sync_level,
            } => {
                assert_eq!(max_pending, 64);
                assert_eq!(max_delay_ms, 50);
                assert!(sync_level.is_data());
            }
            other => panic!("expected Batch, got {other:?}"),
        }
    }

    #[test]
    fn build_fsync_policy_none_strategy_yields_none_variant() {
        let wal = WalSettings {
            flush_strategy: WalFlushStrategy::None,
            sync_level: WalSyncLevel::None,
            ..Default::default()
        };
        match build_fsync_policy(&wal) {
            FsyncPolicy::None { sync_level } => assert!(sync_level.is_none()),
            other => panic!("expected None variant, got {other:?}"),
        }
    }

    #[test]
    fn build_fsync_policy_every_write_with_all_sync() {
        let wal = WalSettings {
            flush_strategy: WalFlushStrategy::EveryWrite,
            sync_level: WalSyncLevel::All,
            ..Default::default()
        };
        match build_fsync_policy(&wal) {
            FsyncPolicy::EveryWrite { sync_level } => assert!(sync_level.is_all()),
            other => panic!("expected EveryWrite, got {other:?}"),
        }
    }
}
