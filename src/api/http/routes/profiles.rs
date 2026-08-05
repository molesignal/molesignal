// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Continuous Profiling HTTP 端点（持续性能分析）。
//!
//! 摄取：
//! - `POST /profiles/upload`   pprof 直传（JFR 待实现，task 2.6）
//! - `POST /profiles/ingest`   Pyroscope 兼容：`name{labels}` + `format=pprof|folded|lines`
//! - `POST /profiles/otlp`     OTLP Profiles 适配器（Alpha，proto 待 vendoring，task 2.5）
//!
//! 查询 / 聚合：
//! - `GET  /profiles`            元数据列表 / 筛选
//! - `GET  /profiles/flamegraph` 窗口内合并火焰图（flamebearer）；支持 trace 关联过滤
//! - `POST /profiles/flamegraph/selection` 精确合并用户选中的 profile
//! - `GET  /profiles/diff`       baseline vs comparison 差分火焰图
//! - `GET  /profiles/{id}`       原始 pprof 下载
//!
//! 与自诊断 `profiling`（`/debug/profile/*`，本服务运维）相互独立：本模块面向**被
//! 观测的用户应用**。三协议归一化后统一双路落盘：规范 pprof + zstd 旁路归档 object
//! store，元数据行经 `IngestService` 进 `StreamType::Profiles` 流。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use opentelemetry_proto::tonic::{
    collector::profiles::v1development::ExportProfilesServiceRequest,
    common::v1::AnyValue,
    profiles::v1development::{Profile, ProfilesDictionary},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{
        AppState,
        http::routes::ingest::otlp::{any_value_to_json, decode_otlp, detect_encoding, hex},
    },
    app::iam::IamContext,
    domain::{
        iam::{IamScope, permission},
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{DEFAULT_PROFILE_STREAM, MOLESIGNAL_SYSTEM_STREAM, Schema, StreamType},
    },
    infra::profiles::{
        self, Frame, NormalizedProfile, ProfileType, Sample, ValueType,
        merge::{self as profiles_merge, DiffFlamebearer, Flamebearer},
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// `_timestamp` 隐式时间列（与存储 `arrow_schema::TS_COL` 对齐）。
const TS_COL: &str = "_timestamp";
/// 火焰图窗口内合并 profile 数默认上限；超限均匀采样 + `truncated`。
const DEFAULT_MAX_MERGE: usize = 1_000;
/// 聚合时扫描元数据行的硬上限（防止超大窗口拉爆）。
const SCAN_CEILING: usize = 50_000;
/// 列表 / 聚合默认回看窗口（µs）：1 小时。
const DEFAULT_LOOKBACK_US: i64 = 3_600 * 1_000_000;
/// 增强型 profiling 能力的 license feature（diff / 跨服务大窗口聚合 / 长保留 /
/// 服务端符号化 / Pyroscope render 出口）。OSS 核心不门禁，仅这些增强项门禁。
const PROFILES_ENHANCED_FEATURE: &str = "profiling_enhanced";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/profiles", get(list_profiles))
        .route("/profiles/upload", post(upload))
        .route("/profiles/ingest", post(pyroscope_ingest))
        .route("/profiles/otlp", post(otlp_profiles))
        .route("/profiles/flamegraph", get(flamegraph))
        .route("/profiles/flamegraph/selection", post(flamegraph_selection))
        .route("/profiles/diff", get(diff))
        .route("/profiles/{id}", get(download))
}

// ===== 通用 helper =====

#[derive(Debug, Serialize)]
struct IngestAck {
    accepted: usize,
}

/// 单引号转义（SQL 字符串字面量）。
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// 增强型 profiling 门禁使用统一的 `license.has_feature` 模式。OSS 核心
/// （三协议摄取、单窗口火焰图、列表、trace 关联）不调用此函数；diff 等增强项调用，
/// 未授权时返 403 + 明确说明所需 edition（非裸 403，前端据此渲染门禁页）。
fn require_profiles_enhanced(state: &AppState) -> Result<()> {
    if !state
        .platform
        .license
        .has_feature(PROFILES_ENHANCED_FEATURE)
    {
        return Err(Error::forbidden(
            "differential profiling requires the profiling-enhanced feature (Pro edition)",
        ));
    }
    Ok(())
}

fn col(res: &QueryResult, name: &str) -> Option<usize> {
    res.columns.iter().position(|c| c == name)
}

fn cell(row: &[Value], idx: Option<usize>) -> Option<&Value> {
    idx.and_then(|i| row.get(i))
}

fn str_cell(row: &[Value], idx: Option<usize>) -> Option<String> {
    cell(row, idx).and_then(Value::as_str).map(str::to_string)
}

fn i64_cell(row: &[Value], idx: Option<usize>) -> Option<i64> {
    cell(row, idx).and_then(Value::as_i64)
}

/// `label=k:v`（或 `k=v`）→ `(k, v)`。
fn parse_label(s: &str) -> Option<(String, String)> {
    s.split_once([':', '='])
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
}

