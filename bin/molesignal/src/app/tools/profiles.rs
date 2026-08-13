// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*};
use crate::{
    app::iam::IamContext,
    domain::stream::{DEFAULT_PROFILE_STREAM, StreamType},
    infra::profiles::{self, NormalizedProfile, merge as profiles_merge},
    shared::{Error, Result, time::TimeRange},
};

const DEFAULT_MAX_PROFILES: usize = 200;
const HARD_MAX_PROFILES: usize = 1_000;
const SCAN_LIMIT: usize = 10_000;
const PROFILES_ENHANCED_FEATURE: &str = "profiling_enhanced";

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListContinuousProfiles => list(runtime, auth, arguments).await,
        BuiltinToolKind::GetProfileFlamegraph => flamegraph(runtime, auth, arguments).await,
        BuiltinToolKind::CompareProfiles => compare(runtime, auth, arguments).await,
        _ => unreachable!("profiles handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFilters {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    profile_type: Option<String>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    max_profiles: Option<usize>,
}

async fn list(runtime: &ToolRuntime, auth: &IamContext, arguments: Value) -> Result<ToolResult> {
    let args: ProfileFilters = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(100).clamp(1, HARD_MAX_PROFILES);
    let filters = sql_filters(&args);
    let statement = format!(
        "SELECT id, service, profile_type, total_value, sample_count, duration_nanos, unsymbolized, trace_id, span_id, labels, _timestamp FROM {}{} ORDER BY _timestamp DESC LIMIT {limit}",
        quote_ident(DEFAULT_PROFILE_STREAM),
        where_clause(&filters),
    );
    let Some(result) = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        DEFAULT_PROFILE_STREAM,
        StreamType::Profiles,
        limit,
    )
    .await?
    else {
        return Ok(ToolResult::json(json!({
            "profiles": [], "stream_available": false,
            "message": "Continuous Profiles has not received data yet",
        })));
    };
    let wanted = parse_label(args.label.as_deref());
    let profiles = rows_as_objects(&result)
        .into_iter()
        .filter(|row| row_matches_label(row.get("labels"), wanted.as_ref()))
        .collect::<Vec<_>>();
    Ok(ToolResult::json(json!({
        "profiles": profiles, "stream_available": true,
        "scanned_rows": result.scanned_rows, "took_ms": result.took_ms,
    })))
}

async fn flamegraph(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ProfileFilters = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let max_profiles = args
        .max_profiles
        .unwrap_or(DEFAULT_MAX_PROFILES)
        .clamp(1, HARD_MAX_PROFILES);
    let keys = object_keys(runtime, auth, range, &args).await?;
    let (sampled, truncated) = profiles_merge::even_sample(keys, max_profiles);
    let profiles = load_profiles(runtime, &sampled).await;
    Ok(ToolResult::json(json!({
        "flamebearer": profiles_merge::build_flamebearer(&profiles),
        "profile_count": profiles.len(), "truncated": truncated,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompareArgs {
    baseline: TimeRangeArg,
    comparison: TimeRangeArg,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    profile_type: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    max_profiles: Option<usize>,
}

async fn compare(runtime: &ToolRuntime, auth: &IamContext, arguments: Value) -> Result<ToolResult> {
    if !runtime.license.has_feature(PROFILES_ENHANCED_FEATURE) {
        return Err(Error::forbidden(
            "compare_profiles requires the profiling-enhanced feature (Pro edition)",
        ));
    }
    let args: CompareArgs = parse_args(arguments)?;
    let filters = ProfileFilters {
        service: args.service,
        profile_type: args.profile_type,
        label: args.label,
        ..ProfileFilters::default()
    };
    let max_profiles = args
        .max_profiles
        .unwrap_or(DEFAULT_MAX_PROFILES)
        .clamp(1, HARD_MAX_PROFILES);
    let baseline_keys = object_keys(runtime, auth, time_range(args.baseline)?, &filters).await?;
    let comparison_keys =
        object_keys(runtime, auth, time_range(args.comparison)?, &filters).await?;
    let (baseline_keys, baseline_truncated) =
        profiles_merge::even_sample(baseline_keys, max_profiles);
    let (comparison_keys, comparison_truncated) =
        profiles_merge::even_sample(comparison_keys, max_profiles);
    let baseline = load_profiles(runtime, &baseline_keys).await;
    let comparison = load_profiles(runtime, &comparison_keys).await;
    Ok(ToolResult::json(json!({
        "flamebearer": profiles_merge::build_diff(&baseline, &comparison),
        "baseline_count": baseline.len(), "comparison_count": comparison.len(),
        "truncated": baseline_truncated || comparison_truncated,
    })))
}

async fn object_keys(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: TimeRange,
    args: &ProfileFilters,
) -> Result<Vec<String>> {
    let filters = sql_filters(args);
    let statement = format!(
        "SELECT object_key, labels FROM {}{} ORDER BY _timestamp DESC LIMIT {SCAN_LIMIT}",
        quote_ident(DEFAULT_PROFILE_STREAM),
        where_clause(&filters),
    );
    let Some(result) = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        range,
        DEFAULT_PROFILE_STREAM,
        StreamType::Profiles,
        SCAN_LIMIT,
    )
    .await?
    else {
        return Ok(Vec::new());
    };
    let wanted = parse_label(args.label.as_deref());
    Ok(rows_as_objects(&result)
        .into_iter()
        .filter(|row| row_matches_label(row.get("labels"), wanted.as_ref()))
        .filter_map(|row| {
            row.get("object_key")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect())
}

fn sql_filters(args: &ProfileFilters) -> Vec<String> {
    let mut filters = Vec::new();
    push_string_filter(&mut filters, "service", args.service.as_deref());
    push_string_filter(&mut filters, "profile_type", args.profile_type.as_deref());
    push_string_filter(&mut filters, "trace_id", args.trace_id.as_deref());
    push_string_filter(&mut filters, "span_id", args.span_id.as_deref());
    filters
}

fn parse_label(value: Option<&str>) -> Option<(String, String)> {
    value?
        .split_once([':', '='])
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .filter(|(key, _)| !key.is_empty())
}

fn row_matches_label(labels: Option<&Value>, wanted: Option<&(String, String)>) -> bool {
    let Some((key, value)) = wanted else {
        return true;
    };
    labels
        .and_then(|labels| {
            labels.as_object().cloned().or_else(|| {
                labels
                    .as_str()
                    .and_then(|value| serde_json::from_str(value).ok())
            })
        })
        .and_then(|labels| labels.get(key).cloned())
        .and_then(|value| value.as_str().map(str::to_string))
        .is_some_and(|candidate| candidate == *value)
}

async fn load_profiles(runtime: &ToolRuntime, keys: &[String]) -> Vec<NormalizedProfile> {
    let mut loaded = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(profile) = load_profile(runtime, key).await else {
            continue;
        };
        loaded.push(profile);
    }
    loaded
}

async fn load_profile(runtime: &ToolRuntime, key: &str) -> Option<NormalizedProfile> {
    let raw = profiles::get_archive(&runtime.observability.object_store, key)
        .await
        .ok()?;
    let profile = profiles::decode_pprof(&raw).ok()?;
    Some(profiles::normalize_pprof(&profile, "", &BTreeMap::new()))
}
