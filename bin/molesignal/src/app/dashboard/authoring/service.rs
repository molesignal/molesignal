// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};

use super::{DashboardAuthoringCompiler, compiler::CompiledDashboard};
use crate::{
    app::dashboard::{
        DashboardService,
        contract_registry::{
            DashboardContractResolver, ResolvedDashboardContracts,
            builtin_dashboard_contract_resolver,
        },
        validation::{DashboardValidationMode, ensure_dashboard_model_with},
    },
    domain::dashboard::authoring::{
        AuthoringElement, DashboardAuthoringCapabilities, DashboardAuthoringSpec, DashboardDraft,
        DashboardDraftRepository, DashboardDraftStatus, DashboardQueryPreflight, DraftConsumption,
        PreflightReport, PreflightWarningRecord, PreparedDashboardDraft, SectionElement,
        VisualizationManifest,
    },
    shared::{Error, Result, contracts::ContractIssue, ids::Id, time::TimestampMicros},
};

const DEFAULT_DRAFT_TTL: Duration = Duration::from_secs(30 * 60);
const MIN_DRAFT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_DRAFT_TTL: Duration = Duration::from_secs(2 * 60 * 60);

pub struct DashboardAuthoringService {
    drafts: Arc<dyn DashboardDraftRepository>,
    preflight: Arc<dyn DashboardQueryPreflight>,
    dashboard: Arc<DashboardService>,
    contracts: Arc<dyn DashboardContractResolver>,
    draft_ttl: Duration,
}

struct ReadyDraftInput {
    org_id: Id,
    actor: Id,
    folder_id: Option<Id>,
    spec: DashboardAuthoringSpec,
    compiled: CompiledDashboard,
    preflight: PreflightReport,
}

impl DashboardAuthoringService {
    pub fn new(
        drafts: Arc<dyn DashboardDraftRepository>,
        preflight: Arc<dyn DashboardQueryPreflight>,
        dashboard: Arc<DashboardService>,
    ) -> Self {
        Self {
            drafts,
            preflight,
            dashboard,
            contracts: builtin_dashboard_contract_resolver(),
            draft_ttl: DEFAULT_DRAFT_TTL,
        }
    }

    pub fn with_contract_resolver(mut self, contracts: Arc<dyn DashboardContractResolver>) -> Self {
        self.contracts = contracts;
        self
    }

    pub fn with_draft_ttl(mut self, ttl: Duration) -> Self {
        self.draft_ttl = ttl.clamp(MIN_DRAFT_TTL, MAX_DRAFT_TTL);
        self
    }

    pub async fn capabilities(&self) -> Result<DashboardAuthoringCapabilities> {
        let contracts = self.contracts.active().await?;
        let manifest = &contracts.manifest;
        Ok(DashboardAuthoringCapabilities {
            authoring_versions: manifest.authoring_versions.clone(),
            dashboard_model_version: manifest.dashboard_model_version,
            compiler_version: manifest.compiler_version.clone(),
            query_kinds: manifest.query_kinds.clone(),
            visualizations: manifest.visualizations.clone(),
            units: manifest.units.clone(),
            reducers: manifest.reducers.clone(),
            limits: serde_json::to_value(&manifest.limits)
                .expect("Dashboard visualization limits must serialize"),
            workflow: vec![
                "get_dashboard_capabilities".into(),
                "prepare_dashboard".into(),
                "preview_dashboard_draft".into(),
                "propose_dashboard_creation".into(),
                "confirm_or_approve_create_dashboard".into(),
            ],
        })
    }

