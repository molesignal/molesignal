// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Durable asynchronous search-job use cases shared by HTTP, Mole Agent, and inbound MCP.

use std::sync::Arc;

use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjectPath};
use serde::Serialize;
use serde_json::Value;

use super::QueryService;
use crate::{
    domain::query::QueryRequest,
    infra::persistence::repositories::search::jobs::{
        SearchJob, SearchJobRepository, SearchJobState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFAULT_TTL_SECS: i64 = 7 * 86_400;

#[derive(Debug, Clone, Serialize)]
pub struct SearchJobResultPage {
    pub state: SearchJobState,
    pub result_rows: Option<i64>,
    pub page: i64,
    pub page_size: i64,
    pub rows: Vec<Value>,
    pub has_more: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct SearchJobService {
    repository: Arc<dyn SearchJobRepository>,
    object_store: Arc<dyn ObjectStore>,
    query: Arc<QueryService>,
}

impl SearchJobService {
    pub fn new(
        repository: Arc<dyn SearchJobRepository>,
        object_store: Arc<dyn ObjectStore>,
        query: Arc<QueryService>,
    ) -> Self {
        Self {
            repository,
            object_store,
            query,
        }
    }

    pub async fn submit(
        &self,
        org_id: Id,
        user_id: Id,
        role_key: String,
        request: QueryRequest,
        ttl_secs: Option<i64>,
    ) -> Result<SearchJob> {
        self.submit_with_id(Id::new(), org_id, user_id, role_key, request, ttl_secs)
            .await
    }

    pub async fn submit_with_id(
        &self,
        id: Id,
        org_id: Id,
        user_id: Id,
        role_key: String,
        mut request: QueryRequest,
        ttl_secs: Option<i64>,
    ) -> Result<SearchJob> {
        request.org_id = org_id.clone();
        let request_json = serde_json::to_value(request)
            .map_err(|error| Error::internal(format!("request json: {error}")))?;
        self.submit_json_with_id(id, org_id, user_id, role_key, request_json, ttl_secs)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn submit_backfill(
        &self,
        org_id: Id,
        user_id: Id,
        role_key: String,
        mut request: QueryRequest,
        pipeline_id: Id,
        window_micros: i64,
        ttl_secs: Option<i64>,
    ) -> Result<SearchJob> {
        request.org_id = org_id.clone();
        let mut request_json = serde_json::to_value(request)
            .map_err(|error| Error::internal(format!("request json: {error}")))?;
        let object = request_json
            .as_object_mut()
            .ok_or_else(|| Error::internal("search job request must serialize as an object"))?;
        object.insert("pipeline_id".into(), Value::String(pipeline_id.0));
        object.insert("backfill_window_micros".into(), window_micros.into());
        self.submit_json_with_id(Id::new(), org_id, user_id, role_key, request_json, ttl_secs)
            .await
    }

    async fn submit_json_with_id(
        &self,
        id: Id,
        org_id: Id,
        user_id: Id,
        role_key: String,
        request_json: Value,
        ttl_secs: Option<i64>,
    ) -> Result<SearchJob> {
        let now = TimestampMicros::now();
        let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS).clamp(60, 30 * 86_400);
        self.repository
            .create(SearchJob {
                id,
                org_id,
                user_id,
                role_key,
                request_json,
                trace_link: crate::shared::trace_context::current_trace_context()
                    .map(|context| context.serialized_link()),
                state: SearchJobState::Pending,
                attempt: 0,
                result_object_key: None,
                result_rows: None,
                error: None,
                submitted_at: now,
                started_at: None,
                finished_at: None,
                expires_at: TimestampMicros(now.0.saturating_add(ttl.saturating_mul(1_000_000))),
            })
            .await
    }

    pub async fn get(&self, org_id: &Id, id: &Id) -> Result<SearchJob> {
        self.repository.get(org_id, id).await
    }

    pub async fn list(&self, org_id: &Id, limit: i64) -> Result<Vec<SearchJob>> {
        self.repository.list(org_id, limit.clamp(1, 1_000)).await
    }

    pub async fn results(
        &self,
        org_id: &Id,
        id: &Id,
        page: i64,
        page_size: i64,
    ) -> Result<SearchJobResultPage> {
        let job = self.repository.get(org_id, id).await?;
        let page = page.max(1);
        let page_size = page_size.clamp(1, 10_000);
        let rows = if job.state == SearchJobState::Done {
            let key = job.result_object_key.as_deref().ok_or_else(|| {
                Error::internal("completed search job is missing its result object")
            })?;
            let bytes = self
                .object_store
                .get(&ObjectPath::from(key))
                .await
                .map_err(|error| Error::internal(format!("read search job result: {error}")))?
                .bytes()
                .await
                .map_err(|error| {
                    Error::internal(format!("read search job result bytes: {error}"))
                })?;
            let offset = page.saturating_sub(1).saturating_mul(page_size) as usize;
            bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .skip(offset)
                .take(page_size as usize)
                .map(|line| {
                    serde_json::from_slice(line).map_err(|error| {
                        Error::internal(format!("decode search job result: {error}"))
                    })
                })
                .collect::<Result<Vec<Value>>>()?
        } else {
            Vec::new()
        };
        let has_more = job
            .result_rows
            .is_some_and(|total| page.saturating_mul(page_size) < total);
        Ok(SearchJobResultPage {
            state: job.state,
            result_rows: job.result_rows,
            page,
            page_size,
            rows,
            has_more,
            error: job.error,
        })
    }

    pub async fn cancel(&self, org_id: &Id, id: &Id) -> Result<SearchJob> {
        let job = self
            .repository
            .cancel(org_id, id, TimestampMicros::now())
            .await?;
        let _ = self
            .query
            .registry()
            .cancel(&execution_id(id, job.attempt).0);
        Ok(job)
    }

    pub async fn retry(&self, org_id: &Id, id: &Id) -> Result<SearchJob> {
        let previous = self.repository.get(org_id, id).await?;
        let now = TimestampMicros::now();
        let ttl = previous
            .expires_at
            .0
            .saturating_sub(previous.submitted_at.0)
            .max(60 * 1_000_000);
        let job = self
            .repository
            .retry(org_id, id, now, TimestampMicros(now.0.saturating_add(ttl)))
            .await?;
        if let Some(key) = previous.result_object_key {
            let _ = self.object_store.delete(&ObjectPath::from(key)).await;
        }
        Ok(job)
    }

    pub async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        let job = self.repository.get(org_id, id).await?;
        if matches!(job.state, SearchJobState::Pending | SearchJobState::Running) {
            let _ = self
                .query
                .registry()
                .cancel(&execution_id(id, job.attempt).0);
        }
        if let Some(key) = job.result_object_key {
            let _ = self.object_store.delete(&ObjectPath::from(key)).await;
        }
        self.repository.delete(org_id, id).await
    }
}

pub(crate) fn execution_id(job_id: &Id, attempt: i32) -> Id {
    Id(format!("search-job-{}-{attempt}", job_id.0))
}
