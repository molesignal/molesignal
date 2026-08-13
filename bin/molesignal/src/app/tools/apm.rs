// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::Value;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{TimeRangeArg, parse_args, time_range},
};
use crate::{
    app::{apm::ApmQueryRequest, iam::IamContext},
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const DAY_MICROS: i64 = 86_400_000_000;

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ApmArgs = parse_args(arguments)?;
    let request = args.request()?;
    let value = match kind {
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
            serde_json::to_value(runtime.observability.apm.errors(&context).await?)
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
            let baseline = required(&args.baseline_version, "baseline_version")?;
            let candidate = required(&args.candidate_version, "candidate_version")?;
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
        _ => unreachable!("APM handler received unrelated tool"),
    }
    .map_err(|error| Error::internal(error.to_string()))?;
    Ok(ToolResult::json(value))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
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
            "p95_micros" => "p95".to_string(),
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
