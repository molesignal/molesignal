// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod actions;
mod proxy;
mod trace;

use std::{
    collections::HashMap,
    ffi::OsStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use headless_chrome::{
    Browser, LaunchOptions, Tab,
    browser::tab::RequestPausedDecision,
    protocol::cdp::{Emulation, Fetch, Fetch::events::RequestPausedEvent, Network, Page},
};
use sha2::{Digest as _, Sha256};
use url::{Host, Url};

use self::{proxy::EgressProxy, trace::TraceCapture};
use super::{
    ArtifactPayload, AttemptOutcome, ExecutionContext, now_micros, security::EgressGuard, unknown,
};
use crate::protocol::v1::{self as wire, ProbeOutcome, StepEvidence};

const MAX_SCREENSHOT_BYTES: usize = 10 * 1024 * 1024;
const MAX_HAR_BYTES: usize = 10 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 10;
const MAX_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;
const MAX_EXCERPT_BYTES: usize = 64 * 1024;
const FINALIZATION_GRACE: Duration = Duration::from_secs(3);

pub(super) async fn execute(
    task: &wire::ProbeTask,
    spec: &wire::BrowserJourneySpec,
    context: &mut ExecutionContext,
) -> Result<AttemptOutcome> {
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let spec = spec.clone();
    let context = context.clone();
    let blocking = tokio::task::spawn_blocking(move || run_blocking(spec, context, guard, timeout));
    match tokio::time::timeout(timeout.saturating_add(FINALIZATION_GRACE), blocking).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(error)) => Ok(unknown(
            "browser_runtime_failed",
            format!("Browser runtime worker failed: {error}"),
        )),
        Err(_) => Ok(unknown(
            "browser_timeout",
            "Browser Journey exceeded its execution timeout",
        )),
    }
}

fn run_blocking(
    spec: wire::BrowserJourneySpec,
    context: ExecutionContext,
    guard: EgressGuard,
    timeout: Duration,
) -> AttemptOutcome {
    let mut runtime = match BrowserRuntime::new(spec, context, guard, timeout) {
        Ok(runtime) => runtime,
        Err(error) => {
            return unknown(
                "browser_runtime_unavailable",
                format!("cannot start Browser runtime: {error}"),
            );
        }
    };
    let mut assertions = Vec::new();
    let mut evidence = Vec::with_capacity(runtime.spec.steps.len());
    let mut action_error = None;

    for step in runtime.spec.steps.clone() {
        let started_at = now_micros();
        let action = actions::action_name(&step).to_string();
        match actions::execute_step(&mut runtime, &step) {
            Ok(executed) => {
                let finished_at = now_micros();
                assertions.extend(executed.assertions);
                evidence.push(StepEvidence {
                    step_id: step.id,
                    name: step.name,
                    action,
                    started_at_micros: started_at,
                    finished_at_micros: finished_at,
                    outcome: executed.outcome as i32,
                    error_category: String::new(),
                    error_message: String::new(),
                    metadata: executed.metadata,
                });
            }
            Err(error) => {
                let message = runtime.context.redact(&error.to_string());
                evidence.push(StepEvidence {
                    step_id: step.id,
                    name: step.name,
                    action,
                    started_at_micros: started_at,
                    finished_at_micros: now_micros(),
                    outcome: ProbeOutcome::Failing as i32,
                    error_category: "browser_step_failed".into(),
                    error_message: message.clone(),
                    metadata: HashMap::new(),
                });
                action_error = Some(message);
                break;
            }
        }
    }

    let critical_failed = assertions.iter().any(|result| {
        !result.passed && result.severity == wire::AssertionSeverity::Critical as i32
    });
    let warning_failed = assertions
        .iter()
        .any(|result| !result.passed && result.severity == wire::AssertionSeverity::Warning as i32);
    let outcome = if action_error.is_some() || critical_failed {
        ProbeOutcome::Failing
    } else if warning_failed {
        ProbeOutcome::Degraded
    } else {
        ProbeOutcome::Healthy
    };

    let capture_failure = matches!(outcome, ProbeOutcome::Failing | ProbeOutcome::Degraded);
    let capture_trace = capture_failure && runtime.spec.capture_trace_on_failure;
    runtime.finish_trace(capture_trace);
    if capture_failure {
        runtime.capture_failure_artifacts();
    }
    let response_excerpt = if capture_failure {
        runtime.page_excerpt()
    } else {
        Vec::new()
    };
    let mut metadata = HashMap::from([
        (
            "browser.final_url".into(),
            runtime.context.redact(&runtime.tab.get_url()),
        ),
        ("browser.step_count".into(), evidence.len().to_string()),
        (
            "browser.artifact_count".into(),
            runtime.artifacts.len().to_string(),
        ),
    ]);
    if let Some(error) = runtime.artifact_error.take() {
        metadata.insert("browser.artifact_error".into(), error);
    }
    AttemptOutcome {
        outcome,
        assertions,
        response_excerpt,
        error_category: action_error
            .as_ref()
            .map_or_else(String::new, |_| "browser_step_failed".into()),
        error_message: action_error.unwrap_or_default(),
        metadata,
        evidence,
        artifacts: runtime.artifacts,
    }
}

