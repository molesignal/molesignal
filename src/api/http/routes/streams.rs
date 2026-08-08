// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, put},
};
use futures::{StreamExt, future::try_join_all, stream};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::{
        iam::{permission, resource_permission},
        storage::{ParquetFileMeta, logical_query_datasets},
        stream::{
            FieldDef, FieldIndexRule, FieldType, Retention, Schema, StreamDefinition,
            StreamIndexType, StreamSettings, StreamType, is_reserved_system_stream,
        },
    },
    intelligence::telemetry::INTELLIGENCE_STREAM,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod field_settings;

use field_settings::{validate_field_masking, validate_system_settings_update};

const DEFAULT_RUNTIME_WINDOW_SECS: i64 = 24 * 60 * 60;
const MIN_RUNTIME_WINDOW_SECS: i64 = 60 * 60;
const MAX_RUNTIME_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;
const DEFAULT_RUNTIME_BUCKET_COUNT: usize = 24;
const MIN_RUNTIME_BUCKET_COUNT: usize = 6;
const MAX_RUNTIME_BUCKET_COUNT: usize = 48;
const HEALTHY_LAG_MICROS: i64 = 15 * 60 * 1_000_000;
const INTERRUPTED_LAG_MICROS: i64 = 2 * 60 * 60 * 1_000_000;
const PARQUET_FILE_META_CONCURRENCY: usize = 8;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/streams", get(list).post(create))
        .route("/streams/runtime", get(runtime))
        .route("/streams/{id}", get(get_one).delete(delete))
        .route("/streams/{id}/settings", put(update_settings))
}

