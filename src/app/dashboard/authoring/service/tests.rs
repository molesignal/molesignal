// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::*;
use crate::domain::dashboard::{
    Dashboard, Folder,
    authoring::{
        ConsumeDashboardDraft, DashboardQueryPreflight, DraftConsumption, PanelPreflight,
        PreflightReport, PreflightStatus, PreflightWarning, visualization_manifest,
    },
    repositories::{DashboardRepository, FolderRepository},
};

const VALID_AUTHORING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/valid/authoring-v1-promql.json"
));

#[derive(Default)]
struct DraftState {
    drafts: HashMap<Id, DashboardDraft>,
    dashboards: HashMap<Id, Dashboard>,
}

#[derive(Default)]
struct MemoryDrafts {
    state: Mutex<DraftState>,
}

#[async_trait]
impl DashboardDraftRepository for MemoryDrafts {
    async fn create(&self, draft: DashboardDraft) -> Result<DashboardDraft> {
        self.state
            .lock()
            .drafts
            .insert(draft.id.clone(), draft.clone());
        Ok(draft)
    }

    async fn get(
        &self,
        org_id: &Id,
        draft_id: &Id,
        now: TimestampMicros,
    ) -> Result<DashboardDraft> {
        let mut state = self.state.lock();
        let draft = state
            .drafts
            .get_mut(draft_id)
            .filter(|draft| &draft.org_id == org_id)
            .ok_or_else(|| Error::not_found("dashboard draft not found"))?;
        if draft.status == DashboardDraftStatus::Ready && draft.expires_at <= now {
            draft.status = DashboardDraftStatus::Expired;
        }
        Ok(draft.clone())
    }

    async fn consume_and_create(&self, request: ConsumeDashboardDraft) -> Result<DraftConsumption> {
        let mut state = self.state.lock();
        let draft = state
            .drafts
            .get(&request.draft_id)
            .filter(|draft| draft.org_id == request.org_id && draft.created_by == request.actor)
            .cloned()
            .ok_or_else(|| Error::not_found("dashboard draft not found"))?;
        if draft.status == DashboardDraftStatus::Consumed {
            let dashboard = state
                .dashboards
                .get(draft.dashboard_id.as_ref().unwrap())
                .cloned()
                .unwrap();
            return Ok(DraftConsumption::Replay(dashboard));
        }
        if draft.status == DashboardDraftStatus::Expired || draft.expires_at <= request.now {
            return Err(draft_error("DRAFT_EXPIRED", "draft expired"));
        }
        if draft.model_hash != request.expected_hash {
            return Err(draft_error("DRAFT_HASH_MISMATCH", "hash mismatch"));
        }
        if draft.compiler_version != request.compiler_version {
            return Err(draft_error("DRAFT_STALE", "compiler mismatch"));
        }
        let dashboard = request.dashboard;
        state
            .dashboards
            .insert(dashboard.id.clone(), dashboard.clone());
        let saved = state.drafts.get_mut(&request.draft_id).unwrap();
        saved.status = DashboardDraftStatus::Consumed;
        saved.dashboard_id = Some(dashboard.id.clone());
        saved.consumed_at = Some(request.now);
        Ok(DraftConsumption::Created(dashboard))
    }
}

#[derive(Default)]
struct MemoryDashboards;

