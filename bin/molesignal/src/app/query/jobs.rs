// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Durable asynchronous search-job use cases shared by HTTP, Mole Agent, and inbound MCP.
//!
//! 任务属于提交它的人。读取、取消、重试和删除都要说明是谁在问（[`SearchJobViewer`]）：
//! 组织管理员看得到、动得了所有人的任务，其他人只有自己提交的。别人的任务对后者来说不存在，
//! 回答与查一个不存在的任务相同。

use std::sync::Arc;

use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjectPath};
use serde::Serialize;
use serde_json::Value;

use super::QueryService;
use crate::{
    domain::{iam::IamContext, query::QueryRequest},
    infra::persistence::repositories::search::jobs::{
        SearchJob, SearchJobCounts, SearchJobListQuery, SearchJobRepository, SearchJobState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFAULT_TTL_SECS: i64 = 7 * 86_400;

/// 持有它的人管理整个组织的任务：与取消别人正在运行的查询是同一个权限。
const ORGANIZATION_ADMINISTRATOR: &str = "org.settings.manage";

/// 系统范围里没有组织权限；能管理系统遥测的人管理那里的任务。
const SYSTEM_ADMINISTRATOR: &str = "sys.telemetry.manage";

/// 谁在看任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchJobViewer {
    /// 管理员：组织里所有人的任务。
    Administrator,
    /// 其他人：只有这位用户自己提交的任务。
    Submitter(Id),
}

impl SearchJobViewer {
    pub fn of(context: &IamContext) -> Self {
        if context.has_permission(ORGANIZATION_ADMINISTRATOR)
            || context.has_permission(SYSTEM_ADMINISTRATOR)
        {
            Self::Administrator
        } else {
            Self::Submitter(context.user_id.clone())
        }
    }

    /// 列表和计数要限定到的提交人；管理员不限定。
    fn submitter(&self) -> Option<&Id> {
        match self {
            Self::Administrator => None,
            Self::Submitter(user_id) => Some(user_id),
        }
    }

    /// 放行看得到的任务。看不到的与不存在的是同一个回答，不透露别人有没有这个任务。
    fn admit(&self, job: SearchJob) -> Result<SearchJob> {
        match self.submitter() {
            Some(user_id) if job.user_id != *user_id => Err(Error::not_found("row not found")),
            _ => Ok(job),
        }
    }
}

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

    pub async fn get(&self, org_id: &Id, id: &Id, viewer: &SearchJobViewer) -> Result<SearchJob> {
        viewer.admit(self.repository.get(org_id, id).await?)
    }

    pub async fn list(
        &self,
        org_id: &Id,
        viewer: &SearchJobViewer,
        limit: i64,
    ) -> Result<Vec<SearchJob>> {
        self.list_page(org_id, viewer, Vec::new(), limit, 0).await
    }

    /// 一页任务，最近提交的在前。`states` 为空表示不按状态筛选，`offset` 是跳过的条数。
    pub async fn list_page(
        &self,
        org_id: &Id,
        viewer: &SearchJobViewer,
        states: Vec<SearchJobState>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SearchJob>> {
        let query = SearchJobListQuery {
            states,
            submitter: viewer.submitter().cloned(),
            limit: limit.clamp(1, 1_000),
            offset: offset.max(0),
        };

        self.repository.list(org_id, &query).await
    }

    /// `viewer` 看得到的任务在各状态的数目。
    pub async fn counts(&self, org_id: &Id, viewer: &SearchJobViewer) -> Result<SearchJobCounts> {
        self.repository
            .count_by_state(org_id, viewer.submitter())
            .await
    }

    pub async fn results(
        &self,
        org_id: &Id,
        id: &Id,
        viewer: &SearchJobViewer,
        page: i64,
        page_size: i64,
    ) -> Result<SearchJobResultPage> {
        let job = self.get(org_id, id, viewer).await?;
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

    pub async fn cancel(
        &self,
        org_id: &Id,
        id: &Id,
        viewer: &SearchJobViewer,
    ) -> Result<SearchJob> {
        // 先确认这个任务是问的人看得到的，再去改它。
        self.get(org_id, id, viewer).await?;

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

    pub async fn retry(&self, org_id: &Id, id: &Id, viewer: &SearchJobViewer) -> Result<SearchJob> {
        let previous = self.get(org_id, id, viewer).await?;
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

    pub async fn delete(&self, org_id: &Id, id: &Id, viewer: &SearchJobViewer) -> Result<()> {
        let job = self.get(org_id, id, viewer).await?;

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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Mutex, time::Duration};

    use async_trait::async_trait;
    use object_store::{PutPayload, memory::InMemory};

    use super::*;
    use crate::{
        app::search::{AdmissionConfig, AdmissionController},
        domain::{
            iam::IamScope,
            query::{PromqlEngine, QueryEngine, QueryResult},
        },
    };

    const ADA: &str = "u-ada";
    const BO: &str = "u-bo";

    fn context(user: &str, permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string(user),
            org_id: Id::from_string("org-1"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: permissions
                .iter()
                .map(|key| (*key).to_owned())
                .collect::<BTreeSet<_>>(),
            policy_version: 0,
        }
    }

    fn job(id: &str, user: &str, state: SearchJobState) -> SearchJob {
        SearchJob {
            id: Id::from_string(id),
            org_id: Id::from_string("org-1"),
            user_id: Id::from_string(user),
            role_key: String::new(),
            request_json: serde_json::json!({}),
            trace_link: None,
            state,
            attempt: 0,
            result_object_key: (state == SearchJobState::Done)
                .then(|| format!("search_jobs/v1/org-1/{id}-0.ndjson")),
            result_rows: (state == SearchJobState::Done).then_some(1),
            error: None,
            submitted_at: TimestampMicros(1),
            started_at: None,
            finished_at: None,
            expires_at: TimestampMicros(9_000_000),
        }
    }

    /// 任务表：只做这些测试用得到的事。
    #[derive(Default)]
    struct Jobs(Mutex<Vec<SearchJob>>);

    impl Jobs {
        fn state_of(&self, id: &str) -> Option<SearchJobState> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.id.as_str() == id)
                .map(|job| job.state)
        }

        fn set_state(&self, id: &Id, state: SearchJobState) -> Result<SearchJob> {
            let mut jobs = self.0.lock().unwrap();

            let job = jobs
                .iter_mut()
                .find(|job| job.id == *id)
                .ok_or_else(|| Error::not_found("row not found"))?;

            job.state = state;
            Ok(job.clone())
        }
    }

    #[async_trait]
    impl SearchJobRepository for Jobs {
        async fn create(&self, job: SearchJob) -> Result<SearchJob> {
            self.0.lock().unwrap().push(job.clone());
            Ok(job)
        }

        async fn get(&self, _: &Id, id: &Id) -> Result<SearchJob> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.id == *id)
                .cloned()
                .ok_or_else(|| Error::not_found("row not found"))
        }

        async fn list(&self, _: &Id, query: &SearchJobListQuery) -> Result<Vec<SearchJob>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|job| query.states.is_empty() || query.states.contains(&job.state))
                .filter(|job| {
                    query
                        .submitter
                        .as_ref()
                        .is_none_or(|user| job.user_id == *user)
                })
                .cloned()
                .collect())
        }

        async fn count_by_state(&self, _: &Id, submitter: Option<&Id>) -> Result<SearchJobCounts> {
            let jobs = self.0.lock().unwrap();

            let count = |state: SearchJobState| {
                jobs.iter()
                    .filter(|job| job.state == state)
                    .filter(|job| submitter.is_none_or(|user| job.user_id == *user))
                    .count() as i64
            };

            Ok(SearchJobCounts {
                pending: count(SearchJobState::Pending),
                running: count(SearchJobState::Running),
                done: count(SearchJobState::Done),
                failed: count(SearchJobState::Failed),
                cancelled: count(SearchJobState::Cancelled),
            })
        }

        async fn claim_next_pending(&self) -> Result<Option<SearchJob>> {
            Ok(None)
        }

        async fn wait_for_pending(&self, _: Duration) -> bool {
            false
        }

        async fn mark_done(
            &self,
            _: &Id,
            _: &Id,
            _: i32,
            _: &str,
            _: i64,
            _: TimestampMicros,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn mark_failed(
            &self,
            _: &Id,
            _: &Id,
            _: i32,
            _: &str,
            _: TimestampMicros,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn cancel(&self, _: &Id, id: &Id, _: TimestampMicros) -> Result<SearchJob> {
            self.set_state(id, SearchJobState::Cancelled)
        }

        async fn retry(
            &self,
            _: &Id,
            id: &Id,
            _: TimestampMicros,
            _: TimestampMicros,
        ) -> Result<SearchJob> {
            self.set_state(id, SearchJobState::Pending)
        }

        async fn list_expired(&self, _: TimestampMicros, _: i64) -> Result<Vec<SearchJob>> {
            Ok(Vec::new())
        }

        async fn delete(&self, _: &Id, id: &Id) -> Result<()> {
            self.0.lock().unwrap().retain(|job| job.id != *id);
            Ok(())
        }
    }

    struct NoEngine;

    #[async_trait]
    impl QueryEngine for NoEngine {
        async fn execute(&self, _: QueryRequest) -> Result<QueryResult> {
            Err(Error::invalid("no engine in this test"))
        }
    }

    #[async_trait]
    impl PromqlEngine for NoEngine {
        async fn execute(&self, _: QueryRequest) -> Result<QueryResult> {
            Err(Error::invalid("no engine in this test"))
        }
    }

    struct Fixture {
        service: SearchJobService,
        jobs: Arc<Jobs>,
        store: Arc<InMemory>,
        org: Id,
    }

    /// Ada 有一个在跑的任务；Bo 有在跑的、失败的和做完的各一个，做完的那个留着结果对象。
    async fn fixture() -> Fixture {
        let jobs = Arc::new(Jobs::default());

        for seed in [
            job("ada-running", ADA, SearchJobState::Running),
            job("bo-running", BO, SearchJobState::Running),
            job("bo-failed", BO, SearchJobState::Failed),
            job("bo-done", BO, SearchJobState::Done),
        ] {
            jobs.create(seed).await.unwrap();
        }

        let store = Arc::new(InMemory::new());

        store
            .put(
                &ObjectPath::from("search_jobs/v1/org-1/bo-done-0.ndjson"),
                PutPayload::from_static(b"{\"a\":1}\n"),
            )
            .await
            .unwrap();

        let query = Arc::new(QueryService::new(
            Arc::new(NoEngine),
            Arc::new(NoEngine),
            Arc::new(AdmissionController::new(AdmissionConfig::default())),
        ));

        Fixture {
            service: SearchJobService::new(jobs.clone(), store.clone(), query),
            jobs,
            store,
            org: Id::from_string("org-1"),
        }
    }

    fn id(value: &str) -> Id {
        Id::from_string(value)
    }

    fn is_missing<T: std::fmt::Debug>(result: &Result<T>) -> bool {
        matches!(result, Err(Error::NotFound(message)) if message == "row not found")
    }

    #[test]
    fn an_organization_administrator_sees_every_job_and_anyone_else_their_own() {
        assert_eq!(
            SearchJobViewer::of(&context(ADA, &["streams.query", "org.settings.manage"])),
            SearchJobViewer::Administrator
        );
        // 系统范围里没有组织权限，管理系统遥测的人就是那里的管理员。
        assert_eq!(
            SearchJobViewer::of(&context(
                ADA,
                &["sys.telemetry.read", "sys.telemetry.manage"]
            )),
            SearchJobViewer::Administrator
        );

        // 能查询、能读组织设置，都不等于能管别人的任务。
        for permissions in [
            &["streams.query"][..],
            &["streams.query", "org.settings.read", "pipelines.run"][..],
            &["sys.telemetry.read"][..],
            &[][..],
        ] {
            assert_eq!(
                SearchJobViewer::of(&context(ADA, permissions)),
                SearchJobViewer::Submitter(id(ADA)),
                "{permissions:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_member_lists_and_counts_only_the_jobs_they_submitted() {
        let fixture = fixture().await;
        let ada = SearchJobViewer::Submitter(id(ADA));

        let listed = fixture
            .service
            .list_page(&fixture.org, &ada, Vec::new(), 100, 0)
            .await
            .unwrap();
        assert_eq!(
            listed.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
            ["ada-running"]
        );

        let counts = fixture.service.counts(&fixture.org, &ada).await.unwrap();
        assert_eq!(counts.total(), 1);
        assert_eq!(counts.running, 1);

        let everyone = fixture
            .service
            .counts(&fixture.org, &SearchJobViewer::Administrator)
            .await
            .unwrap();
        assert_eq!(everyone.total(), 4);
        assert_eq!(everyone.running, 2);
    }

    #[tokio::test]
    async fn another_members_job_answers_as_a_job_that_does_not_exist() {
        let fixture = fixture().await;
        let ada = SearchJobViewer::Submitter(id(ADA));
        let org = &fixture.org;
        let service = &fixture.service;

        // 不存在的任务是这样回答的；别人的任务必须一模一样，不透露它在不在。
        assert!(is_missing(&service.get(org, &id("nobody"), &ada).await));

        assert!(is_missing(&service.get(org, &id("bo-done"), &ada).await));
        assert!(is_missing(
            &service.results(org, &id("bo-done"), &ada, 1, 10).await
        ));
        assert!(is_missing(
            &service.cancel(org, &id("bo-running"), &ada).await
        ));
        assert!(is_missing(
            &service.retry(org, &id("bo-failed"), &ada).await
        ));
        assert!(is_missing(&service.delete(org, &id("bo-done"), &ada).await));

        // 被拒绝的操作什么都没动：状态原样，结果对象还在。
        assert_eq!(
            fixture.jobs.state_of("bo-running"),
            Some(SearchJobState::Running)
        );
        assert_eq!(
            fixture.jobs.state_of("bo-failed"),
            Some(SearchJobState::Failed)
        );
        assert_eq!(fixture.jobs.state_of("bo-done"), Some(SearchJobState::Done));
        assert!(
            fixture
                .store
                .head(&ObjectPath::from("search_jobs/v1/org-1/bo-done-0.ndjson"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_member_manages_the_jobs_they_submitted() {
        let fixture = fixture().await;
        let bo = SearchJobViewer::Submitter(id(BO));
        let org = &fixture.org;
        let service = &fixture.service;

        let page = service
            .results(org, &id("bo-done"), &bo, 1, 10)
            .await
            .unwrap();
        assert_eq!(page.rows, vec![serde_json::json!({"a": 1})]);

        let cancelled = service.cancel(org, &id("bo-running"), &bo).await.unwrap();
        assert_eq!(cancelled.state, SearchJobState::Cancelled);

        let retried = service.retry(org, &id("bo-failed"), &bo).await.unwrap();
        assert_eq!(retried.state, SearchJobState::Pending);

        service.delete(org, &id("bo-done"), &bo).await.unwrap();
        assert_eq!(fixture.jobs.state_of("bo-done"), None);
        assert!(
            fixture
                .store
                .head(&ObjectPath::from("search_jobs/v1/org-1/bo-done-0.ndjson"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_administrator_manages_every_members_jobs() {
        let fixture = fixture().await;
        let administrator = SearchJobViewer::Administrator;
        let org = &fixture.org;
        let service = &fixture.service;

        let listed = service
            .list_page(org, &administrator, vec![SearchJobState::Running], 100, 0)
            .await
            .unwrap();
        assert_eq!(listed.len(), 2);

        let cancelled = service
            .cancel(org, &id("bo-running"), &administrator)
            .await
            .unwrap();
        assert_eq!(cancelled.state, SearchJobState::Cancelled);

        service
            .delete(org, &id("ada-running"), &administrator)
            .await
            .unwrap();
        assert_eq!(fixture.jobs.state_of("ada-running"), None);
    }
}