/// Pyroscope `from` / `until`：unix 秒 / 毫秒 / 微秒（按量级判定）→ µs。
fn parse_time_param(s: Option<&str>) -> Option<i64> {
    let s = s?.trim();
    let n: i64 = s.parse().ok()?;
    Some(if n >= 1_000_000_000_000_000 {
        n // micros
    } else if n >= 1_000_000_000_000 {
        n * 1_000 // millis
    } else {
        n * 1_000_000 // seconds（含较小值）
    })
}

fn time_range(from: Option<i64>, to: Option<i64>) -> TimeRange {
    let end = to.unwrap_or_else(|| TimestampMicros::now().0);
    let start = from.unwrap_or(end - DEFAULT_LOOKBACK_US);
    TimeRange::new(TimestampMicros(start), TimestampMicros(end))
}

fn profiles_stream(scope: IamScope) -> &'static str {
    if scope == IamScope::System {
        MOLESIGNAL_SYSTEM_STREAM
    } else {
        DEFAULT_PROFILE_STREAM
    }
}

async fn run_query(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    required_fields: &[&str],
    statement: String,
    range: TimeRange,
    limit: Option<usize>,
) -> Result<QueryResult> {
    // `_sys/_molesignal` is created before the first scheduled self-profile. Until schema-on-write
    // observes that first metadata event, selecting Profile columns would fail during planning.
    // A missing/uninitialized Profile stream therefore means "no profiles yet", not invalid SQL.
    let definition = match state
        .telemetry
        .streams
        .get(org_id, stream, StreamType::Profiles)
        .await
    {
        Ok(definition) => definition,
        Err(Error::NotFound(_)) => return Ok(empty_query_result()),
        Err(error) => return Err(error),
    };
    if !schema_contains_fields(&definition.schema, required_fields) {
        return Ok(empty_query_result());
    }
    state
        .query
        .run(QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: range,
            stream: Some(StreamHint {
                name: stream.to_string(),
                stream_type: StreamType::Profiles,
            }),
            limit,
            federation_clusters: Vec::new(),
        })
        .await
}

fn schema_contains_fields(schema: &Schema, required_fields: &[&str]) -> bool {
    required_fields
        .iter()
        .all(|required| schema.fields.iter().any(|field| field.name == *required))
}

fn empty_query_result() -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    }
}

/// 双路落盘：归档 zstd pprof → object store；元数据 RawEvent → `IngestService`。
/// `pub(crate)`：OTLP profiles 的 gRPC 入口（`grpc::otlp_server`）也走这条落盘管道。
pub(crate) async fn store_profile(
    state: &AppState,
    org_id: &Id,
    normalized: &NormalizedProfile,
    raw_pprof: &[u8],
    request_bytes: usize,
) -> Result<()> {
    // 计费 / 配额门禁（与 OTLP / native 摄取同源）。
    crate::api::http::billing::ensure_ingest_allowed(
        state,
        org_id,
        request_bytes as u64,
        TimestampMicros::now().0,
    )
    .await?;

    state
        .telemetry
        .profile_storage
        .store_public(org_id, normalized, raw_pprof)
        .await
}

// ===== 摄取 handler =====

#[derive(Debug, Deserialize)]
struct UploadParams {
    service: Option<String>,
    #[serde(alias = "profile_type")]
    r#type: Option<String>,
    /// `pprof`（默认）| `jfr`（待实现）。
    format: Option<String>,
}

/// `POST /profiles/upload`：pprof 直传（task 2.2）。
#[permission("streams.write")]
async fn upload(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<UploadParams>,
    body: Bytes,
) -> Result<Response> {
    let format = params
        .format
        .as_deref()
        .unwrap_or("pprof")
        .to_ascii_lowercase();
    if format == "jfr" {
        return Err(Error::invalid(
            "JFR upload not yet implemented; send pprof (task 2.6)",
        ));
    }
    if format != "pprof" {
        return Err(Error::invalid(format!(
            "unsupported upload format: {format}"
        )));
    }
    if body.is_empty() {
        return Err(Error::invalid("empty profile body"));
    }
    let service = params
        .service
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let raw = profiles::decompress_pprof_input(&body)?;
    let profile = profiles::decode_pprof(&raw)?;
    let mut normalized = profiles::normalize_pprof(&profile, service, &BTreeMap::new());
    if let Some(t) = params.r#type.as_deref().filter(|t| !t.is_empty()) {
        normalized.profile_type = ProfileType::from_name(t);
    }

    store_profile(&state, &ctx.org_id, &normalized, &raw, body.len()).await?;
    Ok((StatusCode::ACCEPTED, Json(IngestAck { accepted: 1 })).into_response())
}

#[derive(Debug, Deserialize)]
struct PyroscopeParams {
    name: Option<String>,
    from: Option<String>,
    until: Option<String>,
    format: Option<String>,
}

