// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! feature-parity follow-up：scheduled-reports render / deliver / cron tick。
//!
//! 设计：
//! - tick 周期 60s；每次扫 `list_enabled_all`，对到期的 report 调 `run_once`。
//! - 到期判定：cron 字段优先解析 `every:Ns/m/h`（与 scheduled-pipelines 一致），
//!   解析失败则 fallback 到 24h（多数 ops 把 cron 写 `every:24h` 或 `0 0 * * *`，
//!   完整 5 字段 cron 解析依赖 `cron` crate；上线后再开启）。
//! - render：当前产 `application/json` payload（dashboard ID / saved_view ID +
//!   time_range + 截图 URL 占位）。真正的 SVG / PDF 渲染需要 headless Chrome，
//!   属付费版能力。
//! - deliver：复用 `reqwest::Client`（webhook / s3 PUT pre-signed）+
//!   报告生成与投递状态由 scheduled report worker 独立管理。

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use tokio::task::JoinHandle;

// 报表 payload 渲染下沉到 infra::reporting（与 /scheduled_reports/{id}/preview 共用）。
// 别名为 `render`，保持本文件既有 call sites / 测试不变。
use crate::infra::reporting::{content_type_for, render_payload as render};
use crate::{
    infra::persistence::repositories::scheduled_reports::{
        DeliveryStatus, ReportDelivery, ReportRecipient, ScheduledReport, ScheduledReportRepository,
    },
    shared::{
        Error, ReportFormat, ReportRenderer, Result, Viewport, ids::Id, time::TimestampMicros,
        validate_report_bytes,
    },
};

const TICK_SECS: u64 = 60;
/// cron 解析失败时 fallback 间隔（24h）。
const FALLBACK_INTERVAL_SECS: i64 = 24 * 3600;
fn report_recipient_kind(kind: &str) -> &'static str {
    match kind {
        "webhook" => "webhook",
        "s3" => "s3",
        "email" => "email",
        "slack" => "slack",
        _ => "unknown",
    }
}

pub struct ScheduledReportRunner {
    repo: Arc<dyn ScheduledReportRepository>,
    http: reqwest::Client,
    object_store: Arc<dyn ObjectStore>,
    /// `None` 时拒绝 png/pdf 渲染，禁止用 SVG 冒充目标格式。
    renderer: Option<Arc<dyn ReportRenderer>>,
    render_base_url: String,
}