#[async_trait::async_trait]
impl ProtectedResource for StreamDefinition {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.telemetry.streams.get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "stream"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeParams {
    window_secs: Option<i64>,
    bucket_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeStatus {
    Healthy,
    Idle,
    Delayed,
    Interrupted,
    Unused,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
struct RuntimeBucket {
    start_micros: i64,
    end_micros: i64,
    rows: u64,
    stored_bytes: u64,
}

#[derive(Debug, Serialize)]
struct StreamRuntime {
    id: String,
    name: String,
    stream_type: StreamType,
    status: RuntimeStatus,
    rows: u64,
    stored_bytes: u64,
    current_stored_bytes: u64,
    first_received_at_micros: Option<i64>,
    last_received_at_micros: Option<i64>,
    stats_available: bool,
    buckets: Vec<RuntimeBucket>,
}

#[derive(Debug, Serialize)]
struct StreamRuntimeResponse {
    generated_at_micros: i64,
    window_start_micros: i64,
    window_end_micros: i64,
    window_secs: i64,
    streams: Vec<StreamRuntime>,
}

#[derive(Debug, Serialize)]
struct StreamResponse {
    id: String,
    org_id: String,
    name: String,
    stream_type: StreamType,
    schema: Schema,
    retention: Option<Retention>,
    effective_retention: Retention,
    settings: StreamSettings,
    created_at_micros: i64,
    updated_at_micros: i64,
}

impl StreamResponse {
    fn new(def: StreamDefinition, settings: StreamSettings, global_retention_days: u32) -> Self {
        let effective_retention = Retention {
            days: def.effective_retention_days(global_retention_days),
        };
        Self {
            id: def.id.0,
            org_id: def.org_id.0,
            name: def.name,
            stream_type: def.stream_type,
            schema: def.schema,
            retention: def.retention,
            effective_retention,
            settings,
            created_at_micros: def.created_at.0,
            updated_at_micros: def.updated_at.0,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateStreamRequest {
    name: String,
    stream_type: StreamType,
    #[serde(default)]
    fields: Vec<CreateFieldRequest>,
    #[serde(default)]
    retention_days: Option<u32>,
    #[serde(default)]
    settings: Option<StreamSettings>,
}

#[derive(Debug, Deserialize)]
struct CreateFieldRequest {
    name: String,
    data_type: FieldType,
    #[serde(default = "default_true")]
    nullable: bool,
    #[serde(default)]
    indexed: bool,
    /// 索引类型：`Exact` → 未分词 STRING 索引供 `col = 'x'` 等值裁剪；其余（含缺省）
    /// → 分词 TEXT 索引供 `MATCH()` 全文。仅在 `indexed = true` 时有意义。缺省不破坏
    /// 老请求（只传 `indexed: true` 仍是 TEXT）。
    #[serde(default)]
    index_type: StreamIndexType,
    /// 字段级静态加密：写入前用 cipher root key 加密、密文落盘；查询用 `decrypt(col)` 还原。
    #[serde(default)]
    encrypted: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateStreamSettingsRequest {
    #[serde(default)]
    fields: Option<Vec<FieldSettingRequest>>,
    #[serde(default)]
    retention_days: Option<RetentionDaysUpdate>,
    #[serde(default)]
    settings: Option<StreamSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RetentionDaysUpdate {
    Set(u32),
    Clear(()),
}

#[derive(Debug, Deserialize)]
struct FieldSettingRequest {
    name: String,
    #[serde(default)]
    indexed: bool,
    #[serde(default)]
    index_type: StreamIndexType,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    sdr_patterns: Vec<String>,
}

fn default_true() -> bool {
    true
}

async fn response_for(state: &AppState, def: StreamDefinition) -> Result<StreamResponse> {
    let settings = state.telemetry.streams.get_settings(&def.id).await?;
    Ok(StreamResponse::new(
        def,
        settings,
        state.telemetry.stream_retention_days,
    ))
}

fn validate_public_stream_name(name: &str) -> Result<()> {
    crate::domain::stream::validate_stream_name(name)?;
    if is_reserved_system_stream(name) {
        return Err(Error::forbidden(
            "`_molesignal` is a protected system stream",
        ));
    }
    Ok(())
}

fn ensure_stream_mutable(def: &StreamDefinition) -> Result<()> {
    if is_reserved_system_stream(&def.name) {
        return Err(Error::forbidden(
            "`_molesignal` is a protected system stream",
        ));
    }
    Ok(())
}

fn validate_days(days: u32) -> Result<()> {
    if days == 0 || days > 3650 {
        return Err(Error::invalid("retention_days must be between 1 and 3650"));
    }
    Ok(())
}

/// full_text 索引类型仅限 string（utf8）字段（spec stream-index-config）。create 与
/// update_settings 对 `index_type == FullText && data_type != Utf8` 的新配置返回 400；
/// 存量 json full_text 配置不受影响（写侧 builder 保持 `Utf8 | Json`，仅拦新提交）。
fn validate_full_text_data_type(
    field_name: &str,
    data_type: FieldType,
    index_type: StreamIndexType,
) -> Result<()> {
    if index_type == StreamIndexType::FullText && data_type != FieldType::Utf8 {
        return Err(Error::invalid(format!(
            "full_text index type is only supported on string (utf8) fields; \
             field `{field_name}` has type `{data_type:?}`"
        )));
    }
    Ok(())
}

fn validate_settings(settings: &StreamSettings) -> Result<()> {
    for condition in &settings.keep_conditions {
        if let Some(days) = condition.retention_days {
            validate_days(days)?;
        }
        if condition.name.trim().is_empty() {
            return Err(Error::invalid("retention condition name cannot be empty"));
        }
        if condition.expression.trim().is_empty() {
            return Err(Error::invalid(
                "retention condition expression cannot be empty",
            ));
        }
    }
    Ok(())
}

#[permission(any("streams.read", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<StreamResponse>>> {
    let streams = state.telemetry.streams.list(&ctx.org_id).await?;
    let mut out = Vec::with_capacity(streams.len());
    for def in streams {
        out.push(response_for(&state, def).await?);
    }
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(format!("{:?}", a.stream_type).cmp(&format!("{:?}", b.stream_type)))
    });
    Ok(Json(out))
}

#[permission(any("streams.read", "sys.telemetry.read"))]
async fn runtime(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<RuntimeParams>,
) -> Result<Json<StreamRuntimeResponse>> {
    let window_secs = params
        .window_secs
        .unwrap_or(DEFAULT_RUNTIME_WINDOW_SECS)
        .clamp(MIN_RUNTIME_WINDOW_SECS, MAX_RUNTIME_WINDOW_SECS);
    let bucket_count = params
        .bucket_count
        .unwrap_or(DEFAULT_RUNTIME_BUCKET_COUNT)
        .clamp(MIN_RUNTIME_BUCKET_COUNT, MAX_RUNTIME_BUCKET_COUNT);
    let generated_at = TimestampMicros::now();
    let window = TimeRange::new(
        TimestampMicros(
            generated_at
                .0
                .saturating_sub(window_secs.saturating_mul(1_000_000)),
        ),
        generated_at,
    );

    let definitions = state
        .telemetry
        .streams
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|definition| definition.stream_type != StreamType::Extend)
        .collect::<Vec<_>>();
    let parquet_file_meta = state.storage.parquet_file_meta.clone();
    let org_id = ctx.org_id.clone();
    let mut streams = stream::iter(definitions)
        .map(|definition| {
            let parquet_file_meta = parquet_file_meta.clone();
            let org_id = org_id.clone();
            async move {
                let lifetime_start = definition.created_at.0.min(window.start.0);
                let lifetime = TimeRange::new(TimestampMicros(lifetime_start), generated_at);
                let lookups =
                    logical_query_datasets(definition.stream_type)
                        .iter()
                        .map(|dataset_kind| {
                            parquet_file_meta.find_dataset(
                                &org_id,
                                &definition.name,
                                definition.stream_type,
                                *dataset_kind,
                                lifetime,
                            )
                        });
                match try_join_all(lookups)
                    .await
                    .map(|groups| groups.into_iter().flatten().collect::<Vec<_>>())
                {
                    Ok(files) => {
                        summarize_runtime(definition, &files, window, generated_at.0, bucket_count)
                    }
                    Err(error) => {
                        tracing::warn!(
                            org_id = %org_id.0,
                            stream = %definition.name,
                            stream_type = ?definition.stream_type,
                            error = %error,
                            "stream runtime parquet-file-meta scan failed"
                        );
                        unavailable_runtime(definition, window, bucket_count)
                    }
                }
            }
        })
        .buffer_unordered(PARQUET_FILE_META_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    streams.sort_by(|a, b| {
        runtime_status_rank(a.status)
            .cmp(&runtime_status_rank(b.status))
            .then(b.rows.cmp(&a.rows))
            .then(a.name.cmp(&b.name))
    });

    Ok(Json(StreamRuntimeResponse {
        generated_at_micros: generated_at.0,
        window_start_micros: window.start.0,
        window_end_micros: window.end.0,
        window_secs,
        streams,
    }))
}

#[resource_permission(
    action = any("streams.read", "sys.telemetry.read"),
    resource = StreamDefinition,
    id = Id::from_string(id),
    bind = stream
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<StreamResponse>> {
    Ok(Json(response_for(&state, stream).await?))
}

#[permission("streams.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateStreamRequest>,
) -> Result<Json<StreamResponse>> {
    let name = req.name.trim().to_string();
    validate_public_stream_name(&name)?;
    match state
        .telemetry
        .streams
        .get(&ctx.org_id, &name, req.stream_type)
        .await
    {
        Ok(_) => {
            return Err(Error::conflict(format!(
                "stream `{name}` with type `{}` already exists",
                req.stream_type.as_str()
            )));
        }
        Err(Error::NotFound(_)) => {}
        Err(error) => return Err(error),
    }
    let settings = req.settings.unwrap_or_default();
    validate_settings(&settings)?;
    let retention = match req.retention_days {
        Some(days) => {
            validate_days(days)?;
            Some(Retention { days })
        }
        None => None,
    };
    let now = TimestampMicros::now();
    let def = StreamDefinition {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name,
        stream_type: req.stream_type,
        schema: Schema {
            fields: req
                .fields
                .into_iter()
                .map(|field| {
                    let index_type = field.index_type;
                    validate_full_text_data_type(&field.name, field.data_type, index_type.clone())?;
                    Ok(FieldDef {
                        name: field.name,
                        // exact 索引对高基数字段（trace_id 等）有意义，隐含 indexed。
                        exact: field.indexed && index_type == StreamIndexType::Exact,
                        data_type: field.data_type,
                        nullable: field.nullable,
                        indexed: field.indexed,
                        encrypted: field.encrypted,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        },
        retention,
        created_at: now,
        updated_at: now,
    };
    validate_field_masking(&settings, &def.schema, def.stream_type)?;
    let created = state.telemetry.streams.create(def).await?;
    let settings = state
        .telemetry
        .streams
        .update_settings(&created.id, settings)
        .await?;
    Ok(Json(StreamResponse::new(
        created,
        settings,
        state.telemetry.stream_retention_days,
    )))
}

#[resource_permission(
    action = any("streams.configure", "sys.telemetry.manage"),
    resource = StreamDefinition,
    id = Id::from_string(id),
    bind = stream
)]
async fn update_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateStreamSettingsRequest>,
) -> Result<Json<StreamResponse>> {
    let id = stream.id.clone();
    let mut def = stream;
    let system_stream = is_reserved_system_stream(&def.name);
    let current_settings = state.telemetry.streams.get_settings(&id).await?;

    if system_stream && req.retention_days.is_some() {
        return Err(Error::forbidden(
            "system stream retention is managed by self telemetry settings",
        ));
    }

    if let Some(retention_days) = req.retention_days {
        let retention = match retention_days {
            RetentionDaysUpdate::Set(days) => {
                validate_days(days)?;
                Some(Retention { days })
            }
            RetentionDaysUpdate::Clear(()) => None,
        };
        state
            .telemetry
            .streams
            .update_retention(&id, retention)
            .await?;
        def.retention = retention;
    }

    let mut settings = req.settings.unwrap_or_else(|| current_settings.clone());
    validate_settings(&settings)?;

    let schema_changed = req.fields.is_some();
    if let Some(field_settings) = req.fields {
        for setting in &field_settings {
            if let Some(field) = def
                .schema
                .fields
                .iter_mut()
                .find(|f| f.name == setting.name)
            {
                validate_full_text_data_type(
                    &field.name,
                    field.data_type,
                    setting.index_type.clone(),
                )?;
                field.indexed = setting.indexed && setting.index_type != StreamIndexType::None;
                // Exact → 未分词 STRING 索引（等值裁剪）；其余索引类型走分词 TEXT（全文）。
                field.exact = field.indexed && setting.index_type == StreamIndexType::Exact;
            }
        }
        settings.index_rules = field_settings
            .into_iter()
            .map(|setting| FieldIndexRule {
                field: setting.name,
                enabled: setting.indexed && setting.index_type != StreamIndexType::None,
                index_type: setting.index_type,
                condition: setting.condition.filter(|value| !value.trim().is_empty()),
                sdr_patterns: setting
                    .sdr_patterns
                    .into_iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect(),
            })
            .collect();
    }

    validate_field_masking(&settings, &def.schema, def.stream_type)?;
    if system_stream {
        validate_system_settings_update(&current_settings, &settings)?;
    }
    if schema_changed {
        state
            .telemetry
            .streams
            .update_schema(&id, def.schema.clone())
            .await?;
    }

    let _ = state
        .telemetry
        .streams
        .update_settings(&id, settings)
        .await?;
    let updated = state.telemetry.streams.get_by_id(&id).await?;
    Ok(Json(response_for(&state, updated).await?))
}

#[resource_permission(
    action = "streams.delete",
    resource = StreamDefinition,
    id = Id::from_string(id),
    bind = stream
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let id = stream.id.clone();
    ensure_stream_mutable(&stream)?;
    state.telemetry.streams.delete(&id).await?;
    Ok("deleted")
}

fn summarize_runtime(
    definition: StreamDefinition,
    files: &[ParquetFileMeta],
    window: TimeRange,
    generated_at_micros: i64,
    bucket_count: usize,
) -> StreamRuntime {
    let mut rows = 0_u64;
    let mut stored_bytes = 0_u64;
    let mut current_stored_bytes = 0_u64;
    let mut first_received_at_micros: Option<i64> = None;
    let mut last_received_at_micros: Option<i64> = None;
    let mut buckets = runtime_buckets(window, bucket_count);

    for file in files.iter().filter(|file| !file.deleted) {
        current_stored_bytes = current_stored_bytes.saturating_add(file.size_bytes);
        first_received_at_micros = Some(
            first_received_at_micros.map_or(file.time_range.start.0, |current| {
                current.min(file.time_range.start.0)
            }),
        );
        last_received_at_micros = Some(
            last_received_at_micros.map_or(file.time_range.end.0, |current| {
                current.max(file.time_range.end.0)
            }),
        );

        let Some((file_rows, file_bytes, overlap_start, overlap_end)) =
            runtime_file_contribution(file, window)
        else {
            continue;
        };
        rows = rows.saturating_add(file_rows);
        stored_bytes = stored_bytes.saturating_add(file_bytes);
        let midpoint = overlap_start.saturating_add(overlap_end.saturating_sub(overlap_start) / 2);
        let index = runtime_bucket_index(window, bucket_count, midpoint);
        buckets[index].rows = buckets[index].rows.saturating_add(file_rows);
        buckets[index].stored_bytes = buckets[index].stored_bytes.saturating_add(file_bytes);
    }

    let status = runtime_status(
        &definition.name,
        last_received_at_micros,
        generated_at_micros,
    );

    StreamRuntime {
        id: definition.id.0,
        name: definition.name,
        stream_type: definition.stream_type,
        status,
        rows,
        stored_bytes,
        current_stored_bytes,
        first_received_at_micros,
        last_received_at_micros,
        stats_available: true,
        buckets,
    }
}

fn unavailable_runtime(
    definition: StreamDefinition,
    window: TimeRange,
    bucket_count: usize,
) -> StreamRuntime {
    StreamRuntime {
        id: definition.id.0,
        name: definition.name,
        stream_type: definition.stream_type,
        status: RuntimeStatus::Unknown,
        rows: 0,
        stored_bytes: 0,
        current_stored_bytes: 0,
        first_received_at_micros: None,
        last_received_at_micros: None,
        stats_available: false,
        buckets: runtime_buckets(window, bucket_count),
    }
}

fn runtime_status(
    stream_name: &str,
    last_received_at_micros: Option<i64>,
    generated_at_micros: i64,
) -> RuntimeStatus {
    match last_received_at_micros {
        Some(last) if last >= generated_at_micros.saturating_sub(HEALTHY_LAG_MICROS) => {
            RuntimeStatus::Healthy
        }
        // Intelligence traces are emitted once per completed agent response. A quiet
        // period is expected and must not be reported as a broken continuous feed.
        Some(_) if stream_name == INTELLIGENCE_STREAM => RuntimeStatus::Idle,
        Some(last) if last >= generated_at_micros.saturating_sub(INTERRUPTED_LAG_MICROS) => {
            RuntimeStatus::Delayed
        }
        Some(_) => RuntimeStatus::Interrupted,
        None => RuntimeStatus::Unused,
    }
}

fn runtime_file_contribution(
    file: &ParquetFileMeta,
    window: TimeRange,
) -> Option<(u64, u64, i64, i64)> {
    if !file.time_range.overlaps(window) {
        return None;
    }
    let overlap_start = file.time_range.start.0.max(window.start.0);
    let overlap_end = file.time_range.end.0.min(window.end.0);
    let file_duration = file
        .time_range
        .end
        .0
        .saturating_sub(file.time_range.start.0);
    if file_duration <= 0 {
        return Some((file.rows, file.size_bytes, overlap_start, overlap_end));
    }
    let overlap_duration = overlap_end.saturating_sub(overlap_start);
    if overlap_duration <= 0 {
        return None;
    }
    let ratio = (overlap_duration as f64 / file_duration as f64).clamp(0.0, 1.0);
    Some((
        runtime_prorate(file.rows, ratio),
        runtime_prorate(file.size_bytes, ratio),
        overlap_start,
        overlap_end,
    ))
}

fn runtime_prorate(value: u64, ratio: f64) -> u64 {
    if value == 0 || ratio <= 0.0 {
        return 0;
    }
    if ratio >= 1.0 {
        return value;
    }
    ((value as f64 * ratio).round() as u64).max(1).min(value)
}

fn runtime_buckets(window: TimeRange, bucket_count: usize) -> Vec<RuntimeBucket> {
    let duration = window.duration_micros().max(1);
    (0..bucket_count)
        .map(|index| {
            let start_micros = window
                .start
                .0
                .saturating_add(((duration as i128 * index as i128) / bucket_count as i128) as i64);
            let end_micros = if index + 1 == bucket_count {
                window.end.0
            } else {
                window.start.0.saturating_add(
                    ((duration as i128 * (index + 1) as i128) / bucket_count as i128) as i64,
                )
            };
            RuntimeBucket {
                start_micros,
                end_micros,
                rows: 0,
                stored_bytes: 0,
            }
        })
        .collect()
}

fn runtime_bucket_index(window: TimeRange, bucket_count: usize, timestamp_micros: i64) -> usize {
    let duration = window.duration_micros().max(1);
    let offset = timestamp_micros
        .saturating_sub(window.start.0)
        .clamp(0, duration.saturating_sub(1));
    (((offset as i128 * bucket_count as i128) / duration as i128) as usize)
        .min(bucket_count.saturating_sub(1))
}

fn runtime_status_rank(status: RuntimeStatus) -> u8 {
    match status {
        RuntimeStatus::Unknown => 0,
        RuntimeStatus::Interrupted => 1,
        RuntimeStatus::Delayed => 2,
        RuntimeStatus::Healthy => 3,
        RuntimeStatus::Idle => 4,
        RuntimeStatus::Unused => 5,
    }
}

#[cfg(test)]
mod runtime_tests {
    use serde_json::Map;

    use super::*;

    fn definition() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("stream-1"),
            org_id: Id::from_string("org-1"),
            name: "app_logs".into(),
            stream_type: StreamType::Logs,
            schema: Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn file(start: i64, end: i64, rows: u64, bytes: u64) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::new(),
            org_id: Id::from_string("org-1"),
            stream: "app_logs".into(),
            stream_type: StreamType::Logs,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: "test.parquet".into(),
            time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
            rows,
            size_bytes: bytes,
            min_values: Map::new(),
            max_values: Map::new(),
            deleted: false,
        }
    }

    #[test]
    fn runtime_separates_window_storage_from_current_storage() {
        let window = TimeRange::new(TimestampMicros(100), TimestampMicros(200));
        let files = vec![file(0, 50, 10, 100), file(150, 250, 20, 200)];
        let summary = summarize_runtime(definition(), &files, window, 200, 4);

        assert_eq!(summary.rows, 10);
        assert_eq!(summary.stored_bytes, 100);
        assert_eq!(summary.current_stored_bytes, 300);
        assert_eq!(summary.first_received_at_micros, Some(0));
        assert_eq!(summary.last_received_at_micros, Some(250));
        assert_eq!(
            summary
                .buckets
                .iter()
                .map(|bucket| bucket.rows)
                .sum::<u64>(),
            10
        );
    }

    #[test]
    fn runtime_status_distinguishes_delayed_interrupted_and_unused() {
        let now = 10 * INTERRUPTED_LAG_MICROS;
        assert_eq!(
            runtime_status("app_logs", Some(now), now),
            RuntimeStatus::Healthy
        );
        assert_eq!(
            runtime_status("app_logs", Some(now - HEALTHY_LAG_MICROS - 1), now),
            RuntimeStatus::Delayed
        );
        assert_eq!(
            runtime_status("app_logs", Some(now - INTERRUPTED_LAG_MICROS - 1), now),
            RuntimeStatus::Interrupted
        );
        assert_eq!(runtime_status("app_logs", None, now), RuntimeStatus::Unused);
    }

    #[test]
    fn event_driven_intelligence_stream_becomes_idle_instead_of_interrupted() {
        let now = 10 * INTERRUPTED_LAG_MICROS;
        assert_eq!(
            runtime_status(INTELLIGENCE_STREAM, Some(now - HEALTHY_LAG_MICROS - 1), now),
            RuntimeStatus::Idle
        );
        assert_eq!(
            runtime_status(INTELLIGENCE_STREAM, None, now),
            RuntimeStatus::Unused
        );
    }

    #[test]
    fn system_stream_cannot_be_created_or_mutated_through_stream_crud() {
        assert!(matches!(
            validate_public_stream_name("_molesignal"),
            Err(Error::Forbidden(_))
        ));

        let mut system = definition();
        system.name = "_molesignal".into();
        assert!(matches!(
            ensure_stream_mutable(&system),
            Err(Error::Forbidden(_))
        ));
        assert!(validate_public_stream_name("_custom").is_ok());
    }
}

#[cfg(test)]
mod full_text_type_tests {
    use super::*;

