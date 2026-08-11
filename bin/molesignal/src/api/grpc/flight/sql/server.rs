// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow Flight SQL server（spec flight-sql）：对外数据库客户端查询协议。
//!
//! 与内部 shard 协议（[`super::super::FlightGrpc`]，可信网络免鉴权）**分端口**部署：
//! 本服务由 [`crate::api::grpc::serve_flight_sql`] 挂在 `flight_sql.bind:port`（默认 5083），
//! 端口可暴露给用户网络 —— 除 Handshake 外每个 RPC 都要求
//! `authorization: Bearer ms_...` 并重新走 [`authenticate_bearer`] + `StreamRead`
//! 权限校验，org 隔离与 HTTP `/api/v1/query` 完全一致。
//!
//! - 语句执行：`CommandStatementQuery` / 无参 prepared statement。SQL 经
//!   `prepare_flight_sql_select` 校验（仅 SELECT）+ 改写（`logs.nginx` → `nginx`），
//!   ticket 内嵌 `{sql, org_id}`（`query.v1.FlightSqlTicket`），`do_get` 时与
//!   bearer 的 org 二次比对防跨 org 重放。执行走 `QueryService::run_tracked`
//!   （进 active query registry，可被 `/api/v1/query/running` 看到 / cancel）。
//! - 元数据：单 catalog `molesignal`，schema = stream_type（logs/metrics/traces/
//!   extend），表 = 当前 org 的 streams。
//! - 结果编码：[`super::encode::query_result_to_batch`]。

use std::{pin::Pin, sync::Arc};

use arrow::{
    datatypes::{DataType, Field, Schema as ArrowSchema},
    ipc::writer::IpcWriteOptions,
};
use arrow_flight::{
    Action, FlightDescriptor, FlightEndpoint, FlightInfo, HandshakeRequest, HandshakeResponse,
    IpcMessage, SchemaAsIpc, Ticket,
    encode::FlightDataEncoderBuilder,
    flight_service_server::{FlightService, FlightServiceServer},
    sql::{
        ActionClosePreparedStatementRequest, ActionCreatePreparedStatementRequest,
        ActionCreatePreparedStatementResult, CommandGetCatalogs, CommandGetCrossReference,
        CommandGetDbSchemas, CommandGetExportedKeys, CommandGetImportedKeys, CommandGetPrimaryKeys,
        CommandGetSqlInfo, CommandGetTableTypes, CommandGetTables, CommandGetXdbcTypeInfo,
        CommandPreparedStatementQuery, CommandStatementQuery, ProstMessageExt, SqlInfo,
        TicketStatementQuery,
        metadata::{SqlInfoData, SqlInfoDataBuilder, XdbcTypeInfoData, XdbcTypeInfoDataBuilder},
        server::FlightSqlService,
    },
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD as BASE64_STANDARD, STANDARD_NO_PAD as BASE64_NO_PAD},
};
use futures::{Stream, TryStreamExt};
use prost::Message;
use tonic::{Request, Response, Status, Streaming};

use crate::{
    api::http::middleware::{
        Permission,
        auth::{authenticate_api_token, authenticate_bearer},
    },
    app::{
        iam::{IamContext, IamContextEnricher, IamService},
        query::QueryService,
    },
    config::FlightSqlSettings,
    domain::{
        iam::{IamMembership, api_token::ApiTokenRepository},
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::{StreamRepository, StreamType},
    },
    infra::{query::parser::prepare_flight_sql_select, storage::arrow_schema::to_arrow},
    protocol::query::v1::FlightSqlTicket,
    shared::{
        Error as MsError,
        time::{TimeRange, TimestampMicros},
        trace_stream::segmented_result_stream,
    },
};

