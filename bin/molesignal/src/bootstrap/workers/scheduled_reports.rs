// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! scheduled-reports render / deliver / cron tick worker。
//!
//! 设计：
//! - tick 周期 60s；每次扫 `list_enabled_all`，对到期且本节点获准运行的 report 调 `run_once`。
//! - 多个 alert 节点按 report id 分担：开跑前先过 [`RunGate`]（归属 + 运行租约），认领成功后重读
//!   report 再确认仍然到期，成员变化时同一份报告不会被两个节点各发一遍。
//! - 到期判定：cron 字段优先解析 `every:Ns/m/h`（与 scheduled-pipelines 一致），
//!   解析失败则 fallback 到 24h（多数 ops 把 cron 写 `every:24h` 或 `0 0 * * *`，
//!   完整 5 字段 cron 解析依赖 `cron` crate；上线后再开启）。
//! - render：json / csv / svg 是静态 payload（dashboard ID / saved_view ID + time_range）；
//!   png / pdf 由 headless Chrome 加载前端的报表页打印。报表页要登录后才能取数，每次渲染
//!   向 [`ReportRenderSession`] 要一份登录态，要不到就让这次运行失败并把原因写进投递记录。
//! - deliver：复用 `reqwest::Client`（webhook / s3 PUT pre-signed）+
//!   报告生成与投递状态由 scheduled report worker 独立管理。

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path};
use tokio::task::JoinHandle;

// 报表 payload 渲染下沉到 infra::reporting（与 /scheduled_reports/{id}/preview 共用）。
// 别名为 `render`，保持本文件既有 call sites / 测试不变。
use crate::infra::reporting::{content_type_for, render_page_url, render_payload as render};
use crate::{
    app::reporting::ReportRenderSession,
    domain::run_claim::{OpenGate, RunGate},
    infra::persistence::repositories::scheduled_reports::{
        DeliveryStatus, ReportDelivery, ReportRecipient, ScheduledReport, ScheduledReportRepository,
    },
    shared::{
        Error, ReportFormat, ReportRenderer, Result, Viewport, ids::Id, time::TimestampMicros,
        validate_report_bytes,
    },
};

const TICK_SECS: u64 = 60;

/// 运行租约里这类工作的名字。
const RUN_KIND: &str = "scheduled_report";

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
    /// png/pdf 渲染时浏览器用的登录态：定时任务没有发起请求的人，由它决定以谁的身份取数。
    sessions: Arc<dyn ReportRenderSession>,
    /// 多个 alert 节点分担报告时的准入；默认每份到期的报告都跑。
    gate: Arc<dyn RunGate>,
}