#[async_trait]
impl DashboardRepository for MemoryDashboards {
    async fn create(&self, dashboard: Dashboard) -> Result<Dashboard> {
        Ok(dashboard)
    }
    async fn update(&self, dashboard: Dashboard) -> Result<Dashboard> {
        Ok(dashboard)
    }
    async fn get(&self, _id: &Id) -> Result<Dashboard> {
        Err(Error::not_found("dashboard"))
    }
    async fn get_by_uid(&self, _org_id: &Id, _uid: &str) -> Result<Dashboard> {
        Err(Error::not_found("dashboard"))
    }
    async fn list(&self, _org_id: &Id, _folder_id: Option<&Id>) -> Result<Vec<Dashboard>> {
        Ok(Vec::new())
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct MemoryFolders;

#[async_trait]
impl FolderRepository for MemoryFolders {
    async fn create(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }
    async fn get_by_id(&self, _id: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }
    async fn get(&self, _org_id: &Id, _id: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }
    async fn list(&self, _org_id: &Id) -> Result<Vec<Folder>> {
        Ok(Vec::new())
    }
    async fn update(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }
    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

struct StaticPreflight(Mutex<PreflightReport>);

#[async_trait]
impl DashboardQueryPreflight for StaticPreflight {
    async fn preflight(
        &self,
        _org_id: &Id,
        _actor: &Id,
        _spec: &DashboardAuthoringSpec,
    ) -> Result<PreflightReport> {
        Ok(self.0.lock().clone())
    }
}

struct MutableContracts(Mutex<Arc<ResolvedDashboardContracts>>);

impl MutableContracts {
    fn replace(&self, contracts: Arc<ResolvedDashboardContracts>) {
        *self.0.lock() = contracts;
    }
}

#[async_trait]
impl DashboardContractResolver for MutableContracts {
    async fn active(&self) -> Result<Arc<ResolvedDashboardContracts>> {
        Ok(Arc::clone(&self.0.lock()))
    }
}

fn service(report: PreflightReport) -> (Arc<DashboardAuthoringService>, Arc<MemoryDrafts>) {
    let drafts = Arc::new(MemoryDrafts::default());
    let dashboard = Arc::new(
        DashboardService::new(Arc::new(MemoryDashboards), Arc::new(MemoryFolders))
            .with_draft_repository(drafts.clone()),
    );
    (
        Arc::new(DashboardAuthoringService::new(
            drafts.clone(),
            Arc::new(StaticPreflight(Mutex::new(report))),
            dashboard,
        )),
        drafts,
    )
}

fn service_with_contracts(
    report: PreflightReport,
    contracts: Arc<dyn DashboardContractResolver>,
) -> (Arc<DashboardAuthoringService>, Arc<MemoryDrafts>) {
    let drafts = Arc::new(MemoryDrafts::default());
    let dashboard = Arc::new(
        DashboardService::new(Arc::new(MemoryDashboards), Arc::new(MemoryFolders))
            .with_draft_repository(drafts.clone())
            .with_contract_resolver(contracts.clone()),
    );
    (
        Arc::new(
            DashboardAuthoringService::new(
                drafts.clone(),
                Arc::new(StaticPreflight(Mutex::new(report))),
                dashboard,
            )
            .with_contract_resolver(contracts),
        ),
        drafts,
    )
}

fn input() -> Value {
    serde_json::from_str(VALID_AUTHORING).unwrap()
}

fn issue_code(error: Error) -> String {
    match error {
        Error::Validation { issues, .. } => issues[0].code.clone(),
        other => panic!("expected validation error, got {other}"),
    }
}

#[test]
fn rejects_unsupported_version_before_deserialization() {
    let error = ensure_supported_authoring_version(
        &json!({
            "authoringVersion": 2
        }),
        visualization_manifest(),
    )
    .unwrap_err();
    assert_eq!(issue_code(error), "UNSUPPORTED_AUTHORING_VERSION");
}

#[tokio::test]
async fn prepare_persists_a_preview_with_empty_result_warning() {
    let report = PreflightReport {
        panels: vec![PanelPreflight {
            path: "/elements/1".into(),
            title: "Request rate".into(),
            query_kind: "promql".into(),
            status: PreflightStatus::Empty,
            tested_from_micros: 10,
            tested_to_micros: 20,
            returned_rows: 0,
            scanned_rows: 0,
            took_ms: 2,
        }],
        warnings: vec![PreflightWarning {
            code: "EMPTY_RESULT".into(),
            path: "/elements/1".into(),
            message: "query returned no rows".into(),
        }],
        issues: Vec::new(),
    };
    let (service, drafts) = service(report);
    let prepared = service
        .prepare(Id("org-a".into()), Id("user-a".into()), input())
        .await
        .unwrap();
    assert_eq!(prepared.warnings[0].code, "EMPTY_RESULT");
    assert_eq!(
        prepared.preview_route,
        format!("/ai/dashboard-drafts/{}", prepared.draft_id.0)
    );
    let state = drafts.state.lock();
    assert_eq!(state.drafts.len(), 1);
    let persisted = state.drafts.get(&prepared.draft_id).unwrap();
    assert_eq!(persisted.contract_binding_revision, 1);
    assert_eq!(persisted.authoring_schema_hash.len(), 64);
    assert_eq!(persisted.model_schema_hash.len(), 64);
    assert_eq!(persisted.visualization_schema_hash.len(), 64);
}

#[tokio::test]
async fn preflight_issue_never_persists_a_draft() {
    let report = PreflightReport {
        issues: vec![ContractIssue::new(
            "INVALID_QUERY",
            "/elements/1/queries/0",
            "query cannot be planned",
            true,
        )],
        ..PreflightReport::default()
    };
    let (service, drafts) = service(report);
    let error = service
        .prepare(Id("org-a".into()), Id("user-a".into()), input())
        .await
        .unwrap_err();
    assert_eq!(issue_code(error), "INVALID_QUERY");
    assert!(drafts.state.lock().drafts.is_empty());
}

#[tokio::test]
async fn draft_guards_return_stable_codes_and_hide_other_creators() {
    let (service, drafts) = service(PreflightReport::default());
    let org = Id("org-a".into());
    let actor = Id("user-a".into());

    let expired = service
        .prepare(org.clone(), actor.clone(), input())
        .await
        .unwrap();
    drafts
        .state
        .lock()
        .drafts
        .get_mut(&expired.draft_id)
        .unwrap()
        .expires_at = TimestampMicros(0);
    assert_eq!(
        issue_code(
            service
                .get_draft(&org, &actor, &expired.draft_id)
                .await
                .unwrap_err()
        ),
        "DRAFT_EXPIRED"
    );

    let stale = service
        .prepare(org.clone(), actor.clone(), input())
        .await
        .unwrap();
    drafts
        .state
        .lock()
        .drafts
        .get_mut(&stale.draft_id)
        .unwrap()
        .compiler_version = "old-compiler".into();
    assert_eq!(
        issue_code(
            service
                .get_draft(&org, &actor, &stale.draft_id)
                .await
                .unwrap_err()
        ),
        "DRAFT_STALE"
    );

    let hash = service
        .prepare(org.clone(), actor.clone(), input())
        .await
        .unwrap();
    assert_eq!(
        issue_code(
            service
                .validate_reference(&org, &actor, &hash.draft_id, "0")
                .await
                .unwrap_err()
        ),
        "DRAFT_HASH_MISMATCH"
    );
    assert!(
        service
            .get_draft(&org, &Id("other-user".into()), &hash.draft_id)
            .await
            .is_err()
    );
    assert!(
        service
            .get_draft(&Id("other-org".into()), &actor, &hash.draft_id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_consumption_creates_once_and_replays_the_same_dashboard() {
    let (service, drafts) = service(PreflightReport::default());
    let org = Id("org-a".into());
    let actor = Id("user-a".into());
    let prepared = service
        .prepare(org.clone(), actor.clone(), input())
        .await
        .unwrap();
    let left = service.create_from_draft(
        org.clone(),
        actor.clone(),
        prepared.draft_id.clone(),
        prepared.model_hash.clone(),
    );
    let right = service.create_from_draft(
        org.clone(),
        actor.clone(),
        prepared.draft_id.clone(),
        prepared.model_hash.clone(),
    );
    let (left, right) = tokio::join!(left, right);
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.replayed(), right.replayed());
    assert_eq!(left.dashboard().id, right.dashboard().id);
    assert_eq!(drafts.state.lock().dashboards.len(), 1);

    let replay = service
        .create_from_draft(org, actor, prepared.draft_id, prepared.model_hash)
        .await
        .unwrap();
    assert!(replay.replayed());
    assert_eq!(replay.dashboard().id, left.dashboard().id);
}

#[tokio::test]
async fn binding_revision_change_rejects_prepared_draft_before_reference_or_execution() {
    let initial = builtin_dashboard_contract_resolver()
        .active()
        .await
        .unwrap();
    let resolver = Arc::new(MutableContracts(Mutex::new(Arc::clone(&initial))));
    let (service, _) = service_with_contracts(PreflightReport::default(), resolver.clone());
    let org = Id("org-a".into());
    let actor = Id("user-a".into());
    let prepared = service
        .prepare(org.clone(), actor.clone(), input())
        .await
        .unwrap();

    let mut binding = initial.binding.clone();
    binding.revision += 1;
    binding.updated_at = TimestampMicros(binding.updated_at.0 + 1);
    resolver.replace(Arc::new(ResolvedDashboardContracts {
        binding,
        model_validator: initial.model_validator.clone(),
        authoring_validator: initial.authoring_validator.clone(),
        manifest: initial.manifest.clone(),
    }));

    let reference_error = service
        .validate_reference(&org, &actor, &prepared.draft_id, &prepared.model_hash)
        .await
        .unwrap_err();
    assert_eq!(issue_code(reference_error), "DRAFT_STALE");
    let execution_error = service
        .create_from_draft(org, actor, prepared.draft_id, prepared.model_hash)
        .await
        .unwrap_err();
    assert_eq!(issue_code(execution_error), "DRAFT_STALE");
}
