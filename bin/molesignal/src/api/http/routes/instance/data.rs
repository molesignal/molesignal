// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 数据的平台设置：数据流没有设置保留天数时保留多久、拨测结果保留多久、Prometheus 写入的活跃
//! series 上限，以及 APM 的基数上限。都在平台组织的设置页修改（`sys.settings.retention.*`、`sys.settings.limits.*`），改动
//! 记入平台审计。调短默认保留期之前可以先看清理预览（[`retention_preview`]）。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        apm::ApmLimits,
        iam::{
            MAX_ACTIVE_SERIES_LIMIT, MAX_STREAM_RETENTION_DAYS,
            MAX_SYNTHETIC_RESULT_RETENTION_DAYS, permission,
        },
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod intake_usage;
mod retention_preview;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/settings/stream_retention",
            get(get_stream_retention).put(put_stream_retention),
        )
        .route(
            "/settings/stream_retention/preview",
            get(retention_preview::handle),
        )
        .route(
            "/settings/synthetic_retention",
            get(get_synthetic_retention).put(put_synthetic_retention),
        )
        .route(
            "/settings/series_limits",
            get(get_series_limits).put(put_series_limits),
        )
        .route(
            "/settings/apm_limits",
            get(get_apm_limits).put(put_apm_limits),
        )
        .route(
            "/settings/intake_usage",
            get(intake_usage::get).put(intake_usage::put),
        )
}

/// 数据流没有设置保留天数时保留的天数：compactor 与分区清单维护每一轮读一次。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRetentionSettings {
    pub retention_days: u32,
}

/// 拨测结果保留的天数：compactor 主节点每天清理更早的结果、它们的附件与已结束的任务；0 = 永久
/// 保留。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticRetentionSettings {
    pub retention_days: u32,
}

/// Prometheus 写入的活跃 series 上限：一个组织的，与组织里一个指标的。各个 intake 节点每隔
/// 半分钟读一次；改它的节点立即生效。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesLimitSettings {
    pub max_active_series_per_org: u32,
    pub max_active_series_per_metric: u32,
}

fn validate_stream_retention_days(days: u32) -> Result<()> {
    if !(1..=MAX_STREAM_RETENTION_DAYS).contains(&days) {
        return Err(Error::invalid(format!(
            "retention_days must be between 1 and {MAX_STREAM_RETENTION_DAYS}"
        )));
    }

    Ok(())
}

#[permission("sys.settings.retention.read")]
async fn get_stream_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<StreamRetentionSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(StreamRetentionSettings {
        retention_days: settings.default_retention_days,
    }))
}

#[permission("sys.settings.retention.manage")]
async fn put_stream_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<StreamRetentionSettings>,
) -> Result<Json<StreamRetentionSettings>> {
    validate_stream_retention_days(request.retention_days)?;

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.default_retention_days;
    settings.default_retention_days = request.retention_days;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    if updated.default_retention_days != previous {
        audit(
            &state,
            &ctx,
            "stream_retention.update",
            json!({
                "retentionDays": updated.default_retention_days,
                "previousRetentionDays": previous,
            }),
        )
        .await;
    }

    Ok(Json(StreamRetentionSettings {
        retention_days: updated.default_retention_days,
    }))
}

#[permission("sys.settings.retention.read")]
async fn get_synthetic_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SyntheticRetentionSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(SyntheticRetentionSettings {
        retention_days: settings.synthetic_result_retention_days,
    }))
}

#[permission("sys.settings.retention.manage")]
async fn put_synthetic_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<SyntheticRetentionSettings>,
) -> Result<Json<SyntheticRetentionSettings>> {
    if request.retention_days > MAX_SYNTHETIC_RESULT_RETENTION_DAYS {
        return Err(Error::invalid(format!(
            "retention_days must be between 0 and {MAX_SYNTHETIC_RESULT_RETENTION_DAYS}"
        )));
    }

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.synthetic_result_retention_days;
    settings.synthetic_result_retention_days = request.retention_days;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    if updated.synthetic_result_retention_days != previous {
        audit(
            &state,
            &ctx,
            "synthetic_retention.update",
            json!({
                "retentionDays": updated.synthetic_result_retention_days,
                "previousRetentionDays": previous,
            }),
        )
        .await;
    }

    Ok(Json(SyntheticRetentionSettings {
        retention_days: updated.synthetic_result_retention_days,
    }))
}