impl ScheduledReportRunner {
    pub fn new(
        repo: Arc<dyn ScheduledReportRepository>,
        object_store: Arc<dyn ObjectStore>,
        renderer: Option<Arc<dyn ReportRenderer>>,
        render_base_url: String,
        sessions: Arc<dyn ReportRenderSession>,
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
            sessions,
            gate: Arc::new(OpenGate),
        }
    }

    /// 与其余 alert 节点分担报告：每份到期的报告开跑前先过 `gate`。
    pub fn with_gate(mut self, gate: Arc<dyn RunGate>) -> Self {
        self.gate = gate;
        self
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
            if !is_due(&r, now) {
                continue;
            }

            // 先认领再执行：归属决定谁来跑，运行租约兜住成员变化时的双跑。
            let permit = match self.gate.admit(RUN_KIND, r.id.as_str()).await {
                Ok(Some(permit)) => permit,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(report_id = %r.id.0, error = %e, "report run admission failed");
                    continue;
                }
            };

            // 认领后重读：另一个节点可能刚跑完并推进了 last_run_at，不能拿列表时的旧快照再发一遍。
            match self.repo.get_by_id(&r.id).await {
                Ok(fresh) if fresh.enabled && is_due(&fresh, now) => {
                    if let Err(e) = self.run_once(&fresh, now).await {
                        tracing::warn!(report_id = %r.id.0, error = %e, "report run failed");
                    }

                    fired += 1;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(report_id = %r.id.0, error = %e, "report reload failed");
                }
            }

            if let Err(e) = permit.release().await {
                tracing::warn!(report_id = %r.id.0, error = %e, "report run claim release failed");
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

            let url = render_page_url(&self.render_base_url, r, chrono::Utc::now())?;

            // 报表页要登录后才能取数。没有登录态时前端会跳去登录页，渲染只能等到超时。
            let session = self.sessions.issue(r).await?;

            let bytes = renderer
                .render(&url, fmt, Viewport::CONFIGURED, Some(&session))
                .await?;

            validate_report_bytes(fmt, &bytes)?;
            return Ok(bytes.to_vec());
        }

        render(r)
    }
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
            run_as_user_id: Some(Id("u-ada".into())),
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
            Arc::new(NoIdentity),
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
        let runner = ScheduledReportRunner::new(
            repo,
            store,
            None,
            "http://127.0.0.1:5080".into(),
            Arc::new(NoIdentity),
        );
        let r = mk_report("png", None, "every:1d");
        let error = runner.render_report(&r).await.unwrap_err();
        assert!(matches!(error, Error::Unavailable(_)));
    }

    // ───────────── png / pdf：浏览器以谁的身份取数 ─────────────

    /// 记下每次渲染拿到的地址和登录态，返回一个 magic bytes 正确的文件。
    #[derive(Default)]
    struct RecordingRenderer {
        renders: std::sync::Mutex<Vec<(String, Option<String>)>>,
    }

    impl RecordingRenderer {
        fn renders(&self) -> Vec<(String, Option<String>)> {
            self.renders.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl ReportRenderer for RecordingRenderer {
        async fn render(
            &self,
            url: &str,
            format: ReportFormat,
            _viewport: Viewport,
            browser_auth_storage: Option<&str>,
        ) -> Result<Bytes> {
            self.renders
                .lock()
                .unwrap()
                .push((url.to_owned(), browser_auth_storage.map(str::to_owned)));

            Ok(match format {
                ReportFormat::Pdf => Bytes::from_static(b"%PDF-1.7 rendered"),
                ReportFormat::Png => Bytes::from_static(b"\x89PNG\r\n\x1a\n rendered"),
            })
        }
    }

    /// 每份报表各有一份登录态，里面带着报表自己的 ID。
    struct SessionPerReport;

    #[async_trait::async_trait]
    impl ReportRenderSession for SessionPerReport {
        async fn issue(&self, report: &ScheduledReport) -> Result<String> {
            Ok(format!("session-for-{}", report.id.0))
        }
    }

    /// 没有可用身份：报表的所有者已经不在组织里。
    struct NoIdentity;

    const NO_IDENTITY: &str =
        "the report runs as ada@example.com, who is no longer a member of this organization";

    #[async_trait::async_trait]
    impl ReportRenderSession for NoIdentity {
        async fn issue(&self, _: &ScheduledReport) -> Result<String> {
            Err(Error::forbidden(NO_IDENTITY))
        }
    }

    fn rendering_runner(
        repo: Arc<Reports>,
        renderer: Arc<RecordingRenderer>,
        sessions: Arc<dyn ReportRenderSession>,
    ) -> (ScheduledReportRunner, Arc<object_store::memory::InMemory>) {
        let store = Arc::new(object_store::memory::InMemory::new());

        let runner = ScheduledReportRunner::new(
            repo,
            store.clone(),
            Some(renderer as Arc<dyn ReportRenderer>),
            "http://web:5173".into(),
            sessions,
        );

        (runner, store)
    }

    fn stored(report: &mut ScheduledReport, prefix: &str) {
        report.recipients = vec![ReportRecipient {
            kind: "s3".into(),
            target: prefix.into(),
        }];
    }

    #[tokio::test]
    async fn a_pdf_report_is_rendered_with_the_session_issued_for_it() {
        let mut report = mk_report("pdf", None, "every:1d");
        stored(&mut report, "reports/weekly");
        let repo = Reports::new(vec![report.clone()]);
        let renderer = Arc::new(RecordingRenderer::default());
        let (runner, store) =
            rendering_runner(repo.clone(), renderer.clone(), Arc::new(SessionPerReport));

        assert_eq!(runner.tick_once().await.unwrap(), 1);

        // 浏览器加载的是这份报表的报表页，登录态是为这份报表签发的那一份。
        let renders = renderer.renders();
        assert_eq!(renders.len(), 1);
        assert!(
            renders[0]
                .0
                .starts_with("http://web:5173/dashboards/d1?report_render=1"),
            "{}",
            renders[0].0
        );
        assert_eq!(
            renders[0].1.as_deref(),
            Some(format!("session-for-{}", report.id.0).as_str())
        );

        // 渲染出的文件投递到了收件位置，并记为已发送。
        let objects: Vec<_> = futures::TryStreamExt::try_collect(store.list(None))
            .await
            .unwrap();
        assert_eq!(objects.len(), 1);
        assert!(
            objects[0]
                .location
                .as_ref()
                .starts_with(&format!("reports/weekly/{}-", report.id.0)),
            "{}",
            objects[0].location
        );
        let body = store
            .get(&objects[0].location)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert!(body.starts_with(b"%PDF-"));

        let deliveries = repo.deliveries();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].status, DeliveryStatus::Sent);
        assert_eq!(deliveries[0].error, None);
    }

    #[tokio::test]
    async fn a_png_report_gets_a_session_too() {
        let report = mk_report("png", None, "every:1d");
        let repo = Reports::new(vec![report.clone()]);
        let renderer = Arc::new(RecordingRenderer::default());
        let (runner, _store) = rendering_runner(repo, renderer.clone(), Arc::new(SessionPerReport));

        let bytes = runner.render_report(&report).await.unwrap();

        assert!(bytes.starts_with(b"\x89PNG"));
        assert_eq!(
            renderer.renders()[0].1.as_deref(),
            Some(format!("session-for-{}", report.id.0).as_str())
        );
    }

    #[tokio::test]
    async fn a_report_with_no_usable_identity_fails_every_recipient_with_the_reason() {
        let mut report = mk_report("pdf", None, "every:1d");
        report.recipients = vec![
            ReportRecipient {
                kind: "s3".into(),
                target: "reports/weekly".into(),
            },
            ReportRecipient {
                kind: "webhook".into(),
                target: "http://127.0.0.1:9/hook".into(),
            },
        ];
        let repo = Reports::new(vec![report.clone()]);
        let renderer = Arc::new(RecordingRenderer::default());
        let (runner, store) =
            rendering_runner(repo.clone(), renderer.clone(), Arc::new(NoIdentity));

        assert_eq!(runner.tick_once().await.unwrap(), 1);

        // 没有登录态就不去加载页面：否则只会等到超时，原因也说不清。
        assert!(renderer.renders().is_empty());
        let objects: Vec<_> = futures::TryStreamExt::try_collect(store.list(None))
            .await
            .unwrap();
        assert!(objects.is_empty());

        let deliveries = repo.deliveries();
        assert_eq!(deliveries.len(), 2);

        for delivery in &deliveries {
            assert_eq!(delivery.status, DeliveryStatus::Failed);
            assert_eq!(
                delivery.error.as_deref(),
                Some(format!("forbidden: {NO_IDENTITY}").as_str())
            );
        }

        // 这次运行算数：下一个周期再试，而不是每分钟失败一次。
        assert_eq!(repo.touches(), 1);
        assert_eq!(runner.tick_once().await.unwrap(), 0);
    }

    // ───────────── 多个 alert 节点分担报告：运行准入 ─────────────

    /// `list_enabled_all` 返回调用方给定的（可能过期的）快照，`get_by_id` 返回最新的一份；
    /// 记录 `last_run_at` 的推进次数。
    struct Reports {
        snapshot: Vec<ScheduledReport>,
        fresh: std::sync::Mutex<Vec<ScheduledReport>>,
        touches: std::sync::atomic::AtomicUsize,
        deliveries: std::sync::Mutex<Vec<ReportDelivery>>,
    }

    impl Reports {
        fn new(reports: Vec<ScheduledReport>) -> Arc<Self> {
            Arc::new(Self {
                snapshot: reports.clone(),
                fresh: std::sync::Mutex::new(reports),
                touches: std::sync::atomic::AtomicUsize::new(0),
                deliveries: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn touches(&self) -> usize {
            self.touches.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn deliveries(&self) -> Vec<ReportDelivery> {
            self.deliveries.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl ScheduledReportRepository for Reports {
        async fn create(&self, _r: ScheduledReport) -> Result<ScheduledReport> {
            unimplemented!()
        }
        async fn update(&self, _r: ScheduledReport) -> Result<ScheduledReport> {
            unimplemented!()
        }
        async fn get(&self, _: &Id, id: &Id) -> Result<ScheduledReport> {
            self.get_by_id(id).await
        }
        async fn get_by_id(&self, id: &Id) -> Result<ScheduledReport> {
            self.fresh
                .lock()
                .unwrap()
                .iter()
                .find(|r| &r.id == id)
                .cloned()
                .ok_or_else(|| Error::not_found("report"))
        }
        async fn list(&self, _: &Id) -> Result<Vec<ScheduledReport>> {
            unimplemented!()
        }
        async fn list_enabled_all(&self) -> Result<Vec<ScheduledReport>> {
            Ok(self.snapshot.clone())
        }
        async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
            unimplemented!()
        }
        async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
            if let Some(r) = self.fresh.lock().unwrap().iter_mut().find(|r| &r.id == id) {
                r.last_run_at = Some(ts);
            }
            self.touches
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        async fn record_delivery(&self, delivery: ReportDelivery) -> Result<()> {
            self.deliveries.lock().unwrap().push(delivery);
            Ok(())
        }
        async fn list_deliveries(&self, _: &Id, _: &Id) -> Result<Vec<ReportDelivery>> {
            Ok(self.deliveries())
        }
    }

    fn runner_with_gate(repo: Arc<Reports>, gate: Arc<dyn RunGate>) -> ScheduledReportRunner {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());

        ScheduledReportRunner::new(
            repo,
            store,
            None,
            "http://127.0.0.1:5080".into(),
            Arc::new(NoIdentity),
        )
        .with_gate(gate)
    }

    fn sharded_gate(
        nodes: &Arc<crate::app::cluster::ownership::test_support::Nodes>,
        node_id: &str,
        claims: &Arc<crate::domain::run_claim::MemoryRunClaims>,
    ) -> Arc<dyn RunGate> {
        let ownership = crate::app::cluster::WorkOwnership::new(
            nodes.clone(),
            node_id,
            crate::app::cluster::PeerRole::Alert,
            Duration::ZERO,
        );
        Arc::new(crate::app::cluster::ShardedRunGate::new(
            ownership,
            claims.clone(),
            node_id,
        ))
    }

    #[tokio::test]
    async fn a_due_report_runs_once_and_is_not_repeated_from_a_stale_listing() {
        let repo = Reports::new(vec![mk_report("json", None, "every:1d")]);
        let nodes = crate::app::cluster::ownership::test_support::Nodes::of(&[(
            "a",
            crate::app::cluster::PeerRole::Alert,
        )]);
        let claims = Arc::new(crate::domain::run_claim::MemoryRunClaims::default());
        let runner = runner_with_gate(repo.clone(), sharded_gate(&nodes, "a", &claims));

        assert_eq!(runner.tick_once().await.unwrap(), 1);
        assert_eq!(repo.touches(), 1);
        // 列表里的快照仍然显示「从未运行」，但认领后重读发现已经运行过：不会再发一遍。
        assert_eq!(runner.tick_once().await.unwrap(), 0);
        assert_eq!(repo.touches(), 1);
    }

    #[tokio::test]
    async fn a_report_held_by_another_node_is_not_run() {
        let report = mk_report("json", None, "every:1d");
        let repo = Reports::new(vec![report.clone()]);
        let claims = Arc::new(crate::domain::run_claim::MemoryRunClaims::default());
        // 两个节点各自只看到自己，都认为自己拥有这份报告。
        let mine = crate::app::cluster::ownership::test_support::Nodes::of(&[(
            "b",
            crate::app::cluster::PeerRole::Alert,
        )]);
        let theirs = crate::app::cluster::ownership::test_support::Nodes::of(&[(
            "a",
            crate::app::cluster::PeerRole::Alert,
        )]);
        let _running_on_a = sharded_gate(&theirs, "a", &claims)
            .admit(RUN_KIND, report.id.as_str())
            .await
            .unwrap()
            .expect("a claims the report");
        let runner = runner_with_gate(repo.clone(), sharded_gate(&mine, "b", &claims));

        assert_eq!(runner.tick_once().await.unwrap(), 0);
        assert_eq!(repo.touches(), 0);
    }

    #[tokio::test]
    async fn a_report_owned_by_another_node_is_left_alone() {
        let reports: Vec<ScheduledReport> = (0..20)
            .map(|_| mk_report("json", None, "every:1d"))
            .collect();
        let repo = Reports::new(reports);
        let nodes = crate::app::cluster::ownership::test_support::Nodes::of(&[
            ("a", crate::app::cluster::PeerRole::Alert),
            ("b", crate::app::cluster::PeerRole::Alert),
        ]);
        let claims = Arc::new(crate::domain::run_claim::MemoryRunClaims::default());
        let on_a = runner_with_gate(repo.clone(), sharded_gate(&nodes, "a", &claims));
        let on_b = runner_with_gate(repo.clone(), sharded_gate(&nodes, "b", &claims));

        let (ran_a, ran_b) = (
            on_a.tick_once().await.unwrap(),
            on_b.tick_once().await.unwrap(),
        );
        // 每份报告只被它的 owner 跑一次：两个节点都分到了，合计恰为全部。
        assert!(ran_a > 0 && ran_b > 0, "{ran_a} + {ran_b}");
        assert_eq!(ran_a + ran_b, 20);
        assert_eq!(repo.touches(), 20);
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
