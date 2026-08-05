// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Built-in and organization-scoped custom report templates.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::report_templates::ReportTemplate,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/report_templates", get(list).post(create))
        .route(
            "/report_templates/{id}",
            get(get_one).put(update).delete(delete),
        )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub target_type: String,
    pub format: String,
    pub time_range_preset: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resp {
    pub id: String,
    pub name: String,
    pub description: String,
    pub target_type: String,
    pub format: String,
    pub time_range_preset: String,
    pub is_builtin: bool,
    pub created_at_micros: Option<i64>,
    pub updated_at_micros: Option<i64>,
}

fn builtins() -> Vec<Resp> {
    vec![
        Resp {
            id: "weekly-platform-health".to_owned(),
            name: "Weekly platform health".to_owned(),
            description: "Dashboard PDF covering availability, error rate, and latency.".to_owned(),
            target_type: "dashboard".to_owned(),
            format: "pdf".to_owned(),
            time_range_preset: "previous-calendar-week".to_owned(),
            is_builtin: true,
            created_at_micros: None,
            updated_at_micros: None,
        },
        Resp {
            id: "daily-error-digest".to_owned(),
            name: "Daily error digest".to_owned(),
            description: "CSV export for recent incidents and high-volume errors.".to_owned(),
            target_type: "saved_view".to_owned(),
            format: "csv".to_owned(),
            time_range_preset: "previous-calendar-day".to_owned(),
            is_builtin: true,
            created_at_micros: None,
            updated_at_micros: None,
        },
        Resp {
            id: "monthly-capacity-review".to_owned(),
            name: "Monthly capacity review".to_owned(),
            description: "JSON export for storage, ingest, and query usage review.".to_owned(),
            target_type: "saved_view".to_owned(),
            format: "json".to_owned(),
            time_range_preset: "previous-calendar-month".to_owned(),
            is_builtin: true,
            created_at_micros: None,
            updated_at_micros: None,
        },
        Resp {
            id: "monthly-sla-compliance".to_owned(),
            name: "Monthly SLA compliance".to_owned(),
            description:
                "Dashboard PDF covering service availability, SLA attainment, error-budget burn, and breach risk."
                    .to_owned(),
            target_type: "dashboard".to_owned(),
            format: "pdf".to_owned(),
            time_range_preset: "previous-calendar-month".to_owned(),
            is_builtin: true,
            created_at_micros: None,
            updated_at_micros: None,
        },
    ]
}

pub(crate) fn builtin_templates_json() -> Vec<Value> {
    builtins()
        .into_iter()
        .filter_map(|template| serde_json::to_value(template).ok())
        .collect()
}

fn to_resp(template: ReportTemplate) -> Resp {
    Resp {
        id: template.id.0,
        name: template.name,
        description: template.description,
        target_type: template.target_type,
        format: template.format,
        time_range_preset: template.time_range_preset,
        is_builtin: false,
        created_at_micros: Some(template.created_at.0),
        updated_at_micros: Some(template.updated_at.0),
    }
}

fn normalize_and_validate(mut req: WriteReq) -> Result<WriteReq> {
    req.name = req.name.trim().to_owned();
    req.description = req.description.trim().to_owned();
    req.target_type = req.target_type.trim().to_lowercase();
    req.format = req.format.trim().to_lowercase();
    req.time_range_preset = req.time_range_preset.trim().to_owned();

    if req.name.is_empty() || req.name.len() > 255 {
        return Err(Error::invalid("name must contain 1 to 255 characters"));
    }
    if req.description.len() > 4_000 {
        return Err(Error::invalid(
            "description must not exceed 4000 characters",
        ));
    }
    if !matches!(req.target_type.as_str(), "dashboard" | "saved_view") {
        return Err(Error::invalid(
            "target_type must be dashboard or saved_view",
        ));
    }
    if !matches!(req.format.as_str(), "pdf" | "csv" | "json") {
        return Err(Error::invalid("format must be one of pdf | csv | json"));
    }
    if req.target_type == "dashboard" && req.format != "pdf" {
        return Err(Error::invalid(
            "dashboard templates only support pdf format",
        ));
    }
    if !matches!(
        req.time_range_preset.as_str(),
        "previous-24-hours"
            | "previous-7-days"
            | "previous-calendar-day"
            | "previous-calendar-week"
            | "previous-calendar-month"
            | "custom"
    ) {
        return Err(Error::invalid(
            "time_range_preset is not a supported report template range",
        ));
    }
    Ok(req)
}

