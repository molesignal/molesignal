// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use serde_json::Value;

use crate::{
    app::dashboard::contract_registry::{
        DashboardContractResolver, ResolvedDashboardContracts, builtin_dashboard_contract_resolver,
    },
    domain::dashboard::{
        Dashboard,
        authoring::{
            ConsumeDashboardDraft, DashboardDraft, DashboardDraftRepository, DraftConsumption,
        },
        repositories::{DashboardRepository, FolderRepository},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod authoring;
pub mod contract_registry;
pub mod validation;

#[cfg(test)]
mod tests;

use validation::{DashboardValidationMode, ensure_dashboard_model_with};

pub struct DashboardService {
    dashboards: Arc<dyn DashboardRepository>,
    folders: Arc<dyn FolderRepository>,
    drafts: Option<Arc<dyn DashboardDraftRepository>>,
    contracts: Arc<dyn DashboardContractResolver>,
}

impl DashboardService {
    pub fn new(
        dashboards: Arc<dyn DashboardRepository>,
        folders: Arc<dyn FolderRepository>,
    ) -> Self {
        Self {
            dashboards,
            folders,
            drafts: None,
            contracts: builtin_dashboard_contract_resolver(),
        }
    }

    pub fn with_draft_repository(mut self, drafts: Arc<dyn DashboardDraftRepository>) -> Self {
        self.drafts = Some(drafts);
        self
    }

    pub fn with_contract_resolver(mut self, contracts: Arc<dyn DashboardContractResolver>) -> Self {
        self.contracts = contracts;
        self
    }

    pub fn folders(&self) -> &dyn FolderRepository {
        &*self.folders
    }

    pub async fn create(
        &self,
        org_id: Id,
        folder_id: Option<Id>,
        creator: Id,
        model: Value,
    ) -> Result<Dashboard> {
        self.create_with_mode(
            org_id,
            folder_id,
            creator,
            model,
            DashboardValidationMode::Native,
        )
        .await
    }

    /// Stores a normalized Grafana import while preserving vendor extension fields.
    pub async fn create_grafana_import(
        &self,
        org_id: Id,
        folder_id: Option<Id>,
        creator: Id,
        model: Value,
    ) -> Result<Dashboard> {
        self.create_with_mode(
            org_id,
            folder_id,
            creator,
            model,
            DashboardValidationMode::GrafanaImport,
        )
        .await
    }

    async fn create_with_mode(
        &self,
        org_id: Id,
        folder_id: Option<Id>,
        creator: Id,
        model: Value,
        validation_mode: DashboardValidationMode,
    ) -> Result<Dashboard> {
        let contracts = self.contracts.active().await?;
        let dashboard = build_dashboard(
            org_id,
            folder_id,
            creator,
            model,
            validation_mode,
            &contracts,
        )?;
        self.dashboards.create(dashboard).await
    }

    pub async fn create_from_draft(
        &self,
        org_id: Id,
        actor: Id,
        draft: DashboardDraft,
        expected_hash: String,
    ) -> Result<DraftConsumption> {
        let now = TimestampMicros::now();
        let contracts = self.contracts.active().await?;
        validate_executable_draft(&draft, &org_id, &actor, &expected_hash, now, &contracts)?;
        if let Some(folder_id) = &draft.folder_id {
            self.folders.get(&org_id, folder_id).await?;
        }
        let dashboard = build_dashboard(
            org_id.clone(),
            draft.folder_id.clone(),
            actor.clone(),
            draft.compiled_model.clone(),
            DashboardValidationMode::Native,
            &contracts,
        )?;
        let drafts = self
            .drafts
            .as_ref()
            .ok_or_else(|| Error::internal("Dashboard draft repository is not configured"))?;
        drafts
            .consume_and_create(ConsumeDashboardDraft {
                org_id,
                actor,
                draft_id: draft.id,
                expected_hash,
                compiler_version: contracts.manifest.compiler_version.clone(),
                now,
                dashboard,
            })
            .await
    }

    pub async fn validate_draft_model(&self, draft: &DashboardDraft) -> Result<()> {
        let contracts = self.contracts.active().await?;
        ensure_dashboard_model_with(
            &draft.compiled_model,
            DashboardValidationMode::Native,
            &contracts.model_validator,
            &contracts.manifest,
        )
    }

    pub fn draft_repository(&self) -> Option<&dyn DashboardDraftRepository> {
        self.drafts.as_deref()
    }

    pub fn model_metadata(model: &Value) -> Result<(String, String, Vec<String>)> {
        dashboard_model_metadata(model)
    }

    pub async fn get(&self, id: &Id) -> Result<Dashboard> {
        self.dashboards.get(id).await
    }

    pub async fn list(&self, org_id: &Id, folder_id: Option<&Id>) -> Result<Vec<Dashboard>> {
        self.dashboards.list(org_id, folder_id).await
    }

    pub async fn update_model(
        &self,
        mut dashboard: Dashboard,
        folder_id: Option<Id>,
        actor: Id,
        mut model: Value,
    ) -> Result<Dashboard> {
        let next_version = dashboard.version.saturating_add(1);
        apply_server_model_fields(
            &mut model,
            &dashboard.id,
            &dashboard.uid,
            next_version,
            folder_id.as_ref(),
        )?;
        let contracts = self.contracts.active().await?;
        ensure_dashboard_model_with(
            &model,
            DashboardValidationMode::Native,
            &contracts.model_validator,
            &contracts.manifest,
        )?;
        let (_, title, tags) = dashboard_model_metadata(&model)?;

        dashboard.folder_id = folder_id;
        dashboard.title = title;
        dashboard.tags = tags;
        dashboard.model = model;
        dashboard.version = next_version;
        dashboard.updated_at = TimestampMicros::now();
        dashboard.updated_by = actor;
        self.dashboards.update(dashboard).await
    }

    /// 按 id 存在与否决定 create / update（幂等覆盖）。跨集群事件 apply 用：
    /// 远端来的 Created/Updated 一律落地，本地无此 id 则插入、有则覆盖。
    pub async fn upsert(&self, d: Dashboard) -> Result<Dashboard> {
        match self.dashboards.get(&d.id).await {
            Ok(_) => self.dashboards.update(d).await,
            Err(Error::NotFound(_)) => self.dashboards.create(d).await,
            Err(e) => Err(e),
        }
    }

    pub async fn delete(&self, id: &Id) -> Result<()> {
        self.dashboards.delete(id).await
    }
}

fn build_dashboard(
    org_id: Id,
    folder_id: Option<Id>,
    creator: Id,
    mut model: Value,
    validation_mode: DashboardValidationMode,
    contracts: &ResolvedDashboardContracts,
) -> Result<Dashboard> {
    let now = TimestampMicros::now();
    let id = Id::new();
    let uid = model
        .get("uid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Id::new().0);
    apply_server_model_fields(&mut model, &id, &uid, 1, folder_id.as_ref())?;
    ensure_dashboard_model_with(
        &model,
        validation_mode,
        &contracts.model_validator,
        &contracts.manifest,
    )?;
    let (_, title, tags) = dashboard_model_metadata(&model)?;
    let dashboard = Dashboard {
        id,
        org_id,
        folder_id,
        uid,
        title,
        tags,
        model,
        version: 1,
        created_at: now,
        updated_at: now,
        created_by: creator.clone(),
        updated_by: creator,
    };
    Ok(dashboard)
}

fn validate_executable_draft(
    draft: &DashboardDraft,
    org_id: &Id,
    actor: &Id,
    expected_hash: &str,
    now: TimestampMicros,
    contracts: &ResolvedDashboardContracts,
) -> Result<()> {
    if &draft.org_id != org_id || &draft.created_by != actor {
        return Err(Error::not_found("dashboard draft not found"));
    }
    let manifest = &contracts.manifest;
    if !manifest
        .authoring_versions
        .contains(&draft.authoring_version)
        || draft.model_schema_version != manifest.dashboard_model_version
        || draft.compiler_version != manifest.compiler_version
        || !contracts.matches_pins(
            draft.contract_binding_revision,
            &draft.authoring_schema_hash,
            &draft.model_schema_hash,
            &draft.visualization_schema_hash,
        )
    {
        return Err(draft_validation_error(
            "DRAFT_STALE",
            "Dashboard draft is not compatible with the active compiler",
        ));
    }
    if draft.is_expired_at(now) {
        return Err(draft_validation_error(
            "DRAFT_EXPIRED",
            "Dashboard draft has expired; prepare it again",
        ));
    }
    if draft.model_hash != expected_hash {
        return Err(draft_validation_error(
            "DRAFT_HASH_MISMATCH",
            "Dashboard draft hash does not match the reviewed preview",
        ));
    }
    ensure_dashboard_model_with(
        &draft.compiled_model,
        DashboardValidationMode::Native,
        &contracts.model_validator,
        manifest,
    )
}

fn draft_validation_error(code: &str, message: &str) -> Error {
    Error::validation(
        "Dashboard draft cannot be executed",
        vec![crate::shared::contracts::ContractIssue::new(
            code,
            "/draft_id",
            message,
            true,
        )],
    )
}

fn apply_server_model_fields(
    model: &mut Value,
    id: &Id,
    uid: &str,
    version: u32,
    folder_id: Option<&Id>,
) -> Result<()> {
    let object = model.as_object_mut().ok_or_else(|| {
        Error::validation(
            "dashboard model is invalid",
            vec![crate::shared::contracts::ContractIssue::new(
                "CONTRACT_TYPE",
                "",
                "dashboard model must be an object",
                true,
            )],
        )
    })?;
    object.insert("id".to_string(), Value::String(id.0.clone()));
    object.insert("uid".to_string(), Value::String(uid.to_string()));
    object.insert("version".to_string(), Value::from(version));
    match folder_id {
        Some(folder_id) => {
            object.insert("folderId".to_string(), Value::String(folder_id.0.clone()));
        }
        None => {
            object.remove("folderId");
        }
    }
    Ok(())
}

fn dashboard_model_metadata(model: &Value) -> Result<(String, String, Vec<String>)> {
    let object = model
        .as_object()
        .ok_or_else(|| Error::invalid("dashboard model must be an object"))?;
    if object.get("engine").and_then(Value::as_str) != Some("molesignal-dashboard") {
        return Err(Error::invalid(
            "dashboard model engine must be molesignal-dashboard",
        ));
    }
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
        return Err(Error::invalid("unsupported dashboard schemaVersion"));
    }
    if !object.get("elements").is_some_and(Value::is_array) {
        return Err(Error::invalid("dashboard model elements must be an array"));
    }
    for key in ["tags", "variables", "annotations", "links"] {
        if !object.get(key).is_some_and(Value::is_array) {
            return Err(Error::invalid(format!(
                "dashboard model {key} must be an array"
            )));
        }
    }
    if !object.get("timeSettings").is_some_and(Value::is_object) {
        return Err(Error::invalid(
            "dashboard model timeSettings must be an object",
        ));
    }
    if !object.get("layout").is_some_and(Value::is_object) {
        return Err(Error::invalid("dashboard model layout must be an object"));
    }
    let refresh = object
        .get("refreshSettings")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::invalid("dashboard model refreshSettings must be an object"))?;
    if !matches!(
        refresh.get("mode").and_then(Value::as_str),
        Some("off" | "interval" | "live")
    ) {
        return Err(Error::invalid(
            "dashboard model refreshSettings.mode must be off, interval or live",
        ));
    }
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::validation(
                "dashboard model is invalid",
                vec![crate::shared::contracts::ContractIssue::new(
                    "EMPTY_DASHBOARD_TITLE",
                    "/title",
                    "dashboard model title must not be empty",
                    true,
                )],
            )
        })?
        .to_string();
    let uid = object
        .get("uid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Id::new().0);
    let tags = object
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((uid, title, tags))
}
