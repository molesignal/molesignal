// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询准入的快照与运行时调整（`/query/admission`）。

use std::{collections::HashMap, time::Duration};

use axum::{Extension, Json, extract::State};
use domain::iam::{IamContext, permission};
use kernel::Result;
use serde::Deserialize;
use serde_json::Value;
use settings::AdmissionSettings;

use crate::{
    api::AppState,
    app::search::{AdmissionConfig, AdmissionController},
};

/// 各工作组的槽位快照：上限、执行中、排队中、累计放行与拒绝，以及排队最多等多久。
#[permission("sys.query.admission.read")]
pub(super) async fn stats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(snapshot(&state.query.admission())))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateReq {
    #[serde(default)]
    default_max_concurrent: Option<usize>,
    #[serde(default)]
    groups: HashMap<String, usize>,
    #[serde(default)]
    role_map: HashMap<String, String>,
    #[serde(default)]
    max_wait_secs: Option<u64>,
    #[serde(default)]
    cluster_default_max_concurrent: usize,
    #[serde(default)]
    cluster_groups: HashMap<String, usize>,
}

/// 热更新准入配置（无需重启；仅本节点、不持久化——重启回落配置文件）。整份替换：省略的字段取
/// 默认值，与配置文件里不写这一项相同。准入配额是整个节点的，所以是平台权限。
#[permission("sys.query.admission.manage")]
pub(super) async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Value>> {
    let defaults = AdmissionSettings::default();
    let admission = state.query.admission();

    admission.refresh(AdmissionConfig {
        default_max_concurrent: req
            .default_max_concurrent
            .unwrap_or(defaults.default_max_concurrent),
        groups: req.groups,
        role_map: req.role_map,
        max_wait: Duration::from_secs(req.max_wait_secs.unwrap_or(defaults.max_wait_secs)),
        cluster_default_max_concurrent: req.cluster_default_max_concurrent,
        cluster_groups: req.cluster_groups,
    });

    Ok(Json(snapshot(&admission)))
}

fn snapshot(admission: &AdmissionController) -> Value {
    serde_json::json!({
        "maxWaitSecs": admission.max_wait().as_secs(),
        "groups": admission.stats(),
    })
}