    /// full_text 索引类型仅限 string 字段（spec stream-index-config）。
    /// create 与 update_settings 两条路径共用 `validate_full_text_data_type`，这里
    /// 覆盖该唯一校验入口的全部分支。
    #[test]
    fn full_text_rejected_on_non_string_fields() {
        // create 路径：提交 {name: "count", data_type: "int64", index_type: "full_text"}。
        let err =
            validate_full_text_data_type("count", FieldType::Int64, StreamIndexType::FullText)
                .expect_err("int64 + full_text must be rejected");
        assert!(matches!(err, Error::InvalidArgument(_)));
        assert!(err.to_string().contains("full_text"));

        // update_settings 路径：json 字段提交 full_text → 400（新建配置被拒）。
        let err =
            validate_full_text_data_type("payload", FieldType::Json, StreamIndexType::FullText)
                .expect_err("json + full_text must be rejected");
        assert!(matches!(err, Error::InvalidArgument(_)));

        // bool / float / timestamp 同属非 string。
        for data_type in [FieldType::Bool, FieldType::Float64, FieldType::Timestamp] {
            assert!(
                validate_full_text_data_type("f", data_type, StreamIndexType::FullText).is_err(),
                "{data_type:?} + full_text 必须被拒"
            );
        }
    }

    #[test]
    fn full_text_allowed_on_utf8_field() {
        assert!(
            validate_full_text_data_type("message", FieldType::Utf8, StreamIndexType::FullText)
                .is_ok(),
            "utf8 + full_text 应放行"
        );
    }

    #[test]
    fn non_full_text_index_types_are_not_affected() {
        // 非 string 字段的其他索引类型不受影响（spec：none / exact / bloom / skip）。
        for index_type in [
            StreamIndexType::None,
            StreamIndexType::Exact,
            StreamIndexType::Bloom,
            StreamIndexType::Skip,
        ] {
            assert!(
                validate_full_text_data_type("count", FieldType::Int64, index_type.clone()).is_ok(),
                "int64 + {index_type:?} 应放行"
            );
        }
        assert!(
            validate_full_text_data_type("trace_id", FieldType::Utf8, StreamIndexType::Exact)
                .is_ok(),
            "utf8 + exact 应放行"
        );
    }
}
