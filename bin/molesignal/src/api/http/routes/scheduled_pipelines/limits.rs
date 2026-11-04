// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /scheduled_pipelines/{id}/limits`：实时流水线的配额与限流步骤今天（UTC）放行了多少、超出了
//! 多少，是所有节点数的合计。各节点每隔几秒写一次，所以比实际晚几秒。

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Serialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::resource_permission,
        pipeline::limit::{LimitUsage, utc_day},
    },
    infra::pipeline::ScheduledPipeline,
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResp {
    pub day_start: i64,
    /// One row per step and bucket, by step name then bucket.
    pub buckets: Vec<LimitBucketResp>,
}

#[derive(Debug, Serialize)]
pub struct LimitBucketResp {
    pub step: String,
    /// Empty for a quota that is not split and for a throttle; the values of the fields a quota is
    /// split by as a JSON list otherwise, `*` for the buckets past the cap.
    pub bucket: String,
    #[serde(flatten)]
    pub usage: LimitUsage,
}

#[resource_permission(
    action = "pipelines.read",
    resource = ScheduledPipeline,
    id = Id(id),
    bind = pipeline
)]
pub(super) async fn today(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<LimitsResp>> {
    let day = utc_day(TimestampMicros::now());

    let rows = state
        .platform
        .pipeline_steps
        .ledger()
        .of_pipeline(&pipeline.org_id, &pipeline.id, day)
        .await?;

    Ok(Json(LimitsResp {
        day_start: day,
        buckets: rows
            .into_iter()
            .map(|row| LimitBucketResp {
                step: row.key.step,
                bucket: row.bucket,
                usage: row.usage,
            })
            .collect(),
    }))
}