#[permission("sys.settings.limits.read")]
async fn get_series_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SeriesLimitSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(SeriesLimitSettings {
        max_active_series_per_org: settings.max_active_series_per_org,
        max_active_series_per_metric: settings.max_active_series_per_metric,
    }))
}

#[permission("sys.settings.limits.manage")]
async fn put_series_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<SeriesLimitSettings>,
) -> Result<Json<SeriesLimitSettings>> {
    let (per_org, per_metric) = (
        request.max_active_series_per_org,
        request.max_active_series_per_metric,
    );

    if per_metric == 0 || per_org < per_metric || per_org > MAX_ACTIVE_SERIES_LIMIT {
        return Err(Error::invalid(format!(
            "series limits must keep 1 <= max_active_series_per_metric <= \
             max_active_series_per_org <= {MAX_ACTIVE_SERIES_LIMIT}"
        )));
    }

    let mut settings = state.iam.instance_settings.get().await?;

    let previous = (
        settings.max_active_series_per_org,
        settings.max_active_series_per_metric,
    );

    settings.max_active_series_per_org = per_org;
    settings.max_active_series_per_metric = per_metric;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    // 本节点立即生效；其它节点由后台刷新读到。
    state.telemetry.prometheus_series_admission.set_limits(
        updated.max_active_series_per_org as usize,
        updated.max_active_series_per_metric as usize,
    );

    if (per_org, per_metric) != previous {
        audit(
            &state,
            &ctx,
            "series_limits.update",
            json!({
                "maxActiveSeriesPerOrg": per_org,
                "maxActiveSeriesPerMetric": per_metric,
                "previousMaxActiveSeriesPerOrg": previous.0,
                "previousMaxActiveSeriesPerMetric": previous.1,
            }),
        )
        .await;
    }

    Ok(Json(SeriesLimitSettings {
        max_active_series_per_org: updated.max_active_series_per_org,
        max_active_series_per_metric: updated.max_active_series_per_metric,
    }))
}

#[permission("sys.settings.limits.read")]
async fn get_apm_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ApmLimits>> {
    Ok(Json(state.iam.instance_settings.get().await?.apm_limits))
}

/// APM 每小时最多记下多少个服务、端点、依赖等，超出的计入溢出。各个投影节点每隔半分钟读一次；
/// 改它的节点立即生效。
#[permission("sys.settings.limits.manage")]
async fn put_apm_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ApmLimits>,
) -> Result<Json<ApmLimits>> {
    request.validate()?;

    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.apm_limits;
    settings.apm_limits = request;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    if let Some(limits) = state
        .telemetry
        .apm_runtime
        .as_ref()
        .and_then(|runtime| runtime.limits())
    {
        *limits.write() = updated.apm_limits;
    }

    if updated.apm_limits != previous {
        audit(
            &state,
            &ctx,
            "apm_limits.update",
            json!({
                "limits": updated.apm_limits,
                "previousLimits": previous,
            }),
        )
        .await;
    }

    Ok(Json(updated.apm_limits))
}

/// 平台设置的改动记入平台审计。记不下来时只记一条警告：设置已经保存了。
pub(super) async fn audit(state: &AppState, ctx: &IamContext, action: &str, payload: Value) {
    let event = AuditEvent {
        id: Id::new(),
        org_id: state.iam.system_org_id.clone(),
        actor_kind: "user".into(),
        actor_id: ctx.user_id.0.clone(),
        action: action.into(),
        target_kind: Some("instance_settings".into()),
        target_id: None,
        ip: None,
        user_agent: None,
        payload,
        ts: TimestampMicros::now(),
    };

    if let Err(error) = state.iam.audit_events.record(event).await {
        tracing::warn!(%error, action, "instance setting change could not be audited");
    }
}
