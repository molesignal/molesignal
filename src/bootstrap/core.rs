// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bootstrap 基础设施：存储连接、基础 repository、加密与节点角色规划。

use std::sync::Arc;

use object_store::ObjectStore;

use crate::{
    app::cluster::{ClusterRegistry, PeerRole},
    config::{Role, Settings},
    domain::{
        iam::{IamPlatformAdministratorRepository, Organization},
        license::LicenseVersionRepository,
        storage::ParquetFileMetaDumpRepository,
        trace_policy::{TraceDebugTokenRepository, TracePolicyRepository},
    },
    infra::{
        caching::{OrgSchemaCache, ParquetFileMetaDumpCache},
        cipher::{CipherKeyRepository, CipherRootKey, FieldKeyService, PgCipherKeyRepository},
        masking::MaskingService,
        persistence::{
            MetaStore,
            repositories::{
                alert_rules::PgAlertRuleRepository,
                cluster::nodes::{
                    ClusterNodesRepository, PgClusterNodesRepository, PgClusterRegistry,
                },
                dashboard_authoring::PgDashboardDraftRepository,
                dashboard_contract_registry::PgDashboardContractRepository,
                dashboards::PgDashboardRepository,
                escalation_policies::PgEscalationPolicyRepository,
                folders::PgFolderRepository,
                iam::{
                    memberships::PgIamMembershipRepository,
                    platform_administrators::PgIamPlatformAdministratorRepository,
                },
                incidents::PgIncidentRepository,
                license_versions::PgLicenseVersionRepository,
                organizations::PgOrganizationRepository,
                parquet_file_meta::PgParquetFileMetaRepository,
                password_resets::{PasswordResetRepository, PgPasswordResetRepository},
                saved_views::PgSavedViewRepository,
                schedules::PgScheduleRepository,
                streams::PgStreamRepository,
                teams::PgTeamRepository,
                trace_policies::{PgTraceDebugTokenRepository, PgTracePolicyRepository},
                users::PgUserRepository,
            },
        },
        storage::object::{self, production::ProductionObjectStore},
    },
    shared::{Result, drain::DrainController},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct RolePlan {
    pub(super) run_ingester: bool,
    pub(super) run_compactor: bool,
    pub(super) run_alert_manager: bool,
    pub(super) run_querier: bool,
}

pub(super) struct Core {
    pub(super) pool: sqlx::PgPool,
    pub(super) store: Arc<dyn ObjectStore>,
    pub(super) orgs: Arc<PgOrganizationRepository>,
    pub(super) system_org: Organization,
    pub(super) users: Arc<PgUserRepository>,
    pub(super) password_resets: Arc<dyn PasswordResetRepository>,
    pub(super) iam_memberships: Arc<PgIamMembershipRepository>,
    pub(super) iam_platform_administrators: Arc<dyn IamPlatformAdministratorRepository>,
    pub(super) license_versions: Arc<dyn LicenseVersionRepository>,
    pub(super) trace_policies: Arc<dyn TracePolicyRepository>,
    pub(super) trace_debug_tokens: Arc<dyn TraceDebugTokenRepository>,
    pub(super) teams: Arc<PgTeamRepository>,
    pub(super) org_schema_cache: Arc<OrgSchemaCache>,
    pub(super) streams: Arc<PgStreamRepository>,
    pub(super) parquet_file_meta_dump_cache: Arc<ParquetFileMetaDumpCache>,
    pub(super) parquet_file_meta: Arc<PgParquetFileMetaRepository>,
    pub(super) folders: Arc<PgFolderRepository>,
    pub(super) dashboards: Arc<PgDashboardRepository>,
    pub(super) dashboard_drafts: Arc<PgDashboardDraftRepository>,
    pub(super) dashboard_contracts: Arc<PgDashboardContractRepository>,
    pub(super) saved_views: Arc<PgSavedViewRepository>,
    pub(super) alert_rules: Arc<PgAlertRuleRepository>,
    pub(super) incidents: Arc<PgIncidentRepository>,
    pub(super) schedules: Arc<PgScheduleRepository>,
    pub(super) escalations: Arc<PgEscalationPolicyRepository>,
    pub(super) email_sender: Option<Arc<crate::infra::notify::EmailSender>>,
    pub(super) cipher_root_key: CipherRootKey,
    pub(super) cipher_keys: Arc<dyn CipherKeyRepository>,
    pub(super) field_key_service: Arc<FieldKeyService>,
    pub(super) regex_patterns:
        Arc<dyn crate::infra::persistence::repositories::regex_patterns::RegexPatternRepository>,
    pub(super) masking_service: Arc<MaskingService>,
    pub(super) field_masking_rules: Arc<dyn crate::domain::masking::FieldMaskingRuleRepository>,
    pub(super) field_masking_service: Arc<crate::infra::masking::FieldMaskingService>,
    pub(super) cluster_repo: Arc<dyn ClusterNodesRepository>,
    pub(super) registry: Arc<dyn ClusterRegistry>,
    pub(super) drain_controller: Arc<DrainController>,
    pub(super) node_id: String,
    pub(super) roles: RolePlan,
}

impl Core {
    pub(super) async fn build(settings: &Settings) -> Result<Self> {
        let meta = MetaStore::connect(&settings.store.meta).await?;
        let pool = meta.pool.clone();

        let credentials =
            crate::infra::storage::object::credentials::resolve(&settings.store.object)?;
        tracing::info!(
            object_store_credentials_source = credentials.source.as_str(),
            "resolved object_store credentials"
        );
        let raw_store = object::build(&settings.store.object)?;
        let store: Arc<dyn ObjectStore> =
            ProductionObjectStore::wrap(raw_store, settings.store.object.clone());
        crate::bootstrap::roles::health_probe::startup_ping(store.as_ref()).await?;

        let orgs = Arc::new(PgOrganizationRepository::new(pool.clone()));
        let system_org = match orgs.ensure_system_organization().await {
            Ok(system_org) => {
                crate::shared::trace_metrics::set_system_load("system_org", true);
                system_org
            }
            Err(error) => {
                crate::shared::trace_metrics::set_system_load("system_org", false);
                return Err(error);
            }
        };
        let users = Arc::new(PgUserRepository::new(pool.clone()));
        let password_resets: Arc<dyn PasswordResetRepository> =
            Arc::new(PgPasswordResetRepository::new(pool.clone()));
        let iam_memberships = Arc::new(PgIamMembershipRepository::new(pool.clone()));
        let iam_platform_administrators: Arc<dyn IamPlatformAdministratorRepository> =
            Arc::new(PgIamPlatformAdministratorRepository::new(pool.clone()));
        let license_versions: Arc<dyn LicenseVersionRepository> =
            Arc::new(PgLicenseVersionRepository::new(pool.clone()));
        let trace_policies: Arc<dyn TracePolicyRepository> =
            Arc::new(PgTracePolicyRepository::new(pool.clone()));
        let trace_debug_tokens: Arc<dyn TraceDebugTokenRepository> =
            Arc::new(PgTraceDebugTokenRepository::new(pool.clone()));
        let teams = Arc::new(PgTeamRepository::new(pool.clone()));
        let org_schema_cache = Arc::new(OrgSchemaCache::new());
        let streams =
            Arc::new(PgStreamRepository::new(pool.clone()).with_cache(org_schema_cache.clone()));

        let parquet_file_meta_dump_repo: Arc<dyn ParquetFileMetaDumpRepository> = Arc::new(
            crate::infra::persistence::repositories::parquet_file_meta::dump::PgParquetFileMetaDumpRepository::new(
                pool.clone(),
            ),
        );
        let parquet_file_meta_dump_cache = Arc::new(ParquetFileMetaDumpCache::new(
            &settings.cache.parquet_file_meta_dump,
        ));
        let parquet_file_meta = {
            let mut repository = PgParquetFileMetaRepository::new(pool.clone());
            if settings.storage.parquet_file_meta_dump.enabled
                && settings
                    .storage
                    .parquet_file_meta_dump
                    .max_partitions_per_tick
                    > 0
            {
                repository = repository.with_dump_query(
                    crate::infra::persistence::repositories::parquet_file_meta::DumpQueryContext {
                        dump_repo: parquet_file_meta_dump_repo,
                        object_store: store.clone(),
                        cold_after_days: settings.storage.parquet_file_meta_dump.cold_after_days,
                        dump_cache: Some(parquet_file_meta_dump_cache.clone()),
                    },
                );
            }
            Arc::new(repository)
        };

        let folders = Arc::new(PgFolderRepository::new(pool.clone()));
        let dashboards = Arc::new(PgDashboardRepository::new(pool.clone()));
        let dashboard_drafts = Arc::new(PgDashboardDraftRepository::new(pool.clone()));
        let dashboard_contracts = Arc::new(PgDashboardContractRepository::new(pool.clone()));
        let saved_views = Arc::new(PgSavedViewRepository::new(pool.clone()));
        let alert_rules = Arc::new(PgAlertRuleRepository::new(pool.clone()));
        let incidents = Arc::new(PgIncidentRepository::new(pool.clone()));
        let schedules = Arc::new(PgScheduleRepository::new(pool.clone()));
        let escalations = Arc::new(PgEscalationPolicyRepository::new(pool.clone()));
        let email_sender = if settings.notify.smtp.host.trim().is_empty() {
            None
        } else {
            match crate::infra::notify::EmailSender::new(&settings.notify.smtp) {
                Ok(sender) => Some(Arc::new(sender)),
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "platform SMTP initialization failed; platform emails are disabled"
                    );
                    None
                }
            }
        };

        let cipher_root_key = CipherRootKey::from_env()
            .map_err(|error| {
                tracing::warn!(
                    error = %error,
                    "MS_CIPHER_KEY missing; falling back to all-zero key (DEV ONLY)"
                );
                error
            })
            .unwrap_or_else(|_| {
                const ZERO_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
                CipherRootKey::from_base64(ZERO_B64).expect("zero key constructs")
            });
        let cipher_keys: Arc<dyn CipherKeyRepository> = Arc::new(PgCipherKeyRepository::new(
            pool.clone(),
            cipher_root_key.clone(),
        ));
        let field_key_service = Arc::new(FieldKeyService::new(cipher_keys.clone()));
        let regex_patterns: Arc<
            dyn crate::infra::persistence::repositories::regex_patterns::RegexPatternRepository,
        > = Arc::new(
            crate::infra::persistence::repositories::regex_patterns::PgRegexPatternRepository::new(
                pool.clone(),
            ),
        );
        let masking_service = Arc::new(MaskingService::new(regex_patterns.clone()));
        let field_masking_rules: Arc<dyn crate::domain::masking::FieldMaskingRuleRepository> =
            Arc::new(
                crate::infra::persistence::repositories::field_masking_rules::PgFieldMaskingRuleRepository::new(
                    pool.clone(),
                ),
            );
        let field_masking_service = Arc::new(crate::infra::masking::FieldMaskingService::new(
            field_masking_rules.clone(),
            streams.clone(),
            cipher_root_key.clone(),
        ));

        let cluster_repo: Arc<dyn ClusterNodesRepository> =
            Arc::new(PgClusterNodesRepository::new(pool.clone()));
        let registry: Arc<dyn ClusterRegistry> = Arc::new(PgClusterRegistry::new(
            cluster_repo.clone(),
            settings.cluster.advertise_addr.clone(),
            settings.cluster.peer_timeout_secs as i64,
        ));
        let drain_controller = Arc::new(DrainController::new());
        let node_id = if settings.node.id.is_empty() {
            crate::shared::ids::Id::new().0
        } else {
            settings.node.id.clone()
        };
        let is_standalone = settings.node.roles.contains(&Role::Standalone);
        let roles = RolePlan {
            run_ingester: is_standalone || settings.node.roles.contains(&Role::Ingester),
            run_compactor: is_standalone || settings.node.roles.contains(&Role::Compactor),
            run_alert_manager: is_standalone || settings.node.roles.contains(&Role::AlertManager),
            run_querier: is_standalone || settings.node.roles.contains(&Role::Querier),
        };

        Ok(Self {
            pool,
            store,
            orgs,
            system_org,
            users,
            password_resets,
            iam_memberships,
            iam_platform_administrators,
            license_versions,
            trace_policies,
            trace_debug_tokens,
            teams,
            org_schema_cache,
            streams,
            parquet_file_meta_dump_cache,
            parquet_file_meta,
            folders,
            dashboards,
            dashboard_drafts,
            dashboard_contracts,
            saved_views,
            alert_rules,
            incidents,
            schedules,
            escalations,
            email_sender,
            cipher_root_key,
            cipher_keys,
            field_key_service,
            regex_patterns,
            masking_service,
            field_masking_rules,
            field_masking_service,
            cluster_repo,
            registry,
            drain_controller,
            node_id,
            roles,
        })
    }

    pub(super) fn spawn_cluster_maintenance(&self, settings: &Settings) {
        let mut peer_roles: Vec<PeerRole> = settings
            .node
            .roles
            .iter()
            .map(|role| match role {
                Role::Router => PeerRole::Router,
                Role::Ingester => PeerRole::Ingester,
                Role::Querier => PeerRole::Querier,
                Role::Compactor => PeerRole::Compactor,
                Role::AlertManager => PeerRole::AlertManager,
                Role::Standalone => PeerRole::Standalone,
            })
            .collect();
        if peer_roles.is_empty() {
            peer_roles.push(PeerRole::Standalone);
        }
        let _heartbeat = crate::bootstrap::roles::heartbeat::HeartbeatTask::new(
            self.cluster_repo.clone(),
            self.node_id.clone(),
            peer_roles,
            settings.cluster.advertise_addr.clone(),
            settings.cluster.heartbeat_interval_secs,
        )
        .with_drain(self.drain_controller.clone())
        .spawn();
        let _sweeper = crate::bootstrap::roles::heartbeat::spawn_sweeper(self.cluster_repo.clone());
    }
}