pub(super) struct BrowserRuntime {
    _browser: Browser,
    _proxy: EgressProxy,
    pub(super) tab: Arc<Tab>,
    pub(super) spec: wire::BrowserJourneySpec,
    pub(super) context: ExecutionContext,
    guard: EgressGuard,
    denied_request: Arc<Mutex<Option<String>>>,
    deadline: Instant,
    artifacts: Vec<ArtifactPayload>,
    artifact_bytes: usize,
    artifact_error: Option<String>,
    trace: Option<TraceCapture>,
}

impl BrowserRuntime {
    fn new(
        spec: wire::BrowserJourneySpec,
        context: ExecutionContext,
        guard: EgressGuard,
        timeout: Duration,
    ) -> Result<Self> {
        if spec.steps.is_empty() || spec.steps.len() > 100 {
            bail!("Browser Journey requires 1 to 100 steps");
        }
        let viewport = spec.viewport.unwrap_or(wire::Viewport {
            width: 1280,
            height: 720,
        });
        if !(1..=7680).contains(&viewport.width) || !(1..=4320).contains(&viewport.height) {
            bail!("Browser viewport is outside the supported range");
        }
        let denied_request = Arc::new(Mutex::new(None));
        let deadline = Instant::now() + timeout;
        let proxy = EgressProxy::start(guard.clone(), denied_request.clone(), deadline)?;
        let proxy_endpoint = proxy.endpoint();
        let options = LaunchOptions::default_builder()
            .args(vec![
                OsStr::new("--disable-dev-shm-usage"),
                OsStr::new("--disable-quic"),
                OsStr::new("--no-first-run"),
                OsStr::new("--proxy-bypass-list=<-loopback>"),
                OsStr::new("--force-webrtc-ip-handling-policy=disable_non_proxied_udp"),
                OsStr::new("--js-flags=--max-old-space-size=512"),
            ])
            .proxy_server(Some(&proxy_endpoint))
            .ignore_certificate_errors(false)
            .build()
            .map_err(|error| anyhow!("build Chrome launch options: {error}"))?;
        let browser = Browser::new(options).context("launch Chrome")?;
        let tab = browser.new_tab().context("create Chrome tab")?;
        tab.set_default_timeout(timeout);
        tab.call_method(Emulation::SetDeviceMetricsOverride {
            width: viewport.width,
            height: viewport.height,
            device_scale_factor: 1.0,
            mobile: false,
            scale: None,
            screen_width: Some(viewport.width),
            screen_height: Some(viewport.height),
            position_x: None,
            position_y: None,
            dont_set_visible_size: None,
            screen_orientation: None,
            viewport: None,
            display_feature: None,
            device_posture: None,
        })?;
        if !spec.user_agent.trim().is_empty() {
            tab.set_user_agent(&spec.user_agent, None, None)?;
        }
        install_egress_interceptor(&tab, guard.clone(), denied_request.clone())?;
        let (trace, trace_error) = if spec.capture_trace_on_failure {
            match TraceCapture::start(&tab) {
                Ok(trace) => (Some(trace), None),
                Err(error) => (None, Some(format!("start Browser trace: {error}"))),
            }
        } else {
            (None, None)
        };
        Ok(Self {
            _browser: browser,
            _proxy: proxy,
            tab,
            spec,
            context,
            guard,
            denied_request,
            deadline,
            artifacts: Vec::new(),
            artifact_bytes: 0,
            artifact_error: trace_error,
            trace,
        })
    }

