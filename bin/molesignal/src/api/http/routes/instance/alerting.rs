// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警的平台设置：分角色集群里，规则没设评估延迟时往回挪多少秒。在平台组织的设置页修改
//! （`sys.settings.alerting.*`），改动记入平台审计；各告警节点每分钟读一次。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::data::audit;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{alerting::rule::MAX_EVALUATION_DELAY_SECS, iam::permission},
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/settings/alert_evaluation",
        get(get_alert_evaluation).put(put_alert_evaluation),
    )
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertEvaluationSettings {
    /// 0 到 3600 秒。
    pub evaluation_delay_secs: u32,
}

#[permission("sys.settings.alerting.read")]
async fn get_alert_evaluation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<AlertEvaluationSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(AlertEvaluationSettings {
        evaluation_delay_secs: settings.alert_evaluation_delay_secs,
    }))
}

#[permission("sys.settings.alerting.manage")]
async fn put_alert_evaluation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<AlertEvaluationSettings>,
) -> Result<Json<AlertEvaluationSettings>> {
    if request.evaluation_delay_secs > MAX_EVALUATION_DELAY_SECS {
        return Err(Error::invalid(format!(
            "evaluation_delay_secs must be between 0 and {MAX_EVALUATION_DELAY_SECS}"
        )));
    }

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.alert_evaluation_delay_secs;
    settings.alert_evaluation_delay_secs = request.evaluation_delay_secs;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    if updated.alert_evaluation_delay_secs != previous {
        audit(
            &state,
            &ctx,
            "alert_evaluation.update",
            json!({
                "evaluationDelaySecs": updated.alert_evaluation_delay_secs,
                "previousEvaluationDelaySecs": previous,
            }),
        )
        .await;
    }

    Ok(Json(AlertEvaluationSettings {
        evaluation_delay_secs: updated.alert_evaluation_delay_secs,
    }))
}