/// `CommandGetSqlInfo` 的静态应答：服务器标识 + 只读声明 + 引号规则。
static SQL_INFO_DATA: std::sync::LazyLock<SqlInfoData> = std::sync::LazyLock::new(|| {
    let mut builder = SqlInfoDataBuilder::new();
    builder.append(SqlInfo::FlightSqlServerName, "MoleSignal");
    builder.append(SqlInfo::FlightSqlServerVersion, env!("CARGO_PKG_VERSION"));
    builder.append(SqlInfo::FlightSqlServerArrowVersion, "58.0.0");
    builder.append(SqlInfo::FlightSqlServerReadOnly, true);
    builder.append(SqlInfo::SqlIdentifierQuoteChar, r#"""#);
    builder.build().expect("static sql info data")
});

/// 元数据 RPC 暴露的单一 catalog 名。
const CATALOG: &str = "molesignal";

/// `CommandGetXdbcTypeInfo` 的静态空数据集（stream 无 XDBC 类型目录）。
static XDBC_TYPE_INFO_DATA: std::sync::LazyLock<XdbcTypeInfoData> =
    std::sync::LazyLock::new(|| {
        XdbcTypeInfoDataBuilder::new()
            .build()
            .expect("empty xdbc type info")
    });

/// `CommandGetPrimaryKeys` 结果 schema（Flight SQL 规范；与 Java
/// `FlightSqlProducer.Schemas.GET_PRIMARY_KEYS_SCHEMA` 对齐）。stream 没有
/// 主键概念，永远返回 0 行 —— 但 RPC 必须实现：DBeaver 打开表时会查约束，
/// Unimplemented 会直接弹错。
static PRIMARY_KEYS_SCHEMA: std::sync::LazyLock<Arc<ArrowSchema>> =
    std::sync::LazyLock::new(|| {
        Arc::new(ArrowSchema::new(vec![
            Field::new("catalog_name", DataType::Utf8, true),
            Field::new("db_schema_name", DataType::Utf8, true),
            Field::new("table_name", DataType::Utf8, false),
            Field::new("column_name", DataType::Utf8, false),
            Field::new("key_name", DataType::Utf8, true),
            Field::new("key_sequence", DataType::Int32, false),
        ]))
    });

/// exported / imported / cross-reference keys 共用的结果 schema（同上，0 行）。
static FOREIGN_KEYS_SCHEMA: std::sync::LazyLock<Arc<ArrowSchema>> =
    std::sync::LazyLock::new(|| {
        Arc::new(ArrowSchema::new(vec![
            Field::new("pk_catalog_name", DataType::Utf8, true),
            Field::new("pk_db_schema_name", DataType::Utf8, true),
            Field::new("pk_table_name", DataType::Utf8, false),
            Field::new("pk_column_name", DataType::Utf8, false),
            Field::new("fk_catalog_name", DataType::Utf8, true),
            Field::new("fk_db_schema_name", DataType::Utf8, true),
            Field::new("fk_table_name", DataType::Utf8, false),
            Field::new("fk_column_name", DataType::Utf8, false),
            Field::new("key_sequence", DataType::Int32, false),
            Field::new("fk_key_name", DataType::Utf8, true),
            Field::new("pk_key_name", DataType::Utf8, true),
            Field::new("update_rule", DataType::UInt8, false),
            Field::new("delete_rule", DataType::UInt8, false),
        ]))
    });

pub struct FlightSqlGrpc {
    query: Arc<QueryService>,
    api_tokens: Arc<dyn ApiTokenRepository>,
    streams: Arc<dyn StreamRepository>,
    iam: Arc<IamService>,
    iam_access: Arc<dyn IamContextEnricher>,
    settings: FlightSqlSettings,
}

type DoGetStream = <FlightSqlGrpc as FlightService>::DoGetStream;

impl FlightSqlGrpc {
    pub fn new(
        query: Arc<QueryService>,
        api_tokens: Arc<dyn ApiTokenRepository>,
        streams: Arc<dyn StreamRepository>,
        iam: Arc<IamService>,
        iam_access: Arc<dyn IamContextEnricher>,
        settings: FlightSqlSettings,
    ) -> Self {
        Self {
            query,
            api_tokens,
            streams,
            iam,
            iam_access,
            settings,
        }
    }

    pub fn into_server(self) -> FlightServiceServer<Self> {
        FlightServiceServer::new(self)
    }

    /// per-RPC 鉴权（spec flight-sql）：bearer 按 HTTP 中间件相同的前缀规则分发
    /// （`ms_` → API token，其余 → JWT），再 `StreamRead`。缺失/非法 →
    /// `UNAUTHENTICATED`，权限不足 → `PERMISSION_DENIED`。
    async fn authenticate<T>(&self, request: &Request<T>) -> Result<IamContext, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| {
                h.strip_prefix("Bearer ")
                    .or_else(|| h.strip_prefix("bearer "))
            })
            .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;
        let mut ctx = authenticate_bearer(bearer, self.iam.as_ref(), self.api_tokens.clone())
            .await
            .map_err(|e| Status::unauthenticated(e.to_string()))?;
        self.iam_access
            .enrich_iam_context(&mut ctx)
            .await
            .map_err(|error| Status::permission_denied(error.to_string()))?;
        Permission::require_any_key(&ctx, &["streams.query", "sys.telemetry.read"])
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        Ok(ctx)
    }

    /// 账号密码路径的 membership 选择：带选择子时匹配 org 的 id / name / slug，
    /// 否则沿用 HTTP login 的"第一个 membership"语义。
    async fn resolve_membership(
        &self,
        user_id: &crate::shared::ids::Id,
        org_selector: Option<&str>,
    ) -> Result<IamMembership, Status> {
        let memberships = self
            .iam
            .iam_memberships
            .list_for_user(user_id)
            .await
            .map_err(error_to_status)?;
        match org_selector {
            None => memberships
                .into_iter()
                .next()
                .ok_or_else(|| Status::permission_denied("user has no membership")),
            Some(sel) => {
                for m in memberships {
                    if m.org_id.0 == sel {
                        return Ok(m);
                    }
                    if let Ok(org) = self.iam.orgs.get(&m.org_id).await
                        && (org.name == sel || org.slug == sel)
                    {
                        return Ok(m);
                    }
                }
                Err(Status::permission_denied(format!(
                    "no membership in org '{sel}'"
                )))
            }
        }
    }

    /// 签发 statement/prepared-statement 共用的 org-bound handle。
    fn statement_handle(&self, ctx: &IamContext, sql: &str) -> Vec<u8> {
        FlightSqlTicket {
            sql: sql.to_string(),
            org_id: ctx.org_id.0.clone(),
        }
        .encode_to_vec()
    }

    /// handle → `FlightSqlTicket`，并校验 ticket org 与 bearer org 一致
    /// （spec flight-sql：防跨 org 重放）。
    fn decode_handle(handle: &[u8], ctx: &IamContext) -> Result<FlightSqlTicket, Status> {
        let ticket = FlightSqlTicket::decode(handle)
            .map_err(|e| Status::invalid_argument(format!("decode statement handle: {e}")))?;
        if ticket.org_id != ctx.org_id.0 {
            return Err(Status::permission_denied(
                "ticket org does not match token org",
            ));
        }
        Ok(ticket)
    }

    /// SQL → `QueryRequest`：校验/改写 + stream hint + 缺省回看窗口（design D4）。
    fn build_query_request(&self, ctx: &IamContext, sql: &str) -> Result<QueryRequest, Status> {
        let prepared = prepare_flight_sql_select(sql).map_err(error_to_status)?;
        let now = TimestampMicros::now();
        let lookback_micros = i64::from(self.settings.default_lookback_hours) * 3600 * 1_000_000;
        Ok(QueryRequest {
            org_id: ctx.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: prepared.sql,
            time_range: TimeRange::new(TimestampMicros(now.0 - lookback_micros), now),
            stream: prepared
                .stream
                .map(|(name, stream_type)| StreamHint { name, stream_type }),
            limit: None,
            federation_clusters: Vec::new(),
        })
    }

    /// 执行 + 编码：`run_tracked` → 单 batch → FlightData 流。
    async fn execute_sql(
        &self,
        ctx: &IamContext,
        sql: &str,
    ) -> Result<Response<DoGetStream>, Status> {
        let req = self.build_query_request(ctx, sql)?;
        let out = self
            .query
            .run_tracked(req, ctx.user_id.clone(), ctx.organization_role_key())
            .await
            .map_err(error_to_status)?;
        let batch = super::encode::query_result_to_batch(&out)
            .map_err(|e| Status::internal(format!("arrow encode: {e}")))?;
        let stream = FlightDataEncoderBuilder::new()
            .with_schema(batch.schema())
            .build(futures::stream::once(async move { Ok(batch) }))
            .map_err(Status::from);
        Ok(Response::new(segmented_result_stream(
            stream,
            "flight_sql.do_get.stream",
            "flight",
        )))
    }

    /// 元数据 RPC 共用的 `FlightInfo`：ticket 原样回传 command（标准 Flight SQL 模式）。
    fn metadata_info(
        schema: &ArrowSchema,
        ticket_bytes: Vec<u8>,
        descriptor: FlightDescriptor,
    ) -> Result<Response<FlightInfo>, Status> {
        let endpoint = FlightEndpoint::new().with_ticket(Ticket::new(ticket_bytes));
        let info = FlightInfo::new()
            .try_with_schema(schema)
            .map_err(|e| Status::internal(format!("encode schema: {e}")))?
            .with_endpoint(endpoint)
            .with_descriptor(descriptor);
        Ok(Response::new(info))
    }

    /// statement / prepared statement 的 `FlightInfo`：**不带 schema**。
    ///
    /// 结果 schema 要执行后才知道；Flight SQL 里 `FlightInfo.schema` 是可选的，
    /// 客户端以 do_get 数据流自带的 schema 为准。注意不能放"0 字段空 schema"——
    /// ADBC（Go 驱动）会把它当预期 schema 与数据流严格比对，报 inconsistent schema。
    fn statement_info(ticket_bytes: Vec<u8>, descriptor: FlightDescriptor) -> Response<FlightInfo> {
        let endpoint = FlightEndpoint::new().with_ticket(Ticket::new(ticket_bytes));
        let info = FlightInfo::new()
            .with_endpoint(endpoint)
            .with_descriptor(descriptor);
        Response::new(info)
    }
}