    pub async fn prepare(
        &self,
        org_id: Id,
        actor: Id,
        input: Value,
    ) -> Result<PreparedDashboardDraft> {
        let contracts = self.contracts.active().await?;
        ensure_supported_authoring_version(&input, &contracts.manifest)?;
        let contract_issues = contracts.authoring_validator.validate(&input);
        if !contract_issues.is_empty() {
            return Err(Error::validation(
                "dashboard authoring specification is invalid",
                contract_issues,
            ));
        }
        let spec: DashboardAuthoringSpec = serde_json::from_value(input).map_err(|error| {
            Error::validation(
                "dashboard authoring specification is invalid",
                vec![ContractIssue::new(
                    "CONTRACT_DESERIALIZATION",
                    "",
                    error.to_string(),
                    true,
                )],
            )
        })?;
        let folder_id = spec.folder_id.as_deref().map(Id::from_string);
        if let Some(folder_id) = &folder_id {
            self.dashboard.folders().get(&org_id, folder_id).await?;
        }
        let compiled = DashboardAuthoringCompiler::compile_with_contracts(
            &spec,
            &contracts.manifest,
            &contracts.model_validator,
        )?;
        let preflight = self.preflight.preflight(&org_id, &actor, &spec).await?;
        if !preflight.issues.is_empty() {
            return Err(Error::validation(
                "dashboard query preflight failed",
                preflight.issues,
            ));
        }
        self.persist_ready_draft(
            ReadyDraftInput {
                org_id,
                actor,
                folder_id,
                spec,
                compiled,
                preflight,
            },
            &contracts,
        )
        .await
    }

    pub async fn get_draft(
        &self,
        org_id: &Id,
        actor: &Id,
        draft_id: &Id,
    ) -> Result<DashboardDraft> {
        let now = TimestampMicros::now();
        let draft = self.drafts.get(org_id, draft_id, now).await?;
        if &draft.created_by != actor {
            return Err(Error::not_found("dashboard draft not found"));
        }
        let contracts = self.contracts.active().await?;
        validate_current_draft(&draft, now, &contracts)?;
        Ok(draft)
    }

    /// Loads the persisted preview for an API caller whose creator/reviewer permission
    /// has already been checked at the protocol boundary. Expired drafts remain readable
    /// so the UI can explain why creation is disabled.
    pub async fn get_draft_for_preview(
        &self,
        org_id: &Id,
        draft_id: &Id,
    ) -> Result<DashboardDraft> {
        let draft = self
            .drafts
            .get(org_id, draft_id, TimestampMicros::now())
            .await?;
        let contracts = self.contracts.active().await?;
        validate_draft_contracts(&draft, &contracts)?;
        Ok(draft)
    }

    pub async fn validate_reference(
        &self,
        org_id: &Id,
        actor: &Id,
        draft_id: &Id,
        expected_hash: &str,
    ) -> Result<DashboardDraft> {
        let draft = self.get_draft(org_id, actor, draft_id).await?;
        if draft.model_hash != expected_hash {
            return Err(draft_error(
                "DRAFT_HASH_MISMATCH",
                "Dashboard draft hash does not match the reviewed preview",
            ));
        }
        Ok(draft)
    }

    pub async fn create_from_draft(
        &self,
        org_id: Id,
        actor: Id,
        draft_id: Id,
        expected_hash: String,
    ) -> Result<DraftConsumption> {
        let draft = self
            .validate_reference(&org_id, &actor, &draft_id, &expected_hash)
            .await?;
        self.dashboard
            .create_from_draft(org_id, actor, draft, expected_hash)
            .await
    }