/// `POST /profiles/ingest`：Pyroscope 兼容摄取（task 2.3）。
#[permission("streams.write")]
async fn pyroscope_ingest(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<PyroscopeParams>,
    body: Bytes,
) -> Result<Response> {
    let name = params.name.unwrap_or_default();
    let (service_raw, type_hint, labels) = profiles::parse_pyroscope_name(&name);
    let service = if service_raw.is_empty() {
        "unknown".to_string()
    } else {
        service_raw
    };
    let format = params
        .format
        .as_deref()
        .unwrap_or("pprof")
        .to_ascii_lowercase();

    let (mut normalized, raw) = match format.as_str() {
        "pprof" => {
            let raw = profiles::decompress_pprof_input(&body)?;
            let profile = profiles::decode_pprof(&raw)?;
            let normalized = profiles::normalize_pprof(&profile, &service, &labels);
            (normalized, raw)
        }
        "folded" | "collapsed" | "lines" => {
            let text = std::str::from_utf8(&body)
                .map_err(|_| Error::invalid("folded body must be UTF-8"))?;
            let type_name = type_hint.as_deref().unwrap_or("samples");
            let normalized = profiles::parse_folded(text, &service, type_name, &labels)?;
            let raw = profiles::encode_pprof_raw(&normalized)?;
            (normalized, raw)
        }
        "jfr" => {
            return Err(Error::invalid(
                "JFR ingest not yet implemented; use pprof/folded (task 2.6)",
            ));
        }
        other => {
            return Err(Error::invalid(format!(
                "unsupported profile format: {other}"
            )));
        }
    };

    if let Some(t) = type_hint.as_deref() {
        normalized.profile_type = ProfileType::from_name(t);
    }
    if let Some(from_us) = parse_time_param(params.from.as_deref()) {
        normalized.start_time_micros = from_us;
        if let Some(until_us) = parse_time_param(params.until.as_deref()) {
            normalized.duration_nanos = (until_us - from_us).max(0).saturating_mul(1_000);
        }
    }

    store_profile(&state, &ctx.org_id, &normalized, &raw, body.len()).await?;
    Ok((StatusCode::OK, Json(IngestAck { accepted: 1 })).into_response())
}

/// `POST /profiles/otlp`：OTLP Profiles 摄取（OTLP/HTTP，protobuf 或 OTLP/JSON）。
///
/// 解码 `ExportProfilesServiceRequest`（请求级共享 dictionary + 多 profile）→ 归一化
/// 为多个 [`NormalizedProfile`]，逐个编码规范 pprof 后走统一双路落盘。gRPC 入口
/// （`grpc::otlp_server`）复用同一个 [`normalize_otlp_profiles`]。
#[permission("streams.write")]
async fn otlp_profiles(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let enc = detect_encoding(&headers)?;
    let req: ExportProfilesServiceRequest = decode_otlp(enc, &body)?;
    let normalized = normalize_otlp_profiles(&req);
    let mut accepted = 0usize;
    for profile in &normalized {
        let raw = profiles::encode_pprof_raw(profile)?;
        store_profile(&state, &ctx.org_id, profile, &raw, body.len()).await?;
        accepted += 1;
    }
    Ok((StatusCode::OK, Json(IngestAck { accepted })).into_response())
}

/// OTLP `ExportProfilesServiceRequest` → 多个 [`NormalizedProfile`]（每个
/// resource × scope × profile 一个）。所有间接寻址走请求级共享 `ProfilesDictionary`
/// 的下标表（`[0]`=null）。HTTP（上）与 gRPC（`grpc::otlp_server`）两个 OTLP profiles
/// 入口共用此函数。
pub(crate) fn normalize_otlp_profiles(
    req: &ExportProfilesServiceRequest,
) -> Vec<NormalizedProfile> {
    let Some(dict) = req.dictionary.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for rp in &req.resource_profiles {
        let mut base_labels: BTreeMap<String, String> = BTreeMap::new();
        let mut service = String::new();
        if let Some(res) = rp.resource.as_ref() {
            for kv in &res.attributes {
                let v = kv
                    .value
                    .as_ref()
                    .map(any_value_to_label)
                    .unwrap_or_default();
                if kv.key == "service.name" {
                    service = v.clone();
                }
                base_labels.insert(kv.key.clone(), v);
            }
        }
        if service.is_empty() {
            service = "unknown".to_string();
        }
        for sp in &rp.scope_profiles {
            let mut labels = base_labels.clone();
            if let Some(scope) = sp.scope.as_ref() {
                for kv in &scope.attributes {
                    let v = kv
                        .value
                        .as_ref()
                        .map(any_value_to_label)
                        .unwrap_or_default();
                    labels.insert(kv.key.clone(), v);
                }
            }
            for p in &sp.profiles {
                out.push(convert_otlp_profile(p, dict, &service, &labels));
            }
        }
    }
    out
}

