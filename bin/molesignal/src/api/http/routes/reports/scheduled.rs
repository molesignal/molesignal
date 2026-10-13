// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled reports HTTP routes（spec scheduled-reports）。

use std::collections::HashMap;

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
    app::{iam::IamContext, reporting::RunAsProfile},
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
    /// 定时投递的 png / pdf 以这位用户的身份渲染：最后一次保存报表的登录用户。
    pub run_as_user_id: Option<String>,
    /// 这位用户的显示名和邮箱。报表没有运行身份，或用户记录已经不存在时为 `null`。
    pub run_as_user: Option<RunAsProfile>,
    pub last_run_at_micros: Option<i64>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

/// 这些用户的显示名和邮箱：直接读用户记录，不要求请求方能读成员目录。
async fn run_as_profiles(
    state: &AppState,
    user_ids: impl IntoIterator<Item = &Id>,
) -> Result<HashMap<Id, RunAsProfile>> {
    crate::app::reporting::run_as_profiles(state.iam.service.users.as_ref(), user_ids).await
}

fn to_resp(r: ScheduledReport, run_as: &HashMap<Id, RunAsProfile>) -> Resp {
    let run_as_user = r
        .run_as_user_id
        .as_ref()
        .and_then(|id| run_as.get(id).cloned());

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
        run_as_user_id: r.run_as_user_id.map(|i| i.0),
        run_as_user,
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
    let reports = state.platform.scheduled_reports.list(&ctx.org_id).await?;

    let run_as = run_as_profiles(
        &state,
        reports
            .iter()
            .filter_map(|report| report.run_as_user_id.as_ref()),
    )
    .await?;

    Ok(Json(
        reports
            .into_iter()
            .map(|report| to_resp(report, &run_as))
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
    let run_as = run_as_profiles(&state, &report.run_as_user_id).await?;

    Ok(Json(to_resp(report, &run_as)))
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

        let url = crate::infra::reporting::render_page_url(
            &state.platform.report_renderer_base_url,
            &r,
            chrono::Utc::now(),
        )?;

        let browser_auth_storage = browser_auth_storage(&state, &ctx)?;

        let bytes = renderer
            .render(
                &url,
                format,
                Viewport::CONFIGURED,
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

/// 即时导出和分享快照以发起请求的用户渲染：登录态里的 token 只在一次渲染的时间内有效。
pub(crate) fn browser_auth_storage(state: &AppState, ctx: &IamContext) -> Result<String> {
    crate::app::reporting::render_session(&state.iam.service, ctx)
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

    let run_as_user_id = crate::app::reporting::run_as_user(&ctx);

    // 先读后写：读不到名字就不创建，不会留下一份已经建好、响应却是失败的报表。
    let run_as = run_as_profiles(&state, &run_as_user_id).await?;
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
        run_as_user_id,
        last_run_at: None,
        created_at: now,
        updated_at: now,
    };

    let r = state.platform.scheduled_reports.create(r).await?;
    Ok(Json(to_resp(r, &run_as)))
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

    // 每次保存都改成保存者：改了收件地址的人，拿不到上一位保存者才看得到的内容。
    let run_as_user_id = crate::app::reporting::run_as_user(&ctx);
    let run_as = run_as_profiles(&state, &run_as_user_id).await?;

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
        run_as_user_id,
        last_run_at: existing.last_run_at,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };

    let r = state.platform.scheduled_reports.update(r).await?;
    Ok(Json(to_resp(r, &run_as)))
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn report(run_as: Option<&str>) -> ScheduledReport {
        ScheduledReport {
            id: Id::from_string("r1"),
            org_id: Id::from_string("org-1"),
            name: "weekly".into(),
            dashboard_id: Some(Id::from_string("d1")),
            saved_view_id: None,
            cron: "every:7d".into(),
            recipients: Vec::new(),
            format: "pdf".into(),
            time_range_json: json!({ "preset": "previous-7-days" }),
            enabled: true,
            run_as_user_id: run_as.map(Id::from_string),
            last_run_at: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(2),
        }
    }

    fn ada() -> HashMap<Id, RunAsProfile> {
        let profile = RunAsProfile {
            id: Id::from_string("u-ada"),
            display_name: "Ada".into(),
            email: "ada@example.com".into(),
        };

        HashMap::from([(profile.id.clone(), profile)])
    }

    fn body(report: ScheduledReport, run_as: &HashMap<Id, RunAsProfile>) -> Value {
        serde_json::to_value(to_resp(report, run_as)).unwrap()
    }

    #[test]
    fn a_report_names_the_user_it_runs_as() {
        let body = body(report(Some("u-ada")), &ada());

        assert_eq!(body["run_as_user_id"], "u-ada");
        assert_eq!(
            body["run_as_user"],
            json!({ "id": "u-ada", "display_name": "Ada", "email": "ada@example.com" })
        );
    }

    #[test]
    fn a_user_who_no_longer_exists_leaves_only_the_id() {
        let body = body(report(Some("u-gone")), &ada());

        assert_eq!(body["run_as_user_id"], "u-gone");
        assert_eq!(body["run_as_user"], Value::Null);
    }

    #[test]
    fn a_report_saved_without_a_signed_in_user_names_no_one() {
        let body = body(report(None), &ada());

        assert_eq!(body["run_as_user_id"], Value::Null);
        assert_eq!(body["run_as_user"], Value::Null);
    }
}
