// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::{
        iam::{permission, resource_permission},
        stream::{
            FieldDef, FieldIndexRule, FieldType, Retention, Schema, StreamDefinition,
            StreamIndexType, StreamSettings, StreamType, is_reserved_system_stream,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod field_settings;
mod runtime;

use field_settings::{validate_field_masking, validate_system_settings_update};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/streams", get(list).post(create))
        .route("/streams/runtime", get(runtime::handle))
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
    /// 规范索引类型；仅在 `indexed = true` 时有意义。缺省兼容旧请求：文本字段映射为
    /// `full_text`，非文本字段映射为 `skip`。
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

fn resolve_create_index_type(
    indexed: bool,
    data_type: FieldType,
    requested: StreamIndexType,
) -> StreamIndexType {
    if !indexed {
        StreamIndexType::None
    } else if requested == StreamIndexType::None {
        StreamIndexType::legacy_default(data_type)
    } else {
        requested
    }
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
    let mut indexed_fields = std::collections::HashSet::new();
    for rule in &settings.index_rules {
        if rule.field.trim().is_empty() {
            return Err(Error::invalid("index rule field cannot be empty"));
        }
        if !indexed_fields.insert(rule.field.as_str()) {
            return Err(Error::invalid(format!(
                "duplicate index rule for field `{}`",
                rule.field
            )));
        }
    }
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
                    let rule = settings
                        .index_rules
                        .iter()
                        .find(|rule| rule.field == field.name);
                    let requested = rule.map_or(field.index_type, |rule| rule.index_type);
                    validate_full_text_data_type(&field.name, field.data_type, requested)?;
                    let index_type = match rule {
                        Some(rule) if rule.enabled => rule.index_type,
                        Some(_) => StreamIndexType::None,
                        None => resolve_create_index_type(
                            field.indexed,
                            field.data_type,
                            field.index_type,
                        ),
                    };
                    Ok(FieldDef {
                        name: field.name,
                        data_type: field.data_type,
                        nullable: field.nullable,
                        index_type: Some(index_type),
                        indexed: index_type != StreamIndexType::None,
                        encrypted: field.encrypted,
                        exact: index_type == StreamIndexType::Exact,
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
    let fields_were_provided = req.fields.is_some();
    if let Some(field_settings) = req.fields {
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
    validate_settings(&settings)?;

    let schema_changed =
        fields_were_provided || settings.index_rules != current_settings.index_rules;
    if schema_changed {
        for rule in &settings.index_rules {
            if let Some(field) = def
                .schema
                .fields
                .iter_mut()
                .find(|field| field.name == rule.field)
            {
                validate_full_text_data_type(&field.name, field.data_type, rule.index_type)?;
                field.configure_index(rule.enabled, rule.index_type);
            }
        }
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

#[cfg(test)]
mod stream_crud_tests {
    use super::*;

    fn definition() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("stream-1"),
            org_id: Id::from_string("org-1"),
            name: "app_logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
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
                validate_full_text_data_type("count", FieldType::Int64, index_type).is_ok(),
                "int64 + {index_type:?} 应放行"
            );
        }
        assert!(
            validate_full_text_data_type("trace_id", FieldType::Utf8, StreamIndexType::Exact)
                .is_ok(),
            "utf8 + exact 应放行"
        );
    }

    #[test]
    fn create_request_preserves_explicit_types_and_maps_legacy_indexed_fields() {
        assert_eq!(
            resolve_create_index_type(true, FieldType::Utf8, StreamIndexType::None),
            StreamIndexType::FullText
        );
        assert_eq!(
            resolve_create_index_type(true, FieldType::Int64, StreamIndexType::None),
            StreamIndexType::Skip
        );
        for index_type in [
            StreamIndexType::Exact,
            StreamIndexType::FullText,
            StreamIndexType::Bloom,
            StreamIndexType::Skip,
        ] {
            assert_eq!(
                resolve_create_index_type(true, FieldType::Utf8, index_type),
                index_type
            );
        }
        assert_eq!(
            resolve_create_index_type(false, FieldType::Utf8, StreamIndexType::FullText),
            StreamIndexType::None
        );
    }
}