fn convert_otlp_profile(
    p: &Profile,
    dict: &ProfilesDictionary,
    service: &str,
    base_labels: &BTreeMap<String, String>,
) -> NormalizedProfile {
    let sample_type = p
        .sample_type
        .as_ref()
        .map(|vt| {
            ValueType::new(
                dict_str(dict, vt.type_strindex),
                dict_str(dict, vt.unit_strindex),
            )
        })
        .unwrap_or_else(|| ValueType::new("samples", "count"));
    let period_type = p.period_type.as_ref().map(|vt| {
        ValueType::new(
            dict_str(dict, vt.type_strindex),
            dict_str(dict, vt.unit_strindex),
        )
    });

    let mut labels = base_labels.clone();
    for &ai in &p.attribute_indices {
        if let Some((k, v)) = dict_attr(dict, ai) {
            labels.insert(k, v);
        }
    }

    let mut samples = Vec::with_capacity(p.samples.len());
    let mut trace_id: Option<String> = None;
    let mut span_id: Option<String> = None;
    for s in &p.samples {
        let mut stack: Vec<Frame> = Vec::new();
        if let Some(stk) = dict.stack_table.get(s.stack_index.max(0) as usize) {
            for &loc_idx in &stk.location_indices {
                let Some(loc) = dict.location_table.get(loc_idx.max(0) as usize) else {
                    continue;
                };
                if loc.lines.is_empty() {
                    // 未符号化帧：仅地址 + mapping build_id（OTLP 里 build_id 是属性）。
                    stack.push(Frame {
                        function: String::new(),
                        file: None,
                        line: None,
                        address: (loc.address != 0).then_some(loc.address),
                        build_id: mapping_build_id(dict, loc.mapping_index),
                    });
                } else {
                    for ln in &loc.lines {
                        let func = dict.function_table.get(ln.function_index.max(0) as usize);
                        let name = func
                            .map(|f| dict_str(dict, f.name_strindex))
                            .unwrap_or("")
                            .to_string();
                        let file = func
                            .map(|f| dict_str(dict, f.filename_strindex))
                            .filter(|s| !s.is_empty())
                            .map(str::to_string);
                        stack.push(Frame {
                            function: name,
                            file,
                            line: (ln.line != 0).then_some(ln.line),
                            address: None,
                            build_id: None,
                        });
                    }
                }
            }
        }
        stack.reverse(); // OTLP/pprof 叶子在前 → NormalizedProfile 根在前

        // values：多值求和成标量；仅有时间戳时按观测计数（NormalizedProfile 是聚合视图）。
        let value = if s.values.is_empty() {
            s.timestamps_unix_nano.len().max(1) as i64
        } else {
            s.values.iter().copied().sum()
        };

        let mut sample_labels: BTreeMap<String, String> = BTreeMap::new();
        for &ai in &s.attribute_indices {
            if let Some((k, v)) = dict_attr(dict, ai) {
                sample_labels.insert(k, v);
            }
        }

        // trace 关联：OTLP 用结构化 Link（trace_id/span_id 字节）。
        if s.link_index > 0
            && let Some(link) = dict.link_table.get(s.link_index as usize)
        {
            if trace_id.is_none() && !link.trace_id.is_empty() {
                trace_id = Some(hex(&link.trace_id));
            }
            if span_id.is_none() && !link.span_id.is_empty() {
                span_id = Some(hex(&link.span_id));
            }
        }

        samples.push(Sample {
            stack,
            values: vec![value],
            labels: sample_labels,
        });
    }

    // 兜底：个别 SDK 把 trace/span 放属性而非 Link。
    if trace_id.is_none() {
        trace_id = labels.get("trace_id").filter(|v| !v.is_empty()).cloned();
    }
    if span_id.is_none() {
        span_id = labels.get("span_id").filter(|v| !v.is_empty()).cloned();
    }

    NormalizedProfile {
        service: service.to_string(),
        profile_type: ProfileType::from_name(&sample_type.ty),
        sample_types: vec![sample_type],
        default_value_index: 0,
        samples,
        period_type,
        period: p.period,
        start_time_micros: (p.time_unix_nano / 1_000) as i64,
        duration_nanos: p.duration_nano as i64,
        labels,
        trace_id,
        span_id,
    }
}

/// dictionary `string_table` 解引用（`[0]`="" / 越界 → ""）。
fn dict_str(dict: &ProfilesDictionary, i: i32) -> &str {
    if i <= 0 {
        return "";
    }
    dict.string_table
        .get(i as usize)
        .map(String::as_str)
        .unwrap_or("")
}

/// dictionary `attribute_table` 解引用 → `(key, value-as-string)`。
fn dict_attr(dict: &ProfilesDictionary, i: i32) -> Option<(String, String)> {
    if i <= 0 {
        return None;
    }
    let kv = dict.attribute_table.get(i as usize)?;
    let key = dict_str(dict, kv.key_strindex).to_string();
    if key.is_empty() {
        return None;
    }
    let val = kv
        .value
        .as_ref()
        .map(any_value_to_label)
        .unwrap_or_default();
    Some((key, val))
}

/// 从 mapping 属性里尽力取 build_id（OTLP 把 build_id 放属性，非 pprof 的字符串字段）。
fn mapping_build_id(dict: &ProfilesDictionary, mapping_index: i32) -> Option<String> {
    if mapping_index <= 0 {
        return None;
    }
    let m = dict.mapping_table.get(mapping_index as usize)?;
    for &ai in &m.attribute_indices {
        if let Some((k, v)) = dict_attr(dict, ai)
            && k.contains("build_id")
            && !v.is_empty()
        {
            return Some(v);
        }
    }
    None
}