    pub(super) fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| anyhow!("Browser Journey timed out"))
    }

    pub(super) fn prepare_command(&self) -> Result<Duration> {
        let remaining = self.remaining()?;
        self.tab.set_default_timeout(remaining);
        Ok(remaining)
    }

    pub(super) fn validate_navigation(&self, target: &str) -> Result<Url> {
        let url = Url::parse(target).context("parse Browser navigation URL")?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("Browser navigation URL must use http or https");
        }
        validate_network_url(&self.guard, &url)?;
        Ok(url)
    }

    pub(super) fn check_egress_violation(&self) -> Result<()> {
        let violation = self
            .denied_request
            .lock()
            .map_err(|_| anyhow!("Browser egress state is unavailable"))?
            .take();
        match violation {
            Some(message) => bail!(message),
            None => Ok(()),
        }
    }

    pub(super) fn capture_screenshot(
        &mut self,
        name: &str,
        full_page: bool,
    ) -> Result<HashMap<String, String>> {
        self.prepare_command()?;
        let clip = if full_page {
            let metrics = self.tab.call_method(Page::GetLayoutMetrics(None))?;
            Some(Page::Viewport {
                x: 0.0,
                y: 0.0,
                width: metrics.css_content_size.width.min(7680.0),
                height: metrics.css_content_size.height.min(16_384.0),
                scale: 1.0,
            })
        } else {
            None
        };
        let bytes = self.tab.capture_screenshot(
            Page::CaptureScreenshotFormatOption::Png,
            None,
            clip,
            true,
        )?;
        if bytes.len() > MAX_SCREENSHOT_BYTES {
            bail!("Browser screenshot exceeds the 10 MiB limit");
        }
        self.record_artifact("screenshot", name, bytes)
    }

    fn capture_failure_artifacts(&mut self) {
        if self.spec.capture_screenshot_on_failure
            && let Err(error) = self.capture_screenshot("failure", true)
        {
            self.record_artifact_error(format!("capture failure screenshot: {error}"));
        }
        if self.spec.capture_har_on_failure
            && let Err(error) = self.capture_har()
        {
            self.record_artifact_error(format!("capture failure HAR: {error}"));
        }
    }

    fn finish_trace(&mut self, capture: bool) {
        let Some(mut trace) = self.trace.take() else {
            return;
        };
        let timeout = self
            .remaining()
            .unwrap_or(Duration::from_millis(100))
            .min(Duration::from_millis(750));
        self.tab.set_default_timeout(timeout);
        match trace.finish(&self.tab, &self.context, capture, timeout) {
            Ok(Some(bytes)) => {
                if let Err(error) = self.record_artifact("trace", "failure", bytes) {
                    self.record_artifact_error(format!("record Browser trace: {error}"));
                }
            }
            Ok(None) => {}
            Err(error) => self.record_artifact_error(format!("capture Browser trace: {error}")),
        }
    }

    fn capture_har(&mut self) -> Result<()> {
        self.prepare_command()?;
        let resources = actions::evaluate_string(
            &self.tab,
            r#"JSON.stringify([
                ...performance.getEntriesByType('navigation'),
                ...performance.getEntriesByType('resource')
            ].map((entry) => ({
                startedDateTime: new Date(performance.timeOrigin + entry.startTime).toISOString(),
                time: entry.duration,
                request: { method: 'GET', url: entry.name, httpVersion: entry.nextHopProtocol || '', headers: [], queryString: [], cookies: [], headersSize: -1, bodySize: 0 },
                response: { status: Number(entry.responseStatus || 0), statusText: '', httpVersion: entry.nextHopProtocol || '', headers: [], cookies: [], content: { size: Number(entry.decodedBodySize || 0), mimeType: '' }, redirectURL: '', headersSize: -1, bodySize: Number(entry.transferSize || 0) },
                cache: {},
                timings: { blocked: 0, dns: Math.max(0, entry.domainLookupEnd - entry.domainLookupStart), connect: Math.max(0, entry.connectEnd - entry.connectStart), ssl: entry.secureConnectionStart > 0 ? Math.max(0, entry.connectEnd - entry.secureConnectionStart) : -1, send: 0, wait: Math.max(0, entry.responseStart - entry.requestStart), receive: Math.max(0, entry.responseEnd - entry.responseStart) }
            })))"#,
        )?;
        let mut entries: serde_json::Value = serde_json::from_str(&resources)?;
        trace::redact_json(&mut entries, &self.context);
        let har = serde_json::to_vec(&serde_json::json!({
            "log": {
                "version": "1.2",
                "creator": { "name": "MoleSignal Probe Agent", "version": env!("CARGO_PKG_VERSION") },
                "entries": entries,
            }
        }))?;
        if har.len() > MAX_HAR_BYTES {
            bail!("Browser HAR exceeds the 10 MiB limit");
        }
        self.record_artifact("har", "failure", har).map(|_| ())
    }

    fn record_artifact(
        &mut self,
        kind: &str,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<HashMap<String, String>> {
        if self.artifacts.len() >= MAX_ARTIFACTS
            || self.artifact_bytes.saturating_add(bytes.len()) > MAX_ARTIFACT_BYTES
        {
            bail!("Browser Artifact budget exceeded");
        }
        let digest = hex::encode(Sha256::digest(&bytes));
        let content_length = bytes.len();
        self.artifact_bytes += content_length;
        self.artifacts.push(ArtifactPayload {
            kind: kind.to_string(),
            name: truncate(name, 128),
            bytes,
        });
        Ok(HashMap::from([
            ("artifact_kind".into(), kind.to_string()),
            ("artifact_name".into(), truncate(name, 128)),
            ("content_length".into(), content_length.to_string()),
            ("sha256".into(), digest),
        ]))
    }

    fn record_artifact_error(&mut self, error: String) {
        if self.artifact_error.is_none() {
            self.artifact_error = Some(truncate(&self.context.redact(&error), 4096));
        }
    }

    fn page_excerpt(&mut self) -> Vec<u8> {
        if self.prepare_command().is_err() {
            return Vec::new();
        }
        actions::evaluate_string(&self.tab, "document.body ? document.body.innerText : ''")
            .map(|value| truncate(&self.context.redact(&value), MAX_EXCERPT_BYTES).into_bytes())
            .unwrap_or_default()
    }
}