/// 编码 metadata builder 产出的单个 batch 为 DoGet 流。
fn batch_stream(
    batch: Result<arrow::array::RecordBatch, arrow_flight::error::FlightError>,
) -> Response<DoGetStream> {
    let stream = FlightDataEncoderBuilder::new()
        .build(futures::stream::once(async move { batch }))
        .map_err(Status::from);
    Response::new(segmented_result_stream(
        stream,
        "flight_sql.metadata.stream",
        "flight",
    ))
}

/// 仅含 schema 的空 DoGet 流（0 行元数据结果用）。
fn empty_stream(schema: Arc<ArrowSchema>) -> Response<DoGetStream> {
    let stream = FlightDataEncoderBuilder::new()
        .with_schema(schema)
        .build(futures::stream::empty())
        .map_err(Status::from);
    Response::new(segmented_result_stream(
        stream,
        "flight_sql.metadata.stream",
        "flight",
    ))
}

/// [`MsError`] → gRPC [`Status`]：语义对齐 HTTP 映射（`http_status_code`）。
/// 5xx 详情只进服务端日志，客户端拿泛化文案（与 HTTP `IntoResponse` 同策略）。
fn error_to_status(e: MsError) -> Status {
    match &e {
        MsError::NotFound(_) => Status::not_found(e.to_string()),
        MsError::InvalidArgument(_) | MsError::Validation { .. } => {
            Status::invalid_argument(e.to_string())
        }
        MsError::Unauthorized(_) => Status::unauthenticated(e.to_string()),
        MsError::Forbidden(_) => Status::permission_denied(e.to_string()),
        // gRPC 无 402 对应；订阅失效语义最贴近 permission_denied / resource_exhausted，
        // 取前者（与 Forbidden 同类，表"当前凭证不被允许"）。
        MsError::PaymentRequired(_) => Status::permission_denied(e.to_string()),
        // gRPC 无 413 对应；配额超限语义最贴近 resource_exhausted（与 429 同类）。
        MsError::ResourceExhausted(_) | MsError::PayloadTooLarge(_) => {
            Status::resource_exhausted(e.to_string())
        }
        MsError::Unavailable(_) => Status::unavailable(e.to_string()),
        MsError::Cancelled(_) => Status::cancelled(e.to_string()),
        MsError::Conflict(_) => Status::aborted(e.to_string()),
        MsError::Internal(_) | MsError::Other(_) => {
            tracing::error!(error = ?e, "flight sql internal error");
            Status::internal("internal error")
        }
    }
}