/// OTLP `AnyValue` → label 字符串（string 取原值，其余 JSON 化）。
fn any_value_to_label(av: &AnyValue) -> String {
    match any_value_to_json(av) {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ===== 查询 / 聚合 handler =====

#[derive(Debug, Deserialize)]
struct ListParams {
    service: Option<String>,
    #[serde(alias = "profile_type")]
    r#type: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    label: Option<String>,
    trace_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ProfileEntry {
    id: String,
    service: String,
    profile_type: String,
    timestamp: i64,
    total_value: i64,
    sample_count: i64,
    duration_nanos: i64,
    unsymbolized: bool,
    trace_id: Option<String>,
    span_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListResponse {
    profiles: Vec<ProfileEntry>,
}

/// `GET /profiles`：元数据列表 / 筛选（task 4.1）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list_profiles(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<ListParams>,
) -> Result<Response> {
    let range = time_range(params.from, params.to);
    let mut conds: Vec<String> = Vec::new();
    if let Some(s) = params.service.as_deref().filter(|s| !s.is_empty()) {
        conds.push(format!("service = '{}'", sql_escape(s)));
    }
    if let Some(t) = params.r#type.as_deref().filter(|t| !t.is_empty()) {
        conds.push(format!("profile_type = '{}'", sql_escape(t)));
    }
    if let Some(t) = params.trace_id.as_deref().filter(|t| !t.is_empty()) {
        conds.push(format!("trace_id = '{}'", sql_escape(t)));
    }
    let where_clause = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };
    let stream = profiles_stream(ctx.scope);
    let statement = format!(
        "SELECT id, service, profile_type, total_value, sample_count, duration_nanos, \
         unsymbolized, trace_id, span_id, labels, {TS_COL} FROM \"{stream}\"{where_clause} \
         ORDER BY {TS_COL} DESC"
    );
    let limit = params.limit.unwrap_or(200).min(1_000);
    let res = run_query(
        &state,
        &ctx.org_id,
        stream,
        &[
            "id",
            "service",
            "profile_type",
            "total_value",
            "sample_count",
            "duration_nanos",
            "unsymbolized",
            "trace_id",
            "span_id",
            "labels",
        ],
        statement,
        range,
        Some(limit),
    )
    .await?;

    let want_label = params.label.as_deref().and_then(parse_label);
    let (id_i, svc_i, type_i) = (
        col(&res, "id"),
        col(&res, "service"),
        col(&res, "profile_type"),
    );
    let (tv_i, sc_i, dur_i) = (
        col(&res, "total_value"),
        col(&res, "sample_count"),
        col(&res, "duration_nanos"),
    );
    let (uns_i, trace_i, span_i, lbl_i, ts_i) = (
        col(&res, "unsymbolized"),
        col(&res, "trace_id"),
        col(&res, "span_id"),
        col(&res, "labels"),
        col(&res, TS_COL),
    );

    let mut profiles_out = Vec::with_capacity(res.rows.len());
    for row in &res.rows {
        if !row_matches_label(cell(row, lbl_i), &want_label) {
            continue;
        }
        profiles_out.push(ProfileEntry {
            id: str_cell(row, id_i).unwrap_or_default(),
            service: str_cell(row, svc_i).unwrap_or_default(),
            profile_type: str_cell(row, type_i).unwrap_or_default(),
            timestamp: i64_cell(row, ts_i).unwrap_or(0),
            total_value: i64_cell(row, tv_i).unwrap_or(0),
            sample_count: i64_cell(row, sc_i).unwrap_or(0),
            duration_nanos: i64_cell(row, dur_i).unwrap_or(0),
            unsymbolized: cell(row, uns_i).and_then(Value::as_bool).unwrap_or(false),
            trace_id: str_cell(row, trace_i).filter(|s| !s.is_empty()),
            span_id: str_cell(row, span_i).filter(|s| !s.is_empty()),
        });
    }
    Ok(Json(ListResponse {
        profiles: profiles_out,
    })
    .into_response())
}

fn row_matches_label(labels: Option<&Value>, want: &Option<(String, String)>) -> bool {
    let Some((k, v)) = want else {
        return true;
    };
    labels
        .and_then(Value::as_object)
        .and_then(|m| m.get(k))
        .and_then(Value::as_str)
        .map(|got| got == v)
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct FlamegraphParams {
    service: Option<String>,
    #[serde(alias = "profile_type")]
    r#type: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    label: Option<String>,
    trace_id: Option<String>,
    span_id: Option<String>,
    max_merge: Option<usize>,
}

#[derive(Debug, Serialize)]
struct FlamegraphResponse {
    flamebearer: Flamebearer,
    truncated: bool,
    profile_count: usize,
}

#[derive(Debug, Deserialize)]
struct FlamegraphSelectionRequest {
    profile_ids: Vec<String>,
    max_merge: Option<usize>,
}

/// `GET /profiles/flamegraph`：窗口内合并火焰图（task 4.3 / 4.2 / 4.6）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn flamegraph(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<FlamegraphParams>,
) -> Result<Response> {
    let range = time_range(params.from, params.to);
    let mut conds: Vec<String> = Vec::new();
    if let Some(s) = params.service.as_deref().filter(|s| !s.is_empty()) {
        conds.push(format!("service = '{}'", sql_escape(s)));
    }
    if let Some(t) = params.r#type.as_deref().filter(|t| !t.is_empty()) {
        conds.push(format!("profile_type = '{}'", sql_escape(t)));
    }
    if let Some(t) = params.trace_id.as_deref().filter(|t| !t.is_empty()) {
        conds.push(format!("trace_id = '{}'", sql_escape(t)));
    }
    if let Some(s) = params.span_id.as_deref().filter(|s| !s.is_empty()) {
        conds.push(format!("span_id = '{}'", sql_escape(s)));
    }
    let max_merge = params.max_merge.unwrap_or(DEFAULT_MAX_MERGE).max(1);
    let want_label = params.label.as_deref().and_then(parse_label);

    let keys = fetch_object_keys(
        &state,
        &ctx.org_id,
        profiles_stream(ctx.scope),
        &conds,
        range,
        &want_label,
    )
    .await?;
    Ok(Json(build_flamegraph_response(&state, keys, max_merge).await).into_response())
}