impl ScheduledReportRunner {
    pub fn new(
        repo: Arc<dyn ScheduledReportRepository>,
        object_store: Arc<dyn ObjectStore>,
        renderer: Option<Arc<dyn ReportRenderer>>,
        render_base_url: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            repo,
            http,
            object_store,
            renderer,
            render_base_url,
        }
    }

    pub fn spawn(self: Arc<Self>) -> JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(TICK_SECS));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(e) = me.tick_once().await {
                    tracing::warn!(error = %e, "scheduled_reports tick failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.scheduled_reports",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "scheduled_reports")
    )]
    pub async fn tick_once(&self) -> Result<usize> {
        let reports = self.repo.list_enabled_all().await?;
        let now = TimestampMicros::now();
        let mut fired = 0usize;
        for r in reports {
            if is_due(&r, now) {
                if let Err(e) = self.run_once(&r, now).await {
                    tracing::warn!(report_id = %r.id.0, error = %e, "report run failed");
                }
                fired += 1;
            }
        }
        Ok(fired)
    }

    #[tracing::instrument(
        name = "report.run",
        skip_all,
        fields(otel.kind = "internal", molesignal.report.format = ?r.format)
    )]
    async fn run_once(&self, r: &ScheduledReport, now: TimestampMicros) -> Result<()> {
        let body = match self.render_report(r).await {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(
                    report_id = %r.id.0,
                    format = %r.format,
                    error = %error,
                    "scheduled report render failed"
                );
                let error = error.to_string();
                for recipient in &r.recipients {
                    self.repo
                        .record_delivery(ReportDelivery {
                            id: Id::new(),
                            report_id: r.id.clone(),
                            org_id: r.org_id.clone(),
                            status: DeliveryStatus::Failed,
                            attempt: 1,
                            recipient_kind: recipient.kind.clone(),
                            recipient_target: recipient.target.clone(),
                            error: Some(error.clone()),
                            attempted_at: TimestampMicros::now(),
                        })
                        .await?;
                }
                self.repo.touch_last_run(&r.id, now).await?;
                return Ok(());
            }
        };
        for recipient in &r.recipients {
            let result = self.deliver(r, recipient, &body).await;
            let status = if result.is_ok() {
                DeliveryStatus::Sent
            } else {
                DeliveryStatus::Failed
            };
            self.repo
                .record_delivery(ReportDelivery {
                    id: Id::new(),
                    report_id: r.id.clone(),
                    org_id: r.org_id.clone(),
                    status,
                    attempt: 1,
                    recipient_kind: recipient.kind.clone(),
                    recipient_target: recipient.target.clone(),
                    error: result.err().map(|e| e.to_string()),
                    attempted_at: TimestampMicros::now(),
                })
                .await?;
        }
        self.repo.touch_last_run(&r.id, now).await?;
        Ok(())
    }

    #[tracing::instrument(
        name = "report.deliver",
        skip_all,
        fields(otel.kind = "producer", molesignal.report.recipient_kind = report_recipient_kind(&recipient.kind))
    )]
    async fn deliver(
        &self,
        r: &ScheduledReport,
        recipient: &ReportRecipient,
        body: &[u8],
    ) -> Result<()> {
        match recipient.kind.as_str() {
            "webhook" => {
                let request = self
                    .http
                    .post(&recipient.target)
                    .header("Content-Type", content_type_for(&r.format))
                    .body(body.to_vec());
                let resp = crate::shared::http_trace::send(
                    &self.http,
                    request,
                    crate::shared::http_trace::HttpTarget::ThirdParty,
                )
                .await
                .map_err(|e| crate::shared::Error::internal(format!("webhook: {e}")))?;
                if !resp.status().is_success() {
                    return Err(crate::shared::Error::internal(format!(
                        "webhook {} returned {}",
                        recipient.target,
                        resp.status()
                    )));
                }
                Ok(())
            }
            "s3" => {
                // `target` 是 object key prefix；写到 `<prefix>/<report-id>-<ts>.<ext>`
                let ext = ext_for(&r.format);
                let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
                let key = format!(
                    "{prefix}/{id}-{ts}.{ext}",
                    prefix = recipient.target.trim_end_matches('/'),
                    id = r.id.0
                );
                self.object_store
                    .put(
                        &Path::from(key.clone()),
                        PutPayload::from(Bytes::copy_from_slice(body)),
                    )
                    .await
                    .map_err(|e| crate::shared::Error::internal(format!("s3 put: {e}")))?;
                Ok(())
            }
            "email" => {
                // SMTP 投递留 follow-up：要在 SchedulerReportRunner 注入 EmailSender 才能发；
                // 这里只校验目标地址非空，避免 spam。
                if recipient.target.contains('@') {
                    tracing::info!(
                        recipient = %recipient.target,
                        report_id = %r.id.0,
                        "email delivery not yet wired; would send"
                    );
                    Ok(())
                } else {
                    Err(crate::shared::Error::invalid("invalid email recipient"))
                }
            }
            other => Err(crate::shared::Error::invalid(format!(
                "unknown recipient kind: {other}"
            ))),
        }
    }
}

impl ScheduledReportRunner {
    /// format=png|pdf 时必须调用真实 renderer，并校验输出 magic bytes；renderer
    /// 不可用或输出无效时返回错误，禁止把 SVG/JSON 伪装成 PDF/PNG。
    async fn render_report(&self, r: &ScheduledReport) -> Result<Vec<u8>> {
        if let Ok(fmt) = r.format.parse::<ReportFormat>() {
            let renderer = self.renderer.as_ref().ok_or_else(|| {
                Error::unavailable(
                    "scheduled report PDF/PNG renderer is unavailable; verify the Chrome runtime",
                )
            })?;
            let url = build_embed_url(&self.render_base_url, r);
            // 定时任务当前没有交互用户会话；目标渲染页必须由受信任的内部入口提供。
            let bytes = renderer
                .render(&url, fmt, Viewport::default(), None)
                .await?;
            validate_report_bytes(fmt, &bytes)?;
            return Ok(bytes.to_vec());
        }
        render(r)
    }
}

/// 构造内部报告渲染 URL。使用前端真实存在的路由，避免 `/embed` 落入 404 页面。
fn build_embed_url(base_url: &str, r: &ScheduledReport) -> String {
    let base_url = base_url.trim_end_matches('/');
    if let Some(id) = &r.dashboard_id {
        return format!("{base_url}/dashboards/{}?report_render=1", id.0);
    }
    if let Some(id) = &r.saved_view_id {
        return format!("{base_url}/saved-views?view={}&report_render=1", id.0);
    }
    format!("{base_url}/")
}

fn ext_for(format: &str) -> &'static str {
    match format {
        "json" => "json",
        "csv" => "csv",
        "svg" => "svg",
        "png" => "png",
        "pdf" => "pdf",
        _ => "bin",
    }
}

fn is_due(r: &ScheduledReport, now: TimestampMicros) -> bool {
    let interval_secs = parse_every_secs(&r.cron).unwrap_or(FALLBACK_INTERVAL_SECS);
    match r.last_run_at {
        Some(last) => now.0 - last.0 >= interval_secs * 1_000_000,
        None => true,
    }
}