/// prepared statement 的占位 dataset schema（IPC bytes）。
///
/// 单字段 `__schema_pending_execution`：字段内容无意义，唯一作用是让 JDBC
/// 驱动把语句分类为 SELECT（见 `do_action_create_prepared_statement` 注释）。
fn placeholder_dataset_schema_bytes() -> Result<bytes::Bytes, Status> {
    let schema = ArrowSchema::new(vec![Field::new(
        "__schema_pending_execution",
        DataType::Utf8,
        true,
    )]);
    let message: IpcMessage = SchemaAsIpc::new(&schema, &IpcWriteOptions::default())
        .try_into()
        .map_err(|e| Status::internal(format!("serialize schema: {e}")))?;
    Ok(message.0)
}

/// basic auth 的 base64 解码：padding 宽容。
///
/// Rust/JDBC 客户端发规范 padding，但 ADBC（Go 驱动）发 **无 padding** 的
/// std base64 —— 两种都必须接受，否则 ADBC 用户在握手期报 "not decodable"。
fn decode_basic_b64(b64: &str) -> Option<Vec<u8>> {
    BASE64_STANDARD
        .decode(b64)
        .or_else(|_| BASE64_NO_PAD.decode(b64))
        .ok()
}

/// username → (email, org 选择子)。邮箱恰含一个 `@`，故仅当出现 ≥2 个 `@` 时
/// 末段才是 org 选择子（`alice@example.com@acme` → email + org）；其余原样当邮箱。
fn split_login_username(username: &str) -> (&str, Option<&str>) {
    if username.matches('@').count() >= 2
        && let Some((email, org)) = username.rsplit_once('@')
    {
        return (email, Some(org));
    }
    (username, None)
}