/// `POST /profiles/flamegraph/selection`：按列表中用户勾选的 profile 精确合并。
///
/// 使用 JSON body 而非把最多 1,000 个 ID 放进 query string，避免代理层 URL
/// 长度上限，并确保“参与分析的 Profiles”选择真正决定分析结果。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn flamegraph_selection(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<FlamegraphSelectionRequest>,
) -> Result<Response> {
    let mut profile_ids = req
        .profile_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    profile_ids.sort();
    profile_ids.dedup();
    if profile_ids.len() > DEFAULT_MAX_MERGE {
        return Err(Error::invalid(format!(
            "at most {DEFAULT_MAX_MERGE} profiles can be selected"
        )));
    }

    let keys = if profile_ids.is_empty() {
        Vec::new()
    } else {
        let selected = profile_ids
            .iter()
            .map(|id| format!("'{}'", sql_escape(id)))
            .collect::<Vec<_>>()
            .join(", ");
        let conds = vec![format!("id IN ({selected})")];
        let range = TimeRange::new(TimestampMicros(0), TimestampMicros::now());
        fetch_object_keys(
            &state,
            &ctx.org_id,
            profiles_stream(ctx.scope),
            &conds,
            range,
            &None,
        )
        .await?
    };
    let max_merge = req
        .max_merge
        .unwrap_or(DEFAULT_MAX_MERGE)
        .clamp(1, DEFAULT_MAX_MERGE);
    Ok(Json(build_flamegraph_response(&state, keys, max_merge).await).into_response())
}

#[derive(Debug, Deserialize)]
struct DiffParams {
    service: Option<String>,
    #[serde(alias = "profile_type")]
    r#type: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(alias = "baselineFrom")]
    baseline_from: Option<i64>,
    #[serde(alias = "baselineTo")]
    baseline_to: Option<i64>,
    label: Option<String>,
    max_merge: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DiffResponse {
    flamebearer: DiffFlamebearer,
    truncated: bool,
    baseline_count: usize,
    comparison_count: usize,
}

/// `GET /profiles/diff`：baseline vs comparison 差分（task 4.4）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn diff(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<DiffParams>,
) -> Result<Response> {
    // 差分火焰图是增强项（Decision 6）：OSS 返 403 + 所需 edition，前端渲染门禁。
    require_profiles_enhanced(&state)?;
    let comparison_range = time_range(params.from, params.to);
    let baseline_range = time_range(params.baseline_from, params.baseline_to);
    let mut conds: Vec<String> = Vec::new();
    if let Some(s) = params.service.as_deref().filter(|s| !s.is_empty()) {
        conds.push(format!("service = '{}'", sql_escape(s)));
    }
    if let Some(t) = params.r#type.as_deref().filter(|t| !t.is_empty()) {
        conds.push(format!("profile_type = '{}'", sql_escape(t)));
    }
    let max_merge = params.max_merge.unwrap_or(DEFAULT_MAX_MERGE).max(1);
    let want_label = params.label.as_deref().and_then(parse_label);

    let stream = profiles_stream(ctx.scope);
    let base_keys = fetch_object_keys(
        &state,
        &ctx.org_id,
        stream,
        &conds,
        baseline_range,
        &want_label,
    )
    .await?;
    let comp_keys = fetch_object_keys(
        &state,
        &ctx.org_id,
        stream,
        &conds,
        comparison_range,
        &want_label,
    )
    .await?;
    let (base_sampled, base_trunc) = profiles_merge::even_sample(base_keys, max_merge);
    let (comp_sampled, comp_trunc) = profiles_merge::even_sample(comp_keys, max_merge);

    let mut baseline = Vec::with_capacity(base_sampled.len());
    for key in &base_sampled {
        if let Some(p) = load_profile(&state, key).await {
            baseline.push(p);
        }
    }
    let mut comparison = Vec::with_capacity(comp_sampled.len());
    for key in &comp_sampled {
        if let Some(p) = load_profile(&state, key).await {
            comparison.push(p);
        }
    }

    let flamebearer = profiles_merge::build_diff(&baseline, &comparison);
    Ok(Json(DiffResponse {
        flamebearer,
        truncated: base_trunc || comp_trunc,
        baseline_count: baseline.len(),
        comparison_count: comparison.len(),
    })
    .into_response())
}

