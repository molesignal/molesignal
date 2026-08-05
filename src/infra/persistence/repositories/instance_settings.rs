// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `instance_settings` 表 CRUD —— 实例级（全局，非 per-org）设置的单行单例。
//! 当前承载控制面设置以及 RUM 客户端 IP 识别策略。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::iam::{
        ClientIpMode, ClientIpResolverSettings, InstanceSettings, InstanceSettingsRepository,
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
                    rum_client_ip_max_chain_length, updated_at_micros
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
                updated_at: TimestampMicros(r.try_get("updated_at_micros").map_err(sqlx_err)?),
            }),
            // 行缺失（迁移未 seed）时回落保守默认：关闭注册、服务图走 ingest。
            None => Ok(InstanceSettings {
                signup_enabled: false,
                signup_require_approval: true,
                service_graph_source: "ingest".to_string(),
                federation_cluster_id: String::new(),
                federation_drain_interval_secs: 10,
                federation_push_batch_size: 100,
                federation_seen_events_ttl_secs: 604_800,
                federation_gossip_interval_secs: 60,
                rum_client_ip_resolver: ClientIpResolverSettings::default(),
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
                 rum_client_ip_max_chain_length, updated_at_micros)
             VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
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
                 updated_at_micros = EXCLUDED.updated_at_micros",
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
        .bind(s.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(s)
    }
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