#[permission("reports.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    let mut templates = builtins();
    templates.extend(
        state
            .platform
            .report_templates
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp),
    );
    Ok(Json(templates))
}

#[permission("reports.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    if let Some(template) = builtins().into_iter().find(|template| template.id == id) {
        return Ok(Json(template));
    }
    let template = state
        .platform
        .report_templates
        .get(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(to_resp(template)))
}

#[permission("reports.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<WriteReq>,
) -> Result<Json<Resp>> {
    let req = normalize_and_validate(req)?;
    let now = TimestampMicros::now();
    let template = state
        .platform
        .report_templates
        .create(ReportTemplate {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            name: req.name,
            description: req.description,
            target_type: req.target_type,
            format: req.format,
            time_range_preset: req.time_range_preset,
            created_at: now,
            updated_at: now,
        })
        .await?;
    Ok(Json(to_resp(template)))
}

#[permission("reports.edit")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<WriteReq>,
) -> Result<Json<Resp>> {
    let req = normalize_and_validate(req)?;
    let existing = state
        .platform
        .report_templates
        .get(&ctx.org_id, &Id(id))
        .await?;
    let template = state
        .platform
        .report_templates
        .update(ReportTemplate {
            id: existing.id,
            org_id: existing.org_id,
            name: req.name,
            description: req.description,
            target_type: req.target_type,
            format: req.format,
            time_range_preset: req.time_range_preset,
            created_at: existing.created_at,
            updated_at: TimestampMicros::now(),
        })
        .await?;
    Ok(Json(to_resp(template)))
}

#[permission("reports.delete")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .platform
        .report_templates
        .delete(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> WriteReq {
        WriteReq {
            name: " Weekly health ".to_owned(),
            description: " Ops ".to_owned(),
            target_type: "DASHBOARD".to_owned(),
            format: "PDF".to_owned(),
            time_range_preset: "previous-calendar-week".to_owned(),
        }
    }

    #[test]
    fn write_request_is_normalized() {
        let req = normalize_and_validate(request()).unwrap();
        assert_eq!(req.name, "Weekly health");
        assert_eq!(req.description, "Ops");
        assert_eq!(req.target_type, "dashboard");
        assert_eq!(req.format, "pdf");
    }

    #[test]
    fn invalid_target_type_is_rejected() {
        let mut req = request();
        req.target_type = "trace".to_owned();
        assert!(normalize_and_validate(req).is_err());
    }

    #[test]
    fn dashboard_template_rejects_data_formats() {
        let mut req = request();
        req.format = "csv".to_owned();
        assert!(normalize_and_validate(req).is_err());
    }

    #[test]
    fn saved_view_template_accepts_json() {
        let mut req = request();
        req.target_type = "saved_view".to_owned();
        req.format = "json".to_owned();
        assert!(normalize_and_validate(req).is_ok());
    }

    #[test]
    fn unknown_range_is_rejected() {
        let mut req = request();
        req.time_range_preset = "previous-13-fortnights".to_owned();
        assert!(normalize_and_validate(req).is_err());
    }

    #[test]
    fn scheduling_fields_are_not_part_of_template_contract() {
        let input = serde_json::json!({
            "name": "Weekly health",
            "description": "Ops",
            "target_type": "dashboard",
            "format": "pdf",
            "time_range_preset": "previous-calendar-week",
            "cron": "every:7d"
        });
        assert!(serde_json::from_value::<WriteReq>(input).is_err());
    }

    #[test]
    fn builtin_templates_are_marked_read_only() {
        assert!(builtins().iter().all(|template| template.is_builtin));
    }

    #[test]
    fn builtin_templates_include_monthly_sla_compliance() {
        let template = builtins()
            .into_iter()
            .find(|template| template.id == "monthly-sla-compliance")
            .expect("monthly SLA compliance template");
        assert_eq!(template.target_type, "dashboard");
        assert_eq!(template.format, "pdf");
        assert_eq!(template.time_range_preset, "previous-calendar-month");
    }
}