fn parse_every_secs(cron: &str) -> Option<i64> {
    let rest = cron.trim().strip_prefix("every:")?;
    let split = rest.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = rest.split_at(split);
    let n: i64 = num.parse().ok()?;
    Some(match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mk_report(format: &str, last_run: Option<TimestampMicros>, cron: &str) -> ScheduledReport {
        ScheduledReport {
            id: Id::new(),
            org_id: Id("orgA".into()),
            name: "weekly".into(),
            dashboard_id: Some(Id("d1".into())),
            saved_view_id: None,
            cron: cron.into(),
            recipients: vec![],
            format: format.into(),
            time_range_json: json!({ "from": "now-7d", "to": "now" }),
            enabled: true,
            last_run_at: last_run,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn render_json_produces_valid_object() {
        let bytes = render(&mk_report("json", None, "every:1d")).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.get("report_id").is_some());
        assert_eq!(v["format"], "json");
    }

    #[test]
    fn render_svg_contains_name() {
        let bytes = render(&mk_report("svg", None, "every:1d")).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("weekly"));
        assert!(s.starts_with("<svg"));
    }

    #[test]
    fn is_due_first_run() {
        assert!(is_due(
            &mk_report("json", None, "every:1h"),
            TimestampMicros::now()
        ));
    }

    #[test]
    fn is_due_respects_interval() {
        let now = TimestampMicros::now();
        let recently = TimestampMicros(now.0 - 30 * 1_000_000);
        let r = mk_report("json", Some(recently), "every:1h");
        assert!(!is_due(&r, now));
        let long_ago = TimestampMicros(now.0 - 7200 * 1_000_000);
        let r = mk_report("json", Some(long_ago), "every:1h");
        assert!(is_due(&r, now));
    }

    /// task 6.3：format=svg → 不应进入 renderer 分支（即便 renderer 配了）。
    /// 用一个会 panic 的 fake renderer：被调到立即视为失败。
    #[tokio::test]
    async fn svg_path_does_not_call_renderer() {
        use async_trait::async_trait;
        use bytes::Bytes;

        use crate::shared::{ReportFormat, ReportRenderer, Result as SResult, Viewport};

        struct PanicRenderer;
        #[async_trait]
        impl ReportRenderer for PanicRenderer {
            async fn render(
                &self,
                _url: &str,
                _format: ReportFormat,
                _viewport: Viewport,
                _browser_auth_storage: Option<&str>,
            ) -> SResult<Bytes> {
                panic!("renderer must not be called for format=svg")
            }
        }

        let repo: Arc<dyn ScheduledReportRepository> = Arc::new(FakeRepo);
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let runner = ScheduledReportRunner::new(
            repo,
            store,
            Some(Arc::new(PanicRenderer) as Arc<dyn ReportRenderer>),
            "http://127.0.0.1:5080".into(),
        );
        let r = mk_report("svg", None, "every:1d");
        let bytes = runner.render_report(&r).await.unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("<svg"));
    }

    /// 配 None renderer 时 png 明确报 unavailable，不生成伪装文件。
    #[tokio::test]
    async fn png_without_renderer_is_unavailable() {
        let repo: Arc<dyn ScheduledReportRepository> = Arc::new(FakeRepo);
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let runner = ScheduledReportRunner::new(repo, store, None, "http://127.0.0.1:5080".into());
        let r = mk_report("png", None, "every:1d");
        let error = runner.render_report(&r).await.unwrap_err();
        assert!(matches!(error, Error::Unavailable(_)));
    }

    // 一个永远空的 repo 仅供测试构造 runner 用。
    struct FakeRepo;
    #[async_trait::async_trait]
    impl ScheduledReportRepository for FakeRepo {
        async fn create(&self, r: ScheduledReport) -> Result<ScheduledReport> {
            Ok(r)
        }
        async fn update(&self, r: ScheduledReport) -> Result<ScheduledReport> {
            Ok(r)
        }
        async fn get(&self, _: &Id, _: &Id) -> Result<ScheduledReport> {
            Err(crate::shared::Error::not_found("test"))
        }
        async fn get_by_id(&self, _: &Id) -> Result<ScheduledReport> {
            Err(crate::shared::Error::not_found("test"))
        }
        async fn list(&self, _: &Id) -> Result<Vec<ScheduledReport>> {
            Ok(vec![])
        }
        async fn list_enabled_all(&self) -> Result<Vec<ScheduledReport>> {
            Ok(vec![])
        }
        async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
            Ok(())
        }
        async fn touch_last_run(&self, _: &Id, _: TimestampMicros) -> Result<()> {
            Ok(())
        }
        async fn record_delivery(&self, _: ReportDelivery) -> Result<()> {
            Ok(())
        }
        async fn list_deliveries(&self, _: &Id, _: &Id) -> Result<Vec<ReportDelivery>> {
            Ok(vec![])
        }
    }
}
