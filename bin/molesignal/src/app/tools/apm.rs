// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::iam::IamContext;
use kernel::{
    Error, Result,
    time::{TimeRange, TimestampMicros},
};
use serde::Deserialize;
use serde_json::Value;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{TimeRangeArg, parse_args, time_range},
};
use crate::app::apm::{ApmQueryRequest, correlation::CorrelationReader};

mod catalog;

const DAY_MICROS: i64 = 86_400_000_000;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    // The catalog is searched by words, not read for a time range like the rest of APM.
    if kind == BuiltinToolKind::SearchServiceCatalog {
        return catalog::search(runtime, auth, arguments).await;
    }

    let mut args: ApmArgs = parse_args(arguments)?;

    // A service asked for by another name of it in the catalog is read as the service.
    let resolved = match args.service.as_deref().map(str::trim) {
        Some(asked) if !asked.is_empty() => {
            catalog::resolve_alias(runtime, auth, asked, &args.request()?).await?
        }
        _ => None,
    };

    if let Some(resolution) = &resolved {
        args.service = Some(resolution.service.clone());
    }

    let request = args.request()?;

    let mut value = match kind {
        BuiltinToolKind::ApmOverview => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count", "error_rate", "p95", "total_time"],
                "total_time",
            )?;

            serde_json::to_value(runtime.observability.apm.overview(&context).await?)
        }
        BuiltinToolKind::ListApmServices => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count", "error_rate", "p95", "name"],
                "request_count",
            )?;

            serde_json::to_value(runtime.observability.apm.services(&context).await?)
        }
        BuiltinToolKind::GetApmService => {
            if args.service.as_deref().is_none_or(str::is_empty) {
                return Err(Error::invalid("service is required"));
            }

            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(runtime.observability.apm.service_detail(&context).await?)
        }
        BuiltinToolKind::ListApmErrors => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["occurrence_count", "error_rate", "last_seen"],
                "occurrence_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .errors(&context, crate::app::apm::ErrorIssueFilter::All)
                    .await?,
            )
        }
        BuiltinToolKind::ListApmTransactions => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count", "error_rate", "p95", "name"],
                "request_count",
            )?;

            serde_json::to_value(runtime.observability.apm.transactions(&context).await?)
        }
        BuiltinToolKind::GetApmTransaction => {
            let transaction = required(&args.transaction, "transaction")?;

            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .transaction_detail(&context, transaction, None)
                    .await?,
            )
        }
        BuiltinToolKind::ListApmDependencies => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count", "error_rate", "p95", "name"],
                "request_count",
            )?;

            serde_json::to_value(runtime.observability.apm.dependencies(&context).await?)
        }
        BuiltinToolKind::GetApmError => {
            let fingerprint = required(&args.fingerprint, "fingerprint")?;

            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["occurrence_count"],
                "occurrence_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .error_detail(&context, fingerprint)
                    .await?,
            )
        }
        BuiltinToolKind::CompareApmVersions => {
            let baseline = required(&args.baseline_version, "baselineVersion")?;
            let candidate = required(&args.candidate_version, "candidateVersion")?;

            if args.service.as_deref().is_none_or(str::is_empty) {
                return Err(Error::invalid("service is required"));
            }

            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .compare_versions(&context, baseline, candidate)
                    .await?,
            )
        }
        BuiltinToolKind::GetApmHealth => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .tenant_health(&context, runtime.observability.apm_runtime.as_deref())
                    .await?,
            )
        }
        BuiltinToolKind::GetApmInsights => {
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                request,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(runtime.observability.apm.insights(&context).await?)
        }
        BuiltinToolKind::GetApmBlastRadius => {
            let service = required(&args.service, "service")?.to_owned();
            // The whole map of the environment: what reaches the service crosses namespaces.
            let whole = ApmQueryRequest {
                service: None,
                namespace: None,
                ..request
            };
            let context = runtime.observability.apm.context(
                auth.org_id.clone(),
                whole,
                &["request_count"],
                "request_count",
            )?;

            serde_json::to_value(
                runtime
                    .observability
                    .apm
                    .blast_radius(
                        &context,
                        runtime.observability.service_graph.as_ref(),
                        &service,
                    )
                    .await?,
            )
        }
        BuiltinToolKind::GetCorrelationQuality => {
            let reader = CorrelationReader {
                query: &runtime.observability.query,
                streams: runtime.observability.streams.as_ref(),
                apm: &runtime.observability.apm,
                catalog: runtime.observability.service_catalog.as_ref(),
            };

            // The platform's own log stream stays with the platform's pages.
            serde_json::to_value(reader.report(&auth.org_id, request, false).await?)
        }
        _ => unreachable!("APM handler received unrelated tool"),
    }
    .map_err(|error| Error::internal(error.to_string()))?;

    if let Some(resolution) = &resolved
        && let Some(object) = value.as_object_mut()
    {
        object.insert("serviceAlias".into(), resolution.note());
    }

    Ok(ToolResult::json(value))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApmArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    transaction: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    baseline_version: Option<String>,
    #[serde(default)]
    candidate_version: Option<String>,
}

fn required<'a>(value: &'a Option<String>, field: &str) -> Result<&'a str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::invalid(format!("{field} is required")))
}

impl ApmArgs {
    fn request(&self) -> Result<ApmQueryRequest> {
        let range = self
            .time_range
            .map(time_range)
            .transpose()?
            .unwrap_or_else(|| {
                let end = TimestampMicros::now();
                TimeRange::new(TimestampMicros(end.0.saturating_sub(DAY_MICROS)), end)
            });

        let sort = self.sort.as_deref().map(|sort| match sort {
            "p95_time" => "p95".to_string(),
            value => value.to_string(),
        });

        Ok(ApmQueryRequest {
            from: range.start.0,
            to: range.end.0,
            namespace: self.namespace.clone(),
            service: self.service.clone(),
            environment: self.environment.clone(),
            version: self.version.clone(),
            sort,
            limit: Some(self.limit.unwrap_or(25).clamp(1, 100)),
            ..ApmQueryRequest::default()
        })
    }
}
