// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled reports HTTP routes（spec scheduled-reports）。

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::Response,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::iam::{permission, resource_permission},
    infra::persistence::repositories::scheduled_reports::{ReportRecipient, ScheduledReport},
    shared::{
        Error, ReportFormat, Result, Viewport, ids::Id, time::TimestampMicros,
        validate_report_bytes,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/scheduled_reports", get(list).post(create))
        .route(
            "/scheduled_reports/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/scheduled_reports/{id}/deliveries", get(deliveries))
        .route("/scheduled_reports/{id}/preview", get(preview))
}

#[async_trait::async_trait]
impl ProtectedResource for ScheduledReport {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.platform.scheduled_reports.get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "report"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub dashboard_id: Option<String>,
    pub saved_view_id: Option<String>,
    pub cron: String,
    pub recipients: Vec<ReportRecipient>,
    pub format: String,
    #[serde(default)]
    pub time_range_json: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub dashboard_id: Option<String>,
    pub saved_view_id: Option<String>,
    pub cron: String,
    pub recipients: Vec<ReportRecipient>,
    pub format: String,
    #[serde(default)]
    pub time_range_json: Value,
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub name: String,
    pub dashboard_id: Option<String>,
    pub saved_view_id: Option<String>,
    pub cron: String,
    pub recipients: Vec<ReportRecipient>,
    pub format: String,
    pub time_range_json: Value,
    pub enabled: bool,
    pub last_run_at_micros: Option<i64>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(r: ScheduledReport) -> Resp {
    Resp {
        id: r.id.0,
        name: r.name,
        dashboard_id: r.dashboard_id.map(|i| i.0),
        saved_view_id: r.saved_view_id.map(|i| i.0),
        cron: r.cron,
        recipients: r.recipients,
        format: r.format,
        time_range_json: r.time_range_json,
        enabled: r.enabled,
        last_run_at_micros: r.last_run_at.map(|t| t.0),
        created_at_micros: r.created_at.0,
        updated_at_micros: r.updated_at.0,
    }
}

fn validate_req(
    dashboard_id: &Option<String>,
    saved_view_id: &Option<String>,
    format: &str,
    recipients: &[ReportRecipient],
) -> Result<()> {
    if dashboard_id.is_some() == saved_view_id.is_some() {
        // 两者都有或都没 → 拒收
        return Err(Error::invalid(
            "exactly one of dashboard_id or saved_view_id must be set",
        ));
    }
    if !matches!(format, "png" | "pdf" | "csv" | "svg" | "json") {
        return Err(Error::invalid(
            "format must be one of png | pdf | csv | svg | json",
        ));
    }
    if recipients.is_empty() {
        return Err(Error::invalid("at least one recipient required"));
    }
    Ok(())
}

#[permission("reports.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    Ok(Json(
        state
            .platform
            .scheduled_reports
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[resource_permission(
    action = "reports.read",
    resource = ScheduledReport,
    id = Id(id),
    bind = report
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    Ok(Json(to_resp(report)))
}

/// `GET /scheduled_reports/{id}/preview`：即时生成可下载报表。PDF/PNG 必须由
/// headless renderer 生成并通过 magic-byte 校验；不可用或失败时返回错误，绝不以
/// JSON/SVG 冒充目标文件。
#[resource_permission(
    action = "reports.read",
    resource = ScheduledReport,
    id = Id(id),
    bind = report
)]
async fn preview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let r = report;
    let body = if let Ok(format) = r.format.parse::<ReportFormat>() {
        let renderer = state.platform.report_renderer.as_ref().ok_or_else(|| {
            Error::unavailable("report PDF/PNG renderer is unavailable; verify the Chrome runtime")
        })?;
        let url = report_render_url(&state.platform.report_renderer_base_url, &r)?;
        let browser_auth_storage = browser_auth_storage(&state, &ctx)?;
        let bytes = renderer
            .render(
                &url,
                format,
                Viewport::default(),
                Some(&browser_auth_storage),
            )
            .await?;
        validate_report_bytes(format, &bytes)?;
        bytes.to_vec()
    } else {
        crate::infra::reporting::render_payload(&r)?
    };
    let content_type = crate::infra::reporting::content_type_for(&r.format);
    let disposition = format!("attachment; filename=\"report-{}.{}\"", r.id.0, r.format);
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_DISPOSITION, disposition)
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .map_err(|e| Error::internal(format!("preview response build: {e}")))
}

pub(crate) fn report_render_url(base_url: &str, report: &ScheduledReport) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::unavailable(
            "scheduled_reports.renderer.base_url is not configured",
        ));
    }
    if let Some(id) = &report.dashboard_id {
        return Ok(format!("{base}/dashboards/{}?report_render=1", id.0));
    }
    if let Some(id) = &report.saved_view_id {
        return Ok(format!("{base}/saved-views?view={}&report_render=1", id.0));
    }
    Err(Error::invalid(
        "report must reference a dashboard or saved view",
    ))
}

pub(crate) fn browser_auth_storage(state: &AppState, ctx: &IamContext) -> Result<String> {
    let token = state.iam.service.issue_token(&ctx.user_id, &ctx.org_id)?;
    serde_json::to_string(&serde_json::json!({
        "state": {
            "token": token,
            "ctx": {
                "user_id": ctx.user_id.0.clone(),
                "org_id": ctx.org_id.0.clone(),
                "display_role": ctx.display_role.clone(),
                "roles": ctx.roles.clone(),
                "scope": ctx.scope,
            }
        },
        "version": 0
    }))
    .map_err(|error| Error::internal(format!("serialize report browser session: {error}")))
}

#[permission("reports.schedule")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    validate_req(
        &req.dashboard_id,
        &req.saved_view_id,
        &req.format,
        &req.recipients,
    )?;
    let now = TimestampMicros::now();
    let r = ScheduledReport {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        dashboard_id: req.dashboard_id.map(Id),
        saved_view_id: req.saved_view_id.map(Id),
        cron: req.cron,
        recipients: req.recipients,
        format: req.format,
        time_range_json: req.time_range_json,
        enabled: req.enabled,
        last_run_at: None,
        created_at: now,
        updated_at: now,
    };
    let r = state.platform.scheduled_reports.create(r).await?;
    Ok(Json(to_resp(r)))
}

#[resource_permission(
    action = "reports.schedule",
    resource = ScheduledReport,
    id = Id(id),
    bind = report
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Resp>> {
    validate_req(
        &req.dashboard_id,
        &req.saved_view_id,
        &req.format,
        &req.recipients,
    )?;
    let existing = report;
    let r = ScheduledReport {
        id: existing.id,
        org_id: existing.org_id.clone(),
        name: req.name,
        dashboard_id: req.dashboard_id.map(Id),
        saved_view_id: req.saved_view_id.map(Id),
        cron: req.cron,
        recipients: req.recipients,
        format: req.format,
        time_range_json: req.time_range_json,
        enabled: req.enabled,
        last_run_at: existing.last_run_at,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let r = state.platform.scheduled_reports.update(r).await?;
    Ok(Json(to_resp(r)))
}

#[resource_permission(
    action = "reports.delete",
    resource = ScheduledReport,
    id = Id(id),
    bind = report
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .platform
        .scheduled_reports
        .delete(&report.org_id, &report.id)
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[resource_permission(
    action = "reports.read",
    resource = ScheduledReport,
    id = Id(id),
    bind = report
)]
async fn deliveries(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let ds = state
        .platform
        .scheduled_reports
        .list_deliveries(&report.org_id, &report.id)
        .await?;
    Ok(Json(serde_json::to_value(ds).unwrap_or(Value::Null)))
}