/// `GET /profiles/{id}`：原始 pprof 下载（task 4.5）。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn download(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let stream = profiles_stream(ctx.scope);
    let statement = format!(
        "SELECT object_key FROM \"{stream}\" WHERE id = '{}' LIMIT 1",
        sql_escape(&id)
    );
    let range = TimeRange::new(TimestampMicros(0), TimestampMicros::now());
    let res = run_query(
        &state,
        &ctx.org_id,
        stream,
        &["id", "object_key"],
        statement,
        range,
        Some(1),
    )
    .await?;
    let key = res
        .rows
        .first()
        .and_then(|row| str_cell(row, col(&res, "object_key")))
        .ok_or_else(|| Error::not_found("profile not found"))?;

    let raw = profiles::get_archive(&state.storage.object_store, &key).await?;
    let gz = profiles::gzip_pprof(&raw)?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], gz).into_response())
}

// ===== 聚合内部 helper =====

/// 拉取窗口内匹配的 `object_key` 集合（service/type/trace 条件已在 SQL，label 在内存过滤）。
async fn fetch_object_keys(
    state: &AppState,
    org_id: &Id,
    stream: &str,
    conds: &[String],
    range: TimeRange,
    want_label: &Option<(String, String)>,
) -> Result<Vec<String>> {
    let where_clause = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };
    let statement =
        format!("SELECT object_key, labels FROM \"{stream}\"{where_clause} ORDER BY {TS_COL} DESC");
    let res = run_query(
        state,
        org_id,
        stream,
        &["object_key", "labels"],
        statement,
        range,
        Some(SCAN_CEILING),
    )
    .await?;
    let key_i = col(&res, "object_key");
    let lbl_i = col(&res, "labels");
    let mut keys = Vec::with_capacity(res.rows.len());
    for row in &res.rows {
        if !row_matches_label(cell(row, lbl_i), want_label) {
            continue;
        }
        if let Some(key) = str_cell(row, key_i).filter(|s| !s.is_empty()) {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// 读回归档 blob 并解码归一化（合并用）；失败的单个 blob 跳过而非整体失败。
async fn load_profile(state: &AppState, key: &str) -> Option<NormalizedProfile> {
    let raw = profiles::get_archive(&state.storage.object_store, key)
        .await
        .ok()?;
    let profile = profiles::decode_pprof(&raw).ok()?;
    Some(profiles::normalize_pprof(&profile, "", &BTreeMap::new()))
}

async fn build_flamegraph_response(
    state: &AppState,
    keys: Vec<String>,
    max_merge: usize,
) -> FlamegraphResponse {
    let (sampled, truncated) = profiles_merge::even_sample(keys, max_merge);
    let mut merged = Vec::with_capacity(sampled.len());
    for key in &sampled {
        if let Some(profile) = load_profile(state, key).await {
            merged.push(profile);
        }
    }
    FlamegraphResponse {
        flamebearer: profiles_merge::build_flamebearer(&merged),
        truncated,
        profile_count: merged.len(),
    }
}

#[cfg(test)]
mod otlp_tests {
    use opentelemetry_proto::tonic::{
        common::v1::{AnyValue, KeyValue, any_value},
        profiles::v1development::{
            Function, Line, Link, Location, Profile as OtlpProfile, ProfilesDictionary,
            ResourceProfiles, Sample as OtlpSample, ScopeProfiles, Stack,
            ValueType as OtlpValueType,
        },
        resource::v1::Resource,
    };

    use super::*;
    use crate::domain::stream::{FieldDef, FieldType};

    #[test]
    fn system_scope_reads_the_protected_self_telemetry_profile_stream() {
        assert_eq!(
            profiles_stream(IamScope::Organization),
            DEFAULT_PROFILE_STREAM
        );
        assert_eq!(profiles_stream(IamScope::ApiToken), DEFAULT_PROFILE_STREAM);
        assert_eq!(profiles_stream(IamScope::System), MOLESIGNAL_SYSTEM_STREAM);
    }

    #[test]
    fn profile_queries_treat_an_uninitialized_schema_as_empty() {
        let empty = Schema { fields: Vec::new() };
        assert!(!schema_contains_fields(&empty, &["id", "service"]));

        let initialized = Schema {
            fields: ["id", "service"]
                .into_iter()
                .map(|name| FieldDef {
                    name: name.to_string(),
                    data_type: FieldType::Utf8,
                    nullable: true,
                    indexed: false,
                    encrypted: false,
                    exact: false,
                })
                .collect(),
        };
        assert!(schema_contains_fields(&initialized, &["id", "service"]));
        assert!(!schema_contains_fields(&initialized, &["id", "labels"]));
    }

    fn str_kv(key: &str, val: &str) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(val.into())),
            }),
            // profiles feature 给 common KeyValue 加了 string-table 引用；用明文 key 时填 0。
            ..Default::default()
        }
    }

    /// 一个最小但完整的 OTLP profiles 请求：共享 dictionary（含一条已符号化栈 +
    /// 一个 Link），单 profile，`values` / `timestamps` 由参数控制以测形状折算。
    fn sample_request(values: Vec<i64>, timestamps: Vec<u64>) -> ExportProfilesServiceRequest {
        let dict = ProfilesDictionary {
            // [0]=null 占位 + main()@app.rs:10
            string_table: vec![
                String::new(),
                "main".into(),
                "cpu".into(),
                "nanoseconds".into(),
                "app.rs".into(),
            ],
            function_table: vec![
                Function::default(),
                Function {
                    name_strindex: 1,
                    filename_strindex: 4,
                    ..Default::default()
                },
            ],
            location_table: vec![
                Location::default(),
                Location {
                    lines: vec![Line {
                        function_index: 1,
                        line: 10,
                        column: 0,
                    }],
                    ..Default::default()
                },
            ],
            stack_table: vec![
                Stack::default(),
                Stack {
                    location_indices: vec![1],
                },
            ],
            link_table: vec![
                Link::default(),
                Link {
                    trace_id: vec![0xaa; 16],
                    span_id: vec![0xbb; 8],
                },
            ],
            ..Default::default()
        };
        let profile = OtlpProfile {
            sample_type: Some(OtlpValueType {
                type_strindex: 2,
                unit_strindex: 3,
            }),
            samples: vec![OtlpSample {
                stack_index: 1,
                link_index: 1,
                values,
                timestamps_unix_nano: timestamps,
                ..Default::default()
            }],
            time_unix_nano: 5_000,
            duration_nano: 1_000,
            ..Default::default()
        };
        ExportProfilesServiceRequest {
            resource_profiles: vec![ResourceProfiles {
                resource: Some(Resource {
                    attributes: vec![str_kv("service.name", "checkout")],
                    ..Default::default()
                }),
                scope_profiles: vec![ScopeProfiles {
                    profiles: vec![profile],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            dictionary: Some(dict),
        }
    }

    #[test]
    fn converts_symbolized_stack_resource_and_link() {
        let out = normalize_otlp_profiles(&sample_request(vec![42], vec![]));
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.service, "checkout");
        assert_eq!(p.profile_type, ProfileType::Cpu);
        assert_eq!(p.sample_types, vec![ValueType::new("cpu", "nanoseconds")]);
        assert_eq!(p.start_time_micros, 5, "5000ns -> 5us");
        assert_eq!(p.samples.len(), 1);
        let s = &p.samples[0];
        assert_eq!(s.values, vec![42]);
        assert_eq!(s.stack.len(), 1);
        assert_eq!(s.stack[0].function, "main");
        assert_eq!(s.stack[0].file.as_deref(), Some("app.rs"));
        assert_eq!(s.stack[0].line, Some(10));
        // trace 关联来自结构化 Link（hex）。
        assert_eq!(p.trace_id, Some("aa".repeat(16)));
        assert_eq!(p.span_id, Some("bb".repeat(8)));
    }

    #[test]
    fn folds_value_timestamp_shapes() {
        // 多值 → 求和。
        let multi = normalize_otlp_profiles(&sample_request(vec![3, 4], vec![]));
        assert_eq!(multi[0].samples[0].values, vec![7]);
        // 空 values + N 时间戳 → 观测计数。
        let ts = normalize_otlp_profiles(&sample_request(vec![], vec![1, 2, 3]));
        assert_eq!(ts[0].samples[0].values, vec![3]);
        // 空 values + 空时间戳 → 至少 1。
        let empty = normalize_otlp_profiles(&sample_request(vec![], vec![]));
        assert_eq!(empty[0].samples[0].values, vec![1]);
    }

    #[test]
    fn unsymbolized_frame_when_location_has_no_lines() {
        let mut req = sample_request(vec![1], vec![]);
        // 把 location[1] 改成无 lines（仅地址）。
        if let Some(dict) = req.dictionary.as_mut() {
            dict.location_table[1] = Location {
                address: 0x1234,
                ..Default::default()
            };
        }
        let out = normalize_otlp_profiles(&req);
        let frame = &out[0].samples[0].stack[0];
        assert_eq!(frame.function, "");
        assert_eq!(frame.address, Some(0x1234));
        assert!(out[0].unsymbolized());
    }

    #[test]
    fn missing_dictionary_yields_empty() {
        let mut req = sample_request(vec![1], vec![]);
        req.dictionary = None;
        assert!(normalize_otlp_profiles(&req).is_empty());
    }
}
