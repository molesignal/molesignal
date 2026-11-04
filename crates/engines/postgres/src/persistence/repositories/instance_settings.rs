// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `instance_settings` 表 CRUD —— 实例级（全局，非 per-org）设置的单行单例。
//! 当前承载控制面设置、RUM 客户端 IP 识别策略、审计记录与数据流的保留天数，以及 Prometheus
//! 写入的活跃 series 上限。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::{
        apm::ApmLimits,
        iam::{
            ClientIpMode, ClientIpResolverSettings, DEFAULT_ALERT_EVALUATION_DELAY_SECS,
            DEFAULT_AUDIT_RETENTION_DAYS, DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG,
            DEFAULT_INTAKE_USAGE_RETENTION_DAYS, DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC,
            DEFAULT_MAX_ACTIVE_SERIES_PER_ORG, DEFAULT_STREAM_RETENTION_DAYS,
            DEFAULT_SYNTHETIC_RESULT_RETENTION_DAYS, InstanceSettings, InstanceSettingsRepository,
            SigninLimits,
        },
    },
    shared::{Result, time::TimestampMicros},
};

pub struct PgInstanceSettingsRepository {
    pool: PgPool,
}

impl PgInstanceSettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InstanceSettingsRepository for PgInstanceSettingsRepository {
    async fn get(&self) -> Result<InstanceSettings> {
        let row = sqlx::query(
            "SELECT signup_enabled, signup_require_approval, service_graph_source,
                    federation_cluster_id, federation_drain_interval_secs, federation_push_batch_size,
                    federation_seen_events_ttl_secs, federation_gossip_interval_secs,
                    rum_client_ip_mode, rum_client_ip_header, rum_client_ip_trusted_proxy_cidrs,
                    rum_client_ip_fallback_to_peer, rum_client_ip_allow_private,
                    rum_client_ip_max_chain_length, audit_retention_days, default_retention_days,
                    synthetic_result_retention_days, max_active_series_per_org,
                    max_active_series_per_metric, apm_services_per_org_hour, apm_transactions_per_service_hour,
                    apm_dependencies_per_service_hour, apm_operations_per_dependency_hour,
                    apm_error_groups_per_service_hour, apm_versions_per_service_hour,
                    apm_instances_per_service_hour, alert_evaluation_delay_secs,
                    intake_usage_retention_days, intake_usage_max_services_per_org,
                    signin_limit_account_source_failures, signin_limit_source_failures,
                    signin_limit_account_failures, signin_limit_account_interval_secs,
                    signin_limit_lock_minutes, updated_at
             FROM instance_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        match row {
            Some(r) => Ok(InstanceSettings {
                signup_enabled: r.try_get("signup_enabled").map_err(sqlx_err)?,
                signup_require_approval: r.try_get("signup_require_approval").map_err(sqlx_err)?,
                service_graph_source: r.try_get("service_graph_source").map_err(sqlx_err)?,
                federation_cluster_id: r.try_get("federation_cluster_id").map_err(sqlx_err)?,
                federation_drain_interval_secs: r
                    .try_get("federation_drain_interval_secs")
                    .map_err(sqlx_err)?,
                federation_push_batch_size: r
                    .try_get("federation_push_batch_size")
                    .map_err(sqlx_err)?,
                federation_seen_events_ttl_secs: r
                    .try_get("federation_seen_events_ttl_secs")
                    .map_err(sqlx_err)?,
                federation_gossip_interval_secs: r
                    .try_get("federation_gossip_interval_secs")
                    .map_err(sqlx_err)?,
                rum_client_ip_resolver: ClientIpResolverSettings {
                    mode: parse_client_ip_mode(
                        r.try_get::<String, _>("rum_client_ip_mode")
                            .map_err(sqlx_err)?,
                    )?,
                    header_name: r.try_get("rum_client_ip_header").map_err(sqlx_err)?,
                    trusted_proxy_cidrs: r
                        .try_get("rum_client_ip_trusted_proxy_cidrs")
                        .map_err(sqlx_err)?,
                    fallback_to_peer: r
                        .try_get("rum_client_ip_fallback_to_peer")
                        .map_err(sqlx_err)?,
                    allow_private_client_ips: r
                        .try_get("rum_client_ip_allow_private")
                        .map_err(sqlx_err)?,
                    max_chain_length: r
                        .try_get::<i16, _>("rum_client_ip_max_chain_length")
                        .map_err(sqlx_err)? as u16,
                },
                // CHECK 约束保证 0..=3650。
                audit_retention_days: r
                    .try_get::<i32, _>("audit_retention_days")
                    .map_err(sqlx_err)?
                    .max(0) as u32,
                // CHECK 约束保证 1..=3650，series 上限为正。
                default_retention_days: positive(&r, "default_retention_days")?,
                // CHECK 约束保证 0..=3650。
                synthetic_result_retention_days: r
                    .try_get::<i32, _>("synthetic_result_retention_days")
                    .map_err(sqlx_err)?
                    .max(0) as u32,
                intake_usage_retention_days: positive(&r, "intake_usage_retention_days")?,
                intake_usage_max_services_per_org: positive(
                    &r,
                    "intake_usage_max_services_per_org",
                )?,
                max_active_series_per_org: positive(&r, "max_active_series_per_org")?,
                max_active_series_per_metric: positive(&r, "max_active_series_per_metric")?,
                apm_limits: ApmLimits {
                    services_per_org_hour: positive(&r, "apm_services_per_org_hour")? as usize,
                    transactions_per_service_hour: positive(
                        &r,
                        "apm_transactions_per_service_hour",
                    )? as usize,
                    dependencies_per_service_hour: positive(
                        &r,
                        "apm_dependencies_per_service_hour",
                    )? as usize,
                    operations_per_dependency_hour: positive(
                        &r,
                        "apm_operations_per_dependency_hour",
                    )? as usize,
                    error_groups_per_service_hour: positive(
                        &r,
                        "apm_error_groups_per_service_hour",
                    )? as usize,
                    versions_per_service_hour: positive(&r, "apm_versions_per_service_hour")?
                        as usize,
                    instances_per_service_hour: positive(&r, "apm_instances_per_service_hour")?
                        as usize,
                },
                // CHECK 约束保证 0..=3600。
                alert_evaluation_delay_secs: r
                    .try_get::<i32, _>("alert_evaluation_delay_secs")
                    .map_err(sqlx_err)?
                    .max(0) as u32,
                // CHECK 约束保证次数不为负，秒数和分钟数为正。
                signin_limits: SigninLimits {
                    account_source_failures: r
                        .try_get::<i32, _>("signin_limit_account_source_failures")
                        .map_err(sqlx_err)?
                        .max(0) as u32,
                    source_failures: r
                        .try_get::<i32, _>("signin_limit_source_failures")
                        .map_err(sqlx_err)?
                        .max(0) as u32,
                    account_failures: r
                        .try_get::<i32, _>("signin_limit_account_failures")
                        .map_err(sqlx_err)?
                        .max(0) as u32,
                    account_interval_secs: positive(&r, "signin_limit_account_interval_secs")?,
                    lock_minutes: positive(&r, "signin_limit_lock_minutes")?,
                },
                updated_at: r
                    .try_get::<TimestampMicros, _>("updated_at")
                    .map_err(sqlx_err)?,
            }),
            // 行缺失（迁移未 seed）时回落保守默认：关闭注册、服务图走 intake。
            None => Ok(InstanceSettings {
                signup_enabled: false,
                signup_require_approval: true,
                service_graph_source: "intake".to_string(),
                federation_cluster_id: String::new(),
                federation_drain_interval_secs: 10,
                federation_push_batch_size: 100,
                federation_seen_events_ttl_secs: 604_800,
                federation_gossip_interval_secs: 60,
                rum_client_ip_resolver: ClientIpResolverSettings::default(),
                audit_retention_days: DEFAULT_AUDIT_RETENTION_DAYS,
                default_retention_days: DEFAULT_STREAM_RETENTION_DAYS,
                synthetic_result_retention_days: DEFAULT_SYNTHETIC_RESULT_RETENTION_DAYS,
                max_active_series_per_org: DEFAULT_MAX_ACTIVE_SERIES_PER_ORG,
                max_active_series_per_metric: DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC,
                apm_limits: ApmLimits::default(),
                alert_evaluation_delay_secs: DEFAULT_ALERT_EVALUATION_DELAY_SECS,
                intake_usage_retention_days: DEFAULT_INTAKE_USAGE_RETENTION_DAYS,
                intake_usage_max_services_per_org: DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG,
                signin_limits: SigninLimits::default(),
                updated_at: TimestampMicros(0),
            }),
        }
    }

    async fn update(&self, s: InstanceSettings) -> Result<InstanceSettings> {
        sqlx::query(
            "INSERT INTO instance_settings
                (id, signup_enabled, signup_require_approval, service_graph_source,
                 federation_cluster_id, federation_drain_interval_secs, federation_push_batch_size,
                 federation_seen_events_ttl_secs, federation_gossip_interval_secs,
                 rum_client_ip_mode, rum_client_ip_header, rum_client_ip_trusted_proxy_cidrs,
                 rum_client_ip_fallback_to_peer, rum_client_ip_allow_private,
                 rum_client_ip_max_chain_length, audit_retention_days, default_retention_days,
                 max_active_series_per_org, max_active_series_per_metric, updated_at,
                 apm_services_per_org_hour, apm_transactions_per_service_hour,
                 apm_dependencies_per_service_hour, apm_operations_per_dependency_hour,
                 apm_error_groups_per_service_hour, apm_versions_per_service_hour,
                 apm_instances_per_service_hour, synthetic_result_retention_days,
                 alert_evaluation_delay_secs, intake_usage_retention_days,
                 intake_usage_max_services_per_org, signin_limit_account_source_failures,
                 signin_limit_source_failures, signin_limit_account_failures,
                 signin_limit_account_interval_secs, signin_limit_lock_minutes)
             VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30,
                     $31, $32, $33, $34, $35)
             ON CONFLICT (id) DO UPDATE
             SET signup_enabled = EXCLUDED.signup_enabled,
                 signup_require_approval = EXCLUDED.signup_require_approval,
                 service_graph_source = EXCLUDED.service_graph_source,
                 federation_cluster_id = EXCLUDED.federation_cluster_id,
                 federation_drain_interval_secs = EXCLUDED.federation_drain_interval_secs,
                 federation_push_batch_size = EXCLUDED.federation_push_batch_size,
                 federation_seen_events_ttl_secs = EXCLUDED.federation_seen_events_ttl_secs,
                 federation_gossip_interval_secs = EXCLUDED.federation_gossip_interval_secs,
                 rum_client_ip_mode = EXCLUDED.rum_client_ip_mode,
                 rum_client_ip_header = EXCLUDED.rum_client_ip_header,
                 rum_client_ip_trusted_proxy_cidrs = EXCLUDED.rum_client_ip_trusted_proxy_cidrs,
                 rum_client_ip_fallback_to_peer = EXCLUDED.rum_client_ip_fallback_to_peer,
                 rum_client_ip_allow_private = EXCLUDED.rum_client_ip_allow_private,
                 rum_client_ip_max_chain_length = EXCLUDED.rum_client_ip_max_chain_length,
                 audit_retention_days = EXCLUDED.audit_retention_days,
                 default_retention_days = EXCLUDED.default_retention_days,
                 max_active_series_per_org = EXCLUDED.max_active_series_per_org,
                 max_active_series_per_metric = EXCLUDED.max_active_series_per_metric,
                 updated_at = EXCLUDED.updated_at,
                 apm_services_per_org_hour = EXCLUDED.apm_services_per_org_hour,
                 apm_transactions_per_service_hour = EXCLUDED.apm_transactions_per_service_hour,
                 apm_dependencies_per_service_hour = EXCLUDED.apm_dependencies_per_service_hour,
                 apm_operations_per_dependency_hour = EXCLUDED.apm_operations_per_dependency_hour,
                 apm_error_groups_per_service_hour = EXCLUDED.apm_error_groups_per_service_hour,
                 apm_versions_per_service_hour = EXCLUDED.apm_versions_per_service_hour,
                 apm_instances_per_service_hour = EXCLUDED.apm_instances_per_service_hour,
                 synthetic_result_retention_days = EXCLUDED.synthetic_result_retention_days,
                 alert_evaluation_delay_secs = EXCLUDED.alert_evaluation_delay_secs,
                 intake_usage_retention_days = EXCLUDED.intake_usage_retention_days,
                 intake_usage_max_services_per_org = EXCLUDED.intake_usage_max_services_per_org,
                 signin_limit_account_source_failures = EXCLUDED.signin_limit_account_source_failures,
                 signin_limit_source_failures = EXCLUDED.signin_limit_source_failures,
                 signin_limit_account_failures = EXCLUDED.signin_limit_account_failures,
                 signin_limit_account_interval_secs = EXCLUDED.signin_limit_account_interval_secs,
                 signin_limit_lock_minutes = EXCLUDED.signin_limit_lock_minutes",
        )
        .bind(s.signup_enabled)
        .bind(s.signup_require_approval)
        .bind(&s.service_graph_source)
        .bind(&s.federation_cluster_id)
        .bind(s.federation_drain_interval_secs)
        .bind(s.federation_push_batch_size)
        .bind(s.federation_seen_events_ttl_secs)
        .bind(s.federation_gossip_interval_secs)
        .bind(client_ip_mode_name(s.rum_client_ip_resolver.mode))
        .bind(&s.rum_client_ip_resolver.header_name)
        .bind(&s.rum_client_ip_resolver.trusted_proxy_cidrs)
        .bind(s.rum_client_ip_resolver.fallback_to_peer)
        .bind(s.rum_client_ip_resolver.allow_private_client_ips)
        .bind(s.rum_client_ip_resolver.max_chain_length as i16)
        .bind(i32::try_from(s.audit_retention_days).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.default_retention_days).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.max_active_series_per_org).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.max_active_series_per_metric).unwrap_or(i32::MAX))
        .bind(s.updated_at)
        .bind(i32::try_from(s.apm_limits.services_per_org_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.transactions_per_service_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.dependencies_per_service_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.operations_per_dependency_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.error_groups_per_service_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.versions_per_service_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.apm_limits.instances_per_service_hour).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.synthetic_result_retention_days).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.alert_evaluation_delay_secs).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.intake_usage_retention_days).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.intake_usage_max_services_per_org).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.signin_limits.account_source_failures).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.signin_limits.source_failures).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.signin_limits.account_failures).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.signin_limits.account_interval_secs).unwrap_or(i32::MAX))
        .bind(i32::try_from(s.signin_limits.lock_minutes).unwrap_or(i32::MAX))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(s)
    }
}

/// 一个按 CHECK 约束为正的 INTEGER 列。
fn positive(row: &sqlx::postgres::PgRow, column: &str) -> Result<u32> {
    let value = row.try_get::<i32, _>(column).map_err(sqlx_err)?;
    Ok(value.max(1) as u32)
}

fn client_ip_mode_name(mode: ClientIpMode) -> &'static str {
    match mode {
        ClientIpMode::Peer => "peer",
        ClientIpMode::Header => "header",
        ClientIpMode::ForwardedChain => "forwarded_chain",
    }
}

fn parse_client_ip_mode(value: String) -> Result<ClientIpMode> {
    match value.as_str() {
        "peer" => Ok(ClientIpMode::Peer),
        "header" => Ok(ClientIpMode::Header),
        "forwarded_chain" => Ok(ClientIpMode::ForwardedChain),
        other => Err(crate::shared::Error::internal(format!(
            "invalid rum client IP mode stored in instance_settings: {other}"
        ))),
    }
}
