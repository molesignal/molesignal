// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `query.v1.QueryExecutionService` 的 server 端：用本节点的引擎执行其它节点转来的查询。
//!
//! 执行的是本地引擎（多个 querier 时由它们协调分片），不经过 `QueryService` 的准入控制、
//! 结果缓存与脱敏：这些在接收用户请求的节点上已经执行过。

use std::sync::Arc;

use serde::Serialize;
use tonic::{Request, Response, Status};

use crate::{
    domain::{
        query::{PromqlEngine, QueryEngine, QueryRequest},
        storage::DatasetTypeId,
    },
    infra::cluster::grpc_status::error_to_status,
    protocol::query::v1::{
        ExecuteOp, ExecuteRequest, ExecuteResponse,
        query_execution_service_server::{QueryExecutionService, QueryExecutionServiceServer},
    },
    shared::Result,
};

pub struct QueryExecutionGrpc {
    sql: Arc<dyn QueryEngine>,
    promql: Arc<dyn PromqlEngine>,
}

impl QueryExecutionGrpc {
    pub fn new(sql: Arc<dyn QueryEngine>, promql: Arc<dyn PromqlEngine>) -> Self {
        Self { sql, promql }
    }

    pub fn into_server(self) -> QueryExecutionServiceServer<Self> {
        QueryExecutionServiceServer::new(self)
    }
}

fn encode<T: Serialize>(result: Result<T>) -> std::result::Result<Vec<u8>, Status> {
    let value = result.map_err(error_to_status)?;
    serde_json::to_vec(&value).map_err(|error| Status::internal(format!("encode result: {error}")))
}

#[tonic::async_trait]
impl QueryExecutionService for QueryExecutionGrpc {
    async fn execute(
        &self,
        request: Request<ExecuteRequest>,
    ) -> std::result::Result<Response<ExecuteResponse>, Status> {
        let ExecuteRequest {
            op,
            request,
            dataset_type,
        } = request.into_inner();
        let req: QueryRequest = serde_json::from_slice(&request)
            .map_err(|error| Status::invalid_argument(format!("query decode: {error}")))?;
        let result = match ExecuteOp::try_from(op).unwrap_or(ExecuteOp::Unspecified) {
            ExecuteOp::Sql => encode(self.sql.execute(req).await)?,
            ExecuteOp::SqlDataset => {
                let dataset = DatasetTypeId::new(dataset_type)
                    .map_err(|error| Status::invalid_argument(error.to_string()))?;
                encode(self.sql.execute_dataset(req, dataset).await)?
            }
            ExecuteOp::SqlExplain => encode(self.sql.explain(req).await)?,
            ExecuteOp::Promql => encode(self.promql.execute(req).await)?,
            ExecuteOp::PromqlExemplars => encode(self.promql.query_exemplars(req).await)?,
            ExecuteOp::Unspecified => {
                return Err(Status::invalid_argument("execute op unspecified"));
            }
        };
        Ok(Response::new(ExecuteResponse {
            result: result.into(),
        }))
    }
}
