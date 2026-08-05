// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Scheduled report 渲染（spec scheduled-reports）。
//!
//! 纯 payload 渲染：dashboard / saved_view ID + 时间窗 + 元数据，按 `format` 编码成
//! json / csv / svg 字节。PDF/PNG 必须走 `ReportRenderer`，本模块会明确拒绝，避免
//! 生成 MIME 与内容不匹配的损坏文件。由两处共用：
//! - `scheduled_reports` worker（定时投递，bootstrap）；
//! - `GET /scheduled_reports/{id}/preview` handler（即时预览，api）。
//!
//! png / pdf 的 headless 渲染是付费版能力（headless Chrome），单独在 worker 侧叠加，
//! 不在本模块。

use crate::{
    infra::persistence::repositories::scheduled_reports::ScheduledReport,
    shared::{Error, Result},
};

/// 渲染静态报表 payload（json / csv / svg）。PDF/PNG 必须由 headless renderer 生成。
pub fn render_payload(r: &ScheduledReport) -> Result<Vec<u8>> {
    let payload = serde_json::json!({
        "report_id": r.id.0,
        "report_name": r.name,
        "dashboard_id": r.dashboard_id.as_ref().map(|i| &i.0),
        "saved_view_id": r.saved_view_id.as_ref().map(|i| &i.0),
        "time_range": r.time_range_json,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "format": r.format,
    });
    let bytes = match r.format.as_str() {
        "json" => serde_json::to_vec_pretty(&payload).unwrap_or_default(),
        "csv" => {
            let mut out = String::from("field,value\n");
            if let Some(obj) = payload.as_object() {
                for (k, v) in obj.iter() {
                    out.push_str(&format!("{k},{v}\n"));
                }
            }
            out.into_bytes()
        }
        "svg" => format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"640\" height=\"360\">\
             <text x=\"20\" y=\"40\" font-family=\"sans-serif\" font-size=\"16\">{}</text>\
             <text x=\"20\" y=\"80\" font-family=\"sans-serif\" font-size=\"12\">{}</text>\
             </svg>",
            r.name,
            chrono::Utc::now().to_rfc3339()
        )
        .into_bytes(),
        "png" | "pdf" => {
            return Err(Error::unavailable(
                "PDF/PNG report output requires the headless renderer",
            ));
        }
        other => {
            return Err(Error::invalid(format!(
                "unsupported report format: {other}"
            )));
        }
    };
    Ok(bytes)
}

/// report format → MIME content-type。
pub fn content_type_for(format: &str) -> &'static str {
    match format {
        "json" => "application/json",
        "csv" => "text/csv",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn mk_report(format: &str) -> ScheduledReport {
        ScheduledReport {
            id: Id::new(),
            org_id: Id("orgA".into()),
            name: "weekly".into(),
            dashboard_id: Some(Id("d1".into())),
            saved_view_id: None,
            cron: "every:1d".into(),
            recipients: vec![],
            format: format.into(),
            time_range_json: json!({ "from": "now-7d", "to": "now" }),
            enabled: true,
            last_run_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn json_payload_is_valid_object_with_expected_fields() {
        let bytes = render_payload(&mk_report("json")).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["format"], "json");
        assert_eq!(v["dashboard_id"], "d1");
        assert!(v.get("report_id").is_some());
        assert!(v.get("time_range").is_some());
    }

    #[test]
    fn svg_payload_contains_name_and_is_svg() {
        let s = String::from_utf8(render_payload(&mk_report("svg")).unwrap()).unwrap();
        assert!(s.starts_with("<svg"));
        assert!(s.contains("weekly"));
    }

    #[test]
    fn content_type_mapping() {
        assert_eq!(content_type_for("json"), "application/json");
        assert_eq!(content_type_for("svg"), "image/svg+xml");
        assert_eq!(content_type_for("png"), "image/png");
        assert_eq!(content_type_for("weird"), "application/octet-stream");
    }

    #[test]
    fn pdf_and_png_require_real_renderer() {
        assert!(matches!(
            render_payload(&mk_report("pdf")),
            Err(Error::Unavailable(_))
        ));
        assert!(matches!(
            render_payload(&mk_report("png")),
            Err(Error::Unavailable(_))
        ));
    }
}