    async fn persist_ready_draft(
        &self,
        input: ReadyDraftInput,
        contracts: &ResolvedDashboardContracts,
    ) -> Result<PreparedDashboardDraft> {
        let ReadyDraftInput {
            org_id,
            actor,
            folder_id,
            spec,
            compiled,
            preflight,
        } = input;
        let now = TimestampMicros::now();
        let ttl_micros = i64::try_from(self.draft_ttl.as_micros()).unwrap_or(i64::MAX);
        let expires_at = TimestampMicros(now.0.saturating_add(ttl_micros));
        let normalized_spec =
            serde_json::to_value(&spec).map_err(|error| Error::internal(error.to_string()))?;
        let warnings = preflight
            .warnings
            .iter()
            .map(|warning| PreflightWarningRecord {
                code: warning.code.clone(),
                path: warning.path.clone(),
                message: warning.message.clone(),
            })
            .collect::<Vec<_>>();
        let draft = DashboardDraft {
            id: Id::new(),
            org_id,
            created_by: actor,
            authoring_version: spec.authoring_version,
            model_schema_version: compiled.dashboard_model_version,
            compiler_version: compiled.compiler_version,
            contract_binding_revision: contracts.binding.revision,
            authoring_schema_hash: contracts.binding.selection.authoring.schema_hash.clone(),
            model_schema_hash: contracts.binding.selection.model.schema_hash.clone(),
            visualization_schema_hash: contracts
                .binding
                .selection
                .visualization
                .schema_hash
                .clone(),
            authoring_spec: normalized_spec,
            compiled_model: compiled.model,
            model_hash: compiled.model_hash,
            folder_id,
            status: DashboardDraftStatus::Ready,
            dashboard_id: None,
            warnings,
            preflight,
            created_at: now,
            expires_at,
            consumed_at: None,
        };
        let draft = self.drafts.create(draft).await?;
        Ok(PreparedDashboardDraft {
            draft_id: draft.id.clone(),
            model_hash: draft.model_hash.clone(),
            expires_at: draft.expires_at,
            summary: draft_summary(&spec),
            warnings: draft.warnings.clone(),
            issues: Vec::new(),
            preview_route: format!("/ai/dashboard-drafts/{}", draft.id.0),
        })
    }
}

fn ensure_supported_authoring_version(
    input: &Value,
    manifest: &VisualizationManifest,
) -> Result<()> {
    let version = input
        .get("authoringVersion")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    if version.is_none_or(|version| !manifest.authoring_versions.contains(&version)) {
        return Err(Error::validation(
            "unsupported Dashboard authoring contract version",
            vec![ContractIssue::new(
                "UNSUPPORTED_AUTHORING_VERSION",
                "/authoringVersion",
                format!(
                    "supported authoring versions: {:?}",
                    manifest.authoring_versions
                ),
                true,
            )],
        ));
    }
    Ok(())
}

fn validate_current_draft(
    draft: &DashboardDraft,
    now: TimestampMicros,
    contracts: &ResolvedDashboardContracts,
) -> Result<()> {
    if draft.is_expired_at(now) {
        return Err(draft_error(
            "DRAFT_EXPIRED",
            "Dashboard draft has expired; prepare it again",
        ));
    }
    validate_draft_contracts(draft, contracts)
}

fn validate_draft_contracts(
    draft: &DashboardDraft,
    contracts: &ResolvedDashboardContracts,
) -> Result<()> {
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
        return Err(draft_error(
            "DRAFT_STALE",
            "Dashboard draft was compiled by an incompatible contract revision",
        ));
    }
    let actual_hash = contracts
        .model_validator
        .canonical_hash(&draft.compiled_model);
    if actual_hash != draft.model_hash {
        return Err(draft_error(
            "DRAFT_HASH_MISMATCH",
            "Dashboard draft content no longer matches its integrity hash",
        ));
    }
    ensure_dashboard_model_with(
        &draft.compiled_model,
        DashboardValidationMode::Native,
        &contracts.model_validator,
        manifest,
    )
}

fn draft_error(code: &str, message: &str) -> Error {
    Error::validation(
        "Dashboard draft is not executable",
        vec![ContractIssue::new(code, "/draft_id", message, true)],
    )
}

fn draft_summary(spec: &DashboardAuthoringSpec) -> Value {
    let mut panels = 0usize;
    let mut text_blocks = 0usize;
    let mut queries = 0usize;
    for element in &spec.elements {
        match element {
            AuthoringElement::Panel(panel) => {
                panels += 1;
                queries += panel.queries.len();
            }
            AuthoringElement::Text(_) => text_blocks += 1,
            AuthoringElement::Section(section) => {
                for child in &section.elements {
                    match child {
                        SectionElement::Panel(panel) => {
                            panels += 1;
                            queries += panel.queries.len();
                        }
                        SectionElement::Text(_) => text_blocks += 1,
                    }
                }
            }
        }
    }
    json!({
        "title": spec.title,
        "panels": panels,
        "text_blocks": text_blocks,
        "queries": queries,
        "folder_id": spec.folder_id
    })
}

#[cfg(test)]
mod tests;