fn stream_type_schema(st: StreamType) -> &'static str {
    match st {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

#[tonic::async_trait]
impl FlightSqlService for FlightSqlGrpc {
    type FlightService = Self;

    /// Handshake（spec flight-sql）：basic auth 双凭据分发，复用既有认证体系 ——
    ///
    /// - password 以 `ms_` 开头 → API token 路径，校验后原样回 bearer（username 忽略）；
    /// - 其余 → username 当邮箱（末段可带 `@<org>` 选择子）、password 当账号密码，
    ///   走 `IamService::authenticate`（与 HTTP login 同函数）签 JWT 回 bearer。
    ///
    /// 两种路径都无服务端 session 状态。直接带 `Bearer <token>` 的客户端同样接受。
    async fn do_handshake(
        &self,
        request: Request<Streaming<HandshakeRequest>>,
    ) -> Result<
        Response<Pin<Box<dyn Stream<Item = Result<HandshakeResponse, Status>> + Send>>>,
        Status,
    > {
        let header = request
            .metadata()
            .get("authorization")
            .ok_or_else(|| Status::unauthenticated("missing authorization header"))?
            .to_str()
            .map_err(|_| Status::unauthenticated("authorization header not parsable"))?;

        let (token, ctx) = if let Some(b64) = header.strip_prefix("Basic ") {
            let decoded = decode_basic_b64(b64)
                .ok_or_else(|| Status::unauthenticated("basic auth not decodable"))?;
            let text = std::str::from_utf8(&decoded)
                .map_err(|_| Status::unauthenticated("basic auth not utf-8"))?;
            let (user, pass) = text
                .split_once(':')
                .ok_or_else(|| Status::unauthenticated("malformed basic auth"))?;
            if pass.starts_with("ms_") {
                let ctx = authenticate_api_token(pass, self.iam.as_ref(), self.api_tokens.clone())
                    .await
                    .map_err(|e| Status::unauthenticated(e.to_string()))?;
                (pass.to_string(), ctx)
            } else {
                // 账号密码：SSO-only 用户无本地密码，authenticate 必然失败 →
                // UNAUTHENTICATED（文档引导其使用 API token）。
                let (email, org_selector) = split_login_username(user);
                let logged_in = self
                    .iam
                    .authenticate(email, pass)
                    .await
                    .map_err(|e| Status::unauthenticated(e.to_string()))?;
                let m = self.resolve_membership(&logged_in.id, org_selector).await?;
                let jwt = self
                    .iam
                    .issue_token(&logged_in.id, &m.org_id)
                    .map_err(error_to_status)?;
                let ctx = IamContext {
                    user_id: logged_in.id,
                    org_id: m.org_id,
                    display_role: String::new(),
                    roles: Vec::new(),
                    credential_role_id: None,
                    credential_application_id: None,
                    scope: crate::domain::iam::IamScope::Organization,
                    permissions: std::collections::BTreeSet::new(),
                    features: std::collections::BTreeSet::new(),
                    policy_version: 0,
                };
                (jwt, ctx)
            }
        } else if let Some(t) = header.strip_prefix("Bearer ") {
            let ctx = authenticate_bearer(t, self.iam.as_ref(), self.api_tokens.clone())
                .await
                .map_err(|e| Status::unauthenticated(e.to_string()))?;
            (t.to_string(), ctx)
        } else {
            return Err(Status::unauthenticated(
                "expected Basic or Bearer authorization",
            ));
        };

        let mut ctx = ctx;
        self.iam_access
            .enrich_iam_context(&mut ctx)
            .await
            .map_err(|error| Status::permission_denied(error.to_string()))?;
        Permission::require_any_key(&ctx, &["streams.query", "sys.telemetry.read"])
            .map_err(|e| Status::permission_denied(e.to_string()))?;

        let response = HandshakeResponse {
            protocol_version: 0,
            payload: token.clone().into_bytes().into(),
        };
        let stream = futures::stream::iter([Ok(response)]);
        let mut resp: Response<Pin<Box<dyn Stream<Item = _> + Send>>> = Response::new(
            segmented_result_stream(stream, "flight_sql.handshake.stream", "grpc"),
        );
        let header = format!("Bearer {token}")
            .parse()
            .map_err(|_| Status::internal("auth header encode"))?;
        resp.metadata_mut().insert("authorization", header);
        Ok(resp)
    }

    // === statement ===

    async fn get_flight_info_statement(
        &self,
        query: CommandStatementQuery,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        let ctx = self.authenticate(&request).await?;
        // plan 阶段就把非 SELECT / 非法 SQL 拒掉，不签发 ticket。
        prepare_flight_sql_select(&query.query).map_err(error_to_status)?;
        let ticket = TicketStatementQuery {
            statement_handle: self.statement_handle(&ctx, &query.query).into(),
        };
        Ok(Self::statement_info(
            ticket.as_any().encode_to_vec(),
            request.into_inner(),
        ))
    }

    async fn do_get_statement(
        &self,
        ticket: TicketStatementQuery,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        let ctx = self.authenticate(&request).await?;
        let t = Self::decode_handle(ticket.statement_handle.as_ref(), &ctx)?;
        self.execute_sql(&ctx, &t.sql).await
    }

    // === prepared statement（最小实现，design D5：无状态、无参数绑定） ===

    async fn do_action_create_prepared_statement(
        &self,
        query: ActionCreatePreparedStatementRequest,
        request: Request<Action>,
    ) -> Result<ActionCreatePreparedStatementResult, Status> {
        let ctx = self.authenticate(&request).await?;
        prepare_flight_sql_select(&query.query).map_err(error_to_status)?;
        // dataset_schema 必须**非空**：JDBC 驱动用"字段列表是否为空"判断
        // SELECT vs UPDATE（空 → 走 DoPut update，SELECT 结果全丢）。真实结果
        // schema 要执行后才知道，这里给单字段占位 schema —— 仅作"这是 SELECT"
        // 的分类信号，客户端实际以 do_get 数据流自带的 schema 为准（ADBC 的
        // 一致性校验只比对 FlightInfo.schema，不比对本字段）。
        Ok(ActionCreatePreparedStatementResult {
            prepared_statement_handle: self.statement_handle(&ctx, &query.query).into(),
            dataset_schema: placeholder_dataset_schema_bytes()?,
            parameter_schema: bytes::Bytes::new(),
        })
    }

    async fn do_action_close_prepared_statement(
        &self,
        query: ActionClosePreparedStatementRequest,
        request: Request<Action>,
    ) -> Result<(), Status> {
        let ctx = self.authenticate(&request).await?;
        // handle 无服务端状态，close 仅校验合法性后 no-op。
        let _ = Self::decode_handle(query.prepared_statement_handle.as_ref(), &ctx)?;
        Ok(())
    }

    async fn get_flight_info_prepared_statement(
        &self,
        cmd: CommandPreparedStatementQuery,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        let ctx = self.authenticate(&request).await?;
        let _ = Self::decode_handle(cmd.prepared_statement_handle.as_ref(), &ctx)?;
        Ok(Self::statement_info(
            cmd.as_any().encode_to_vec(),
            request.into_inner(),
        ))
    }

    async fn do_get_prepared_statement(
        &self,
        cmd: CommandPreparedStatementQuery,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        let ctx = self.authenticate(&request).await?;
        let t = Self::decode_handle(cmd.prepared_statement_handle.as_ref(), &ctx)?;
        self.execute_sql(&ctx, &t.sql).await
    }

    // === metadata（spec flight-sql：catalog `molesignal` / schema = stream_type） ===

    async fn get_flight_info_catalogs(
        &self,
        query: CommandGetCatalogs,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&query.into_builder().schema(), ticket, request.into_inner())
    }

    async fn do_get_catalogs(
        &self,
        query: CommandGetCatalogs,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        let mut builder = query.into_builder();
        builder.append(CATALOG);
        Ok(batch_stream(builder.build()))
    }

    async fn get_flight_info_schemas(
        &self,
        query: CommandGetDbSchemas,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&query.into_builder().schema(), ticket, request.into_inner())
    }

    async fn do_get_schemas(
        &self,
        query: CommandGetDbSchemas,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        let mut builder = query.into_builder();
        for st in [
            StreamType::Logs,
            StreamType::Metrics,
            StreamType::Traces,
            StreamType::Extend,
        ] {
            builder.append(CATALOG, stream_type_schema(st));
        }
        Ok(batch_stream(builder.build()))
    }

    async fn get_flight_info_tables(
        &self,
        query: CommandGetTables,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&query.into_builder().schema(), ticket, request.into_inner())
    }

    /// 表清单按 `IamContext.org_id` 查 streams repo —— org 隔离与查询路径一致。
    /// filter pattern / include_schema 由 `GetTablesBuilder` 统一处理。
    async fn do_get_tables(
        &self,
        query: CommandGetTables,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        let ctx = self.authenticate(&request).await?;
        let mut defs = self
            .streams
            .list(&ctx.org_id)
            .await
            .map_err(error_to_status)?;
        defs.sort_by(|a, b| {
            (stream_type_schema(a.stream_type), a.name.as_str())
                .cmp(&(stream_type_schema(b.stream_type), b.name.as_str()))
        });
        let mut builder = query.into_builder();
        for def in &defs {
            // 表 schema 用 stream 定义里的推断 schema（`to_arrow` 自动前置
            // `_timestamp`）。必须给真实列：DBeaver 数据浏览页靠 `getColumns()`
            // （include_schema=true 的本 RPC）建网格，空 schema = 0 列 = 空白页。
            builder
                .append(
                    CATALOG,
                    stream_type_schema(def.stream_type),
                    &def.name,
                    "TABLE",
                    to_arrow(&def.schema).as_ref(),
                )
                .map_err(Status::from)?;
        }
        Ok(batch_stream(builder.build()))
    }

    async fn get_flight_info_table_types(
        &self,
        query: CommandGetTableTypes,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&query.into_builder().schema(), ticket, request.into_inner())
    }

    async fn do_get_table_types(
        &self,
        query: CommandGetTableTypes,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        let mut builder = query.into_builder();
        builder.append("TABLE");
        Ok(batch_stream(builder.build()))
    }

    async fn get_flight_info_sql_info(
        &self,
        query: CommandGetSqlInfo,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(
            query.into_builder(&SQL_INFO_DATA).schema().as_ref(),
            ticket,
            request.into_inner(),
        )
    }

    async fn do_get_sql_info(
        &self,
        query: CommandGetSqlInfo,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(batch_stream(query.into_builder(&SQL_INFO_DATA).build()))
    }

    // === 约束类元数据（stream 无主外键概念 → 0 行；DBeaver 打开表时会查，
    //     trait 默认的 Unimplemented 会直接弹错，必须实现） ===

    async fn get_flight_info_primary_keys(
        &self,
        query: CommandGetPrimaryKeys,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&PRIMARY_KEYS_SCHEMA, ticket, request.into_inner())
    }

    async fn do_get_primary_keys(
        &self,
        _query: CommandGetPrimaryKeys,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(empty_stream(PRIMARY_KEYS_SCHEMA.clone()))
    }

    async fn get_flight_info_exported_keys(
        &self,
        query: CommandGetExportedKeys,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&FOREIGN_KEYS_SCHEMA, ticket, request.into_inner())
    }

    async fn do_get_exported_keys(
        &self,
        _query: CommandGetExportedKeys,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(empty_stream(FOREIGN_KEYS_SCHEMA.clone()))
    }

    async fn get_flight_info_imported_keys(
        &self,
        query: CommandGetImportedKeys,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&FOREIGN_KEYS_SCHEMA, ticket, request.into_inner())
    }

    async fn do_get_imported_keys(
        &self,
        _query: CommandGetImportedKeys,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(empty_stream(FOREIGN_KEYS_SCHEMA.clone()))
    }

    async fn get_flight_info_cross_reference(
        &self,
        query: CommandGetCrossReference,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(&FOREIGN_KEYS_SCHEMA, ticket, request.into_inner())
    }

    async fn do_get_cross_reference(
        &self,
        _query: CommandGetCrossReference,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(empty_stream(FOREIGN_KEYS_SCHEMA.clone()))
    }

    async fn get_flight_info_xdbc_type_info(
        &self,
        query: CommandGetXdbcTypeInfo,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        self.authenticate(&request).await?;
        let ticket = query.as_any().encode_to_vec();
        Self::metadata_info(
            query.into_builder(&XDBC_TYPE_INFO_DATA).schema().as_ref(),
            ticket,
            request.into_inner(),
        )
    }

    async fn do_get_xdbc_type_info(
        &self,
        query: CommandGetXdbcTypeInfo,
        request: Request<Ticket>,
    ) -> Result<Response<DoGetStream>, Status> {
        self.authenticate(&request).await?;
        Ok(batch_stream(
            query.into_builder(&XDBC_TYPE_INFO_DATA).build(),
        ))
    }

    async fn register_sql_info(&self, _id: i32, _result: &SqlInfo) {
        // SQL_INFO_DATA 是编译期静态集，不支持运行时注册。
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_basic_b64, split_login_username};

    #[test]
    fn basic_b64_accepts_padded_and_unpadded() {
        // 38 字节明文 → 标准 base64 带一个 '=' padding
        let plain = b"demo@molesignal.local:MoleFlight-5083!";
        let padded = "ZGVtb0Btb2xlc2lnbmFsLmxvY2FsOk1vbGVGbGlnaHQtNTA4MyE=";
        let unpadded = padded.trim_end_matches('=');
        assert_eq!(decode_basic_b64(padded).as_deref(), Some(plain.as_slice()));
        // ADBC（Go 驱动）发无 padding 形态，必须同样接受
        assert_eq!(
            decode_basic_b64(unpadded).as_deref(),
            Some(plain.as_slice())
        );
        assert_eq!(decode_basic_b64("!!!not-base64!!!"), None);
    }

    #[test]
    fn login_username_splits_org_selector() {
        // 纯邮箱（一个 @）：整体当邮箱，无选择子
        assert_eq!(
            split_login_username("alice@example.com"),
            ("alice@example.com", None)
        );
        // ≥2 个 @：末段为 org 选择子
        assert_eq!(
            split_login_username("alice@example.com@acme"),
            ("alice@example.com", Some("acme"))
        );
        // org 名含连字符 / id 形态
        assert_eq!(
            split_login_username("a@b.io@org-3f2c"),
            ("a@b.io", Some("org-3f2c"))
        );
        // 无 @（非法邮箱）：原样返回，交给 authenticate 报 invalid credentials
        assert_eq!(split_login_username("token"), ("token", None));
    }
}
