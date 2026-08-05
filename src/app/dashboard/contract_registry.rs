// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Runtime publication and resolution of the DB-backed Dashboard contract bundle.

use std::{
    collections::HashSet,
    sync::{Arc, LazyLock},
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::{
    domain::dashboard::{
        authoring::{VisualizationManifest, visualization_manifest},
        contract_registry::{
            DASHBOARD_AUTHORING_CAPABILITY, DASHBOARD_AUTHORING_CONTRACT, DASHBOARD_MODEL_CONTRACT,
            DASHBOARD_VISUALIZATION_CONTRACT, DashboardContractBinding, DashboardContractBundle,
            DashboardContractDocuments, DashboardContractKind, DashboardContractRef,
            DashboardContractRepository, DashboardContractSelection, DashboardContractStatus,
            DashboardContractVersion, JSON_SCHEMA_2020_12_DIALECT, VISUALIZATION_MANIFEST_DIALECT,
        },
    },
    shared::{
        Error, Result,
        contracts::{
            ContractValidator, DASHBOARD_AUTHORING_V1_SCHEMA, DASHBOARD_MODEL_V2_SCHEMA,
            DASHBOARD_VISUALIZATIONS_V1, canonical_json_bytes, sha256_hex,
        },
        time::TimestampMicros,
    },
};

pub struct ResolvedDashboardContracts {
    pub binding: DashboardContractBinding,
    pub model_validator: Arc<ContractValidator>,
    pub authoring_validator: Arc<ContractValidator>,
    pub manifest: Arc<VisualizationManifest>,
}

impl ResolvedDashboardContracts {
    #[must_use]
    pub fn matches_pins(
        &self,
        binding_revision: i64,
        authoring_schema_hash: &str,
        model_schema_hash: &str,
        visualization_schema_hash: &str,
    ) -> bool {
        let selection = &self.binding.selection;
        self.binding.revision == binding_revision
            && selection.authoring.schema_hash == authoring_schema_hash
            && selection.model.schema_hash == model_schema_hash
            && selection.visualization.schema_hash == visualization_schema_hash
    }
}

#[async_trait]
pub trait DashboardContractResolver: Send + Sync {
    async fn active(&self) -> Result<Arc<ResolvedDashboardContracts>>;
}

pub struct DashboardContractRegistryService {
    repository: Arc<dyn DashboardContractRepository>,
    cache: RwLock<Option<Arc<ResolvedDashboardContracts>>>,
}

impl DashboardContractRegistryService {
    pub fn new(repository: Arc<dyn DashboardContractRepository>) -> Self {
        Self {
            repository,
            cache: RwLock::new(None),
        }
    }

    /// Publishes the deployed canonical files without replacing an existing binding.
    pub async fn publish_builtins(&self) -> Result<Arc<ResolvedDashboardContracts>> {
        let now = TimestampMicros::now();
        let (versions, selection) = builtin_publication(now)?;
        self.repository
            .publish_builtin(&versions, &selection, now)
            .await?;
        *self.cache.write().await = None;
        self.active().await
    }

    /// Activates a fully validated selection. Only trusted internal callers receive this service.
    pub async fn activate(
        &self,
        selection: DashboardContractSelection,
    ) -> Result<Arc<ResolvedDashboardContracts>> {
        let documents = self.repository.load_documents(&selection).await?;
        let candidate = DashboardContractBinding {
            selection: selection.clone(),
            revision: 1,
            updated_at: TimestampMicros::now(),
        };
        resolve_bundle(
            DashboardContractBundle {
                binding: candidate,
                documents,
            },
            false,
        )?;
        self.repository
            .activate(&selection, TimestampMicros::now())
            .await?;
        *self.cache.write().await = None;
        self.active().await
    }
}

#[async_trait]
impl DashboardContractResolver for DashboardContractRegistryService {
    async fn active(&self) -> Result<Arc<ResolvedDashboardContracts>> {
        let bundle = self
            .repository
            .load_active(DASHBOARD_AUTHORING_CAPABILITY)
            .await?;
        if let Some(cached) = self.cache.read().await.as_ref()
            && cached.binding == bundle.binding
        {
            return Ok(Arc::clone(cached));
        }
        let resolved = Arc::new(resolve_bundle(bundle, true)?);
        *self.cache.write().await = Some(Arc::clone(&resolved));
        Ok(resolved)
    }
}

struct BuiltinDashboardContractResolver;

#[async_trait]
impl DashboardContractResolver for BuiltinDashboardContractResolver {
    async fn active(&self) -> Result<Arc<ResolvedDashboardContracts>> {
        Ok(Arc::clone(&BUILTIN_CONTRACTS))
    }
}

#[must_use]
pub fn builtin_dashboard_contract_resolver() -> Arc<dyn DashboardContractResolver> {
    Arc::new(BuiltinDashboardContractResolver)
}

static BUILTIN_CONTRACTS: LazyLock<Arc<ResolvedDashboardContracts>> = LazyLock::new(|| {
    let now = TimestampMicros(0);
    let (versions, selection) =
        builtin_publication(now).expect("built-in Dashboard contracts must be valid");
    let documents = documents_from_versions(&versions)
        .expect("built-in Dashboard contract kinds must be complete");
    Arc::new(
        resolve_bundle(
            DashboardContractBundle {
                binding: DashboardContractBinding {
                    selection,
                    revision: 1,
                    updated_at: now,
                },
                documents,
            },
            true,
        )
        .expect("built-in Dashboard contract bundle must resolve"),
    )
});

fn builtin_publication(
    published_at: TimestampMicros,
) -> Result<(Vec<DashboardContractVersion>, DashboardContractSelection)> {
    let model_document: Value = serde_json::from_str(DASHBOARD_MODEL_V2_SCHEMA)
        .map_err(|error| Error::internal(format!("built-in Dashboard model schema: {error}")))?;
    let authoring_document: Value =
        serde_json::from_str(DASHBOARD_AUTHORING_V1_SCHEMA).map_err(|error| {
            Error::internal(format!("built-in Dashboard authoring schema: {error}"))
        })?;
    let visualization_document: Value = serde_json::from_str(DASHBOARD_VISUALIZATIONS_V1)
        .map_err(|error| Error::internal(format!("built-in visualization manifest: {error}")))?;
    let manifest = VisualizationManifest::from_value(visualization_document.clone())
        .map_err(|error| Error::internal(format!("built-in visualization manifest: {error}")))?;
    let versions = vec![
        contract_version(
            DASHBOARD_MODEL_CONTRACT,
            manifest.dashboard_model_version,
            DashboardContractKind::DashboardModel,
            JSON_SCHEMA_2020_12_DIALECT,
            model_document,
            published_at,
        ),
        contract_version(
            DASHBOARD_AUTHORING_CONTRACT,
            1,
            DashboardContractKind::DashboardAuthoring,
            JSON_SCHEMA_2020_12_DIALECT,
            authoring_document,
            published_at,
        ),
        contract_version(
            DASHBOARD_VISUALIZATION_CONTRACT,
            manifest.manifest_version,
            DashboardContractKind::VisualizationManifest,
            VISUALIZATION_MANIFEST_DIALECT,
            visualization_document,
            published_at,
        ),
    ];
    let documents = documents_from_versions(&versions)?;
    let selection = DashboardContractSelection {
        capability_key: DASHBOARD_AUTHORING_CAPABILITY.into(),
        model: documents.model.reference(),
        authoring: documents.authoring.reference(),
        visualization: documents.visualization.reference(),
        compiler_version: manifest.compiler_version,
        enabled: true,
    };
    Ok((versions, selection))
}

fn contract_version(
    key: &str,
    version: u32,
    kind: DashboardContractKind,
    dialect: &str,
    document: Value,
    published_at: TimestampMicros,
) -> DashboardContractVersion {
    DashboardContractVersion {
        contract_key: key.into(),
        version,
        kind,
        dialect: dialect.into(),
        schema_hash: sha256_hex(canonical_json_bytes(&document)),
        document,
        status: DashboardContractStatus::Published,
        published_at,
    }
}

fn documents_from_versions(
    versions: &[DashboardContractVersion],
) -> Result<DashboardContractDocuments> {
    let find = |kind| {
        versions
            .iter()
            .find(|version| version.kind == kind)
            .cloned()
            .ok_or_else(|| Error::internal("built-in Dashboard contract kind is missing"))
    };
    Ok(DashboardContractDocuments {
        model: find(DashboardContractKind::DashboardModel)?,
        authoring: find(DashboardContractKind::DashboardAuthoring)?,
        visualization: find(DashboardContractKind::VisualizationManifest)?,
    })
}

fn resolve_bundle(
    bundle: DashboardContractBundle,
    require_enabled: bool,
) -> Result<ResolvedDashboardContracts> {
    let selection = &bundle.binding.selection;
    if selection.capability_key != DASHBOARD_AUTHORING_CAPABILITY {
        return Err(registry_error("unexpected Dashboard capability binding"));
    }
    if require_enabled && !selection.enabled {
        return Err(registry_error(
            "Dashboard authoring contract binding is disabled",
        ));
    }
    validate_document(
        &bundle.documents.model,
        &selection.model,
        DashboardContractKind::DashboardModel,
        JSON_SCHEMA_2020_12_DIALECT,
    )?;
    validate_document(
        &bundle.documents.authoring,
        &selection.authoring,
        DashboardContractKind::DashboardAuthoring,
        JSON_SCHEMA_2020_12_DIALECT,
    )?;
    validate_document(
        &bundle.documents.visualization,
        &selection.visualization,
        DashboardContractKind::VisualizationManifest,
        VISUALIZATION_MANIFEST_DIALECT,
    )?;
    let model_validator = Arc::new(
        ContractValidator::compile(bundle.documents.model.document.clone())
            .map_err(|error| registry_error(format!("model schema cannot compile: {error}")))?,
    );
    let authoring_validator = Arc::new(
        ContractValidator::compile(bundle.documents.authoring.document.clone())
            .map_err(|error| registry_error(format!("authoring schema cannot compile: {error}")))?,
    );
    let manifest = Arc::new(
        VisualizationManifest::from_value(bundle.documents.visualization.document)
            .map_err(registry_error)?,
    );
    validate_compatibility(
        selection,
        &bundle.documents.model.document,
        &bundle.documents.authoring.document,
        &manifest,
    )?;
    Ok(ResolvedDashboardContracts {
        binding: bundle.binding,
        model_validator,
        authoring_validator,
        manifest,
    })
}

fn validate_document(
    document: &DashboardContractVersion,
    reference: &DashboardContractRef,
    expected_kind: DashboardContractKind,
    expected_dialect: &str,
) -> Result<()> {
    let actual_hash = sha256_hex(canonical_json_bytes(&document.document));
    if document.contract_key != reference.contract_key
        || document.version != reference.version
        || document.schema_hash != reference.schema_hash
        || actual_hash != document.schema_hash
        || document.kind != expected_kind
        || document.dialect != expected_dialect
        || document.status != DashboardContractStatus::Published
    {
        return Err(registry_error(format!(
            "invalid published contract reference for {}",
            reference.contract_key
        )));
    }
    Ok(())
}

fn validate_compatibility(
    selection: &DashboardContractSelection,
    model_schema: &Value,
    authoring_schema: &Value,
    manifest: &VisualizationManifest,
) -> Result<()> {
    let built_in = visualization_manifest();
    if selection.compiler_version != manifest.compiler_version
        || manifest.compiler_version != built_in.compiler_version
        || schema_version(model_schema, "schemaVersion") != Some(manifest.dashboard_model_version)
        || schema_version(authoring_schema, "authoringVersion")
            .is_none_or(|version| !manifest.authoring_versions.contains(&version))
    {
        return Err(registry_error(
            "Dashboard contract bundle is incompatible with the running compiler",
        ));
    }
    let supported_queries = built_in.query_kinds.iter().collect::<HashSet<_>>();
    let supported_units = built_in.units.iter().collect::<HashSet<_>>();
    let supported_reducers = built_in.reducers.iter().collect::<HashSet<_>>();
    let supported_visualizations = built_in
        .visualizations
        .iter()
        .map(|capability| {
            (
                &capability.visualization_type,
                capability.option_schema_version,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let compatible = manifest
        .query_kinds
        .iter()
        .all(|value| supported_queries.contains(value))
        && manifest
            .units
            .iter()
            .all(|value| supported_units.contains(value))
        && manifest
            .reducers
            .iter()
            .all(|value| supported_reducers.contains(value))
        && manifest.visualizations.iter().all(|capability| {
            supported_visualizations.get(&capability.visualization_type)
                == Some(&capability.option_schema_version)
        });
    if !compatible {
        return Err(registry_error(
            "Dashboard contract bundle advertises unsupported compiler capabilities",
        ));
    }
    Ok(())
}

fn schema_version(schema: &Value, property: &str) -> Option<u32> {
    schema
        .get("properties")?
        .get(property)?
        .get("const")?
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
}

fn registry_error(message: impl Into<String>) -> Error {
    Error::unavailable(format!("Dashboard contract registry: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use parking_lot::RwLock as SyncRwLock;
    use serde_json::json;

    use super::*;

    struct MemoryRepository {
        bundle: SyncRwLock<DashboardContractBundle>,
    }

    impl MemoryRepository {
        fn new(bundle: DashboardContractBundle) -> Self {
            Self {
                bundle: SyncRwLock::new(bundle),
            }
        }

        fn replace(&self, bundle: DashboardContractBundle) {
            *self.bundle.write() = bundle;
        }
    }

    #[async_trait]
    impl DashboardContractRepository for MemoryRepository {
        async fn publish_builtin(
            &self,
            _versions: &[DashboardContractVersion],
            _default_selection: &DashboardContractSelection,
            _now: TimestampMicros,
        ) -> Result<DashboardContractBinding> {
            Ok(self.bundle.read().binding.clone())
        }

        async fn load_active(&self, _capability_key: &str) -> Result<DashboardContractBundle> {
            Ok(self.bundle.read().clone())
        }

        async fn load_documents(
            &self,
            _selection: &DashboardContractSelection,
        ) -> Result<DashboardContractDocuments> {
            Ok(self.bundle.read().documents.clone())
        }

        async fn activate(
            &self,
            selection: &DashboardContractSelection,
            now: TimestampMicros,
        ) -> Result<DashboardContractBinding> {
            let mut bundle = self.bundle.write();
            bundle.binding.selection = selection.clone();
            bundle.binding.revision += 1;
            bundle.binding.updated_at = now;
            Ok(bundle.binding.clone())
        }
    }

    fn builtin_bundle() -> DashboardContractBundle {
        let now = TimestampMicros(7);
        let (versions, selection) = builtin_publication(now).unwrap();
        DashboardContractBundle {
            binding: DashboardContractBinding {
                selection,
                revision: 1,
                updated_at: now,
            },
            documents: documents_from_versions(&versions).unwrap(),
        }
    }

    fn repin(version: &mut DashboardContractVersion, reference: &mut DashboardContractRef) {
        version.schema_hash = sha256_hex(canonical_json_bytes(&version.document));
        reference.schema_hash.clone_from(&version.schema_hash);
    }

    #[test]
    fn canonical_publication_contains_all_hashed_contracts() {
        let (versions, selection) = builtin_publication(TimestampMicros(10)).unwrap();
        assert_eq!(versions.len(), 3);
        assert!(versions.iter().all(|version| {
            version.status == DashboardContractStatus::Published
                && version.schema_hash == sha256_hex(canonical_json_bytes(&version.document))
        }));
        assert_eq!(selection.capability_key, DASHBOARD_AUTHORING_CAPABILITY);
        assert!(selection.enabled);
        resolve_bundle(
            DashboardContractBundle {
                binding: DashboardContractBinding {
                    selection,
                    revision: 1,
                    updated_at: TimestampMicros(10),
                },
                documents: documents_from_versions(&versions).unwrap(),
            },
            true,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn cache_is_reused_only_for_the_same_active_revision() {
        let initial = builtin_bundle();
        let repository = Arc::new(MemoryRepository::new(initial.clone()));
        let service = DashboardContractRegistryService::new(repository.clone());
        let first = service.active().await.unwrap();
        let second = service.active().await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        let mut replacement = initial;
        replacement.binding.revision = 2;
        replacement.binding.updated_at = TimestampMicros(8);
        repository.replace(replacement);
        let third = service.active().await.unwrap();
        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(third.binding.revision, 2);
    }

    #[test]
    fn malformed_hash_mismatch_disabled_and_unsupported_bundles_fail_closed() {
        let mut hash_mismatch = builtin_bundle();
        hash_mismatch.documents.model.document["title"] = json!("changed");
        assert!(resolve_bundle(hash_mismatch, true).is_err());

        let mut malformed = builtin_bundle();
        malformed.documents.authoring.document = json!({"type": "object"});
        repin(
            &mut malformed.documents.authoring,
            &mut malformed.binding.selection.authoring,
        );
        assert!(resolve_bundle(malformed, true).is_err());

        let mut disabled = builtin_bundle();
        disabled.binding.selection.enabled = false;
        assert!(resolve_bundle(disabled, true).is_err());

        let mut unsupported = builtin_bundle();
        unsupported.documents.visualization.document["compilerVersion"] =
            json!("dashboard-authoring-unsupported");
        unsupported.binding.selection.compiler_version = "dashboard-authoring-unsupported".into();
        repin(
            &mut unsupported.documents.visualization,
            &mut unsupported.binding.selection.visualization,
        );
        assert!(resolve_bundle(unsupported, true).is_err());
    }
}