fn install_egress_interceptor(
    tab: &Tab,
    guard: EgressGuard,
    denied: Arc<Mutex<Option<String>>>,
) -> Result<()> {
    tab.enable_request_interception(Arc::new(
        move |_transport, _session, event: RequestPausedEvent| {
            let request_id = event.params.request_id.clone();
            let result = Url::parse(&event.params.request.url)
                .map_err(anyhow::Error::from)
                .and_then(|url| match url.scheme() {
                    "data" | "blob" | "about" => Ok(()),
                    "http" | "https" | "ws" | "wss" => validate_network_url(&guard, &url),
                    scheme => bail!("Browser request scheme `{scheme}` is not allowed"),
                });
            if let Err(error) = result {
                if let Ok(mut current) = denied.lock()
                    && current.is_none()
                {
                    *current = Some(format!("Browser egress policy rejected a request: {error}"));
                }
                RequestPausedDecision::Fail(Fetch::FailRequest {
                    request_id,
                    error_reason: Network::ErrorReason::AccessDenied,
                })
            } else {
                RequestPausedDecision::Continue(None)
            }
        },
    ))?;
    tab.enable_fetch(None, None)?;
    Ok(())
}

fn validate_network_url(guard: &EgressGuard, url: &Url) -> Result<()> {
    let host = match url.host() {
        Some(Host::Domain(host)) => host.to_string(),
        Some(Host::Ipv4(host)) => host.to_string(),
        Some(Host::Ipv6(host)) => host.to_string(),
        None => return Err(anyhow!("Browser request URL has no host")),
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("Browser request URL has no port"))?;
    guard.resolve_blocking(&host, port).map(|_| ())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
