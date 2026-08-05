// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Persisted License 装载与开发环境回退策略。

use std::sync::Arc;

use super::core::Core;
use crate::{
    config::Settings,
    domain::license::{LicenseVersion, LicenseVersionRepository},
    license::{DEFAULT_ROOT_PUBKEY, LicenseFile, SignedLicense, load_file_from_env},
    shared::{CommunityLicense, Error, LicenseGate, LicenseHolder, ids::Id, time::TimestampMicros},
};

pub(super) struct LicenseRuntime {
    pub(super) license: Arc<dyn LicenseGate>,
    pub(super) holder: Arc<LicenseHolder>,
    pub(super) loaded: bool,
}

impl LicenseRuntime {
    pub(super) async fn build(settings: &Settings, core: &Core) -> Self {
        let (initial, loaded) = build_license(
            core.license_versions.as_ref(),
            &core.system_org.id,
            settings,
        )
        .await;
        crate::shared::trace_metrics::set_system_load("license", loaded);
        let holder = Arc::new(LicenseHolder::new(initial));
        let license: Arc<dyn LicenseGate> = holder.clone();
        Self {
            license,
            holder,
            loaded,
        }
    }
}

/// Dev-only license：通过 `MS_DEV_UNLOCK_FEATURES` 在本地无签名 license 时放开 feature gate。
///
/// 值约定：`1` / `all` / `*` 放开**全部** feature；否则按逗号分隔的 feature 名放开
/// （如 `intelligence` 或 `intelligence,sso`）。**生产环境请勿设置此 env。**
struct DevLicense {
    all: bool,
    features: Vec<String>,
}

impl LicenseGate for DevLicense {
    fn has_feature(&self, name: &str) -> bool {
        self.all || self.features.iter().any(|feature| feature == name)
    }

    fn add_ingest_bytes(&self, _bytes: u64) -> bool {
        true
    }

    fn expired(&self, _now_micros: i64) -> bool {
        false
    }

    fn issued_to(&self) -> &str {
        "dev-unlocked"
    }

    fn reset_daily(&self) {}

    fn features(&self) -> Vec<String> {
        if self.all {
            vec!["*".into()]
        } else {
            self.features.clone()
        }
    }

    fn edition(&self) -> &'static str {
        "dev"
    }
}

/// 构造当前进程使用的 License gate，并报告持久化 License 是否健康加载。
pub(super) async fn build_license(
    versions: &dyn LicenseVersionRepository,
    system_org_id: &Id,
    settings: &Settings,
) -> (Arc<dyn LicenseGate>, bool) {
    let now = TimestampMicros::now();
    match versions.active().await {
        Ok(Some(active)) => {
            let verified =
                serde_json::from_value::<LicenseFile>(active.version.signed_package.clone())
                    .map_err(|error| Error::invalid(format!("persisted License package: {error}")))
                    .and_then(|file| {
                        SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, now.0)
                    });
            match verified {
                Ok(license) => {
                    tracing::info!(
                        version_id = %active.version.id.0,
                        "active persisted License verified"
                    );
                    return (Arc::new(license), true);
                }
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        version_id = %active.version.id.0,
                        "active persisted License invalid; degrading to Community"
                    );
                    if settings.license.disaster_fallback_from_environment
                        && let Ok(Some(file)) = load_file_from_env()
                        && let Ok(license) =
                            SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, now.0)
                    {
                        tracing::error!(
                            "using explicitly enabled License disaster fallback from environment"
                        );
                        return (Arc::new(license), false);
                    }
                    return (Arc::new(CommunityLicense::new()), false);
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(
                error = %error,
                "failed to load active persisted License; degrading to Community"
            );
            if settings.license.disaster_fallback_from_environment
                && let Ok(Some(file)) = load_file_from_env()
                && let Ok(license) =
                    SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, now.0)
            {
                return (Arc::new(license), false);
            }
            return (Arc::new(CommunityLicense::new()), false);
        }
    }

    if settings.license.bootstrap_from_environment {
        match load_file_from_env() {
            Ok(Some(file)) => {
                match SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, now.0) {
                    Ok(license) => {
                        let package = serde_json::to_value(&file).unwrap_or_default();
                        let digest =
                            blake3::hash(&serde_json::to_vec(&package).unwrap_or_default())
                                .to_hex()
                                .to_string();
                        let version = LicenseVersion {
                            id: Id::new(),
                            system_org_id: system_org_id.clone(),
                            signed_package: package,
                            payload_digest: digest,
                            summary: serde_json::json!({
                                "expires_at_micros": license.expires_at_micros(),
                                "feature_count": license.features().len(),
                            }),
                            created_by: None,
                            created_at: now,
                        };
                        match versions.insert_and_activate(version, None).await {
                            Ok(active) => {
                                tracing::info!(
                                    version_id = %active.version.id.0,
                                    "bootstrapped persisted License from environment"
                                );
                                return (Arc::new(license), true);
                            }
                            Err(error) => {
                                tracing::error!(
                                    error = %error,
                                    "failed to persist bootstrap License; degrading to Community"
                                );
                                return (Arc::new(CommunityLicense::new()), false);
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            error = %error,
                            "bootstrap License verification failed; degrading to Community"
                        );
                        return (Arc::new(CommunityLicense::new()), false);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!(
                    error = %error,
                    "bootstrap License source unreadable; degrading to Community"
                );
                return (Arc::new(CommunityLicense::new()), false);
            }
        }
    }

    if let Ok(spec) = std::env::var("MS_DEV_UNLOCK_FEATURES") {
        let spec = spec.trim();
        if !spec.is_empty() {
            let all = matches!(spec, "1" | "all" | "*");
            let features = if all {
                Vec::new()
            } else {
                spec.split(',')
                    .map(str::trim)
                    .filter(|feature| !feature.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            };
            tracing::warn!(
                spec = %spec,
                "MS_DEV_UNLOCK_FEATURES set — bypassing license gate (DEV ONLY, do not use in production)"
            );
            return (Arc::new(DevLicense { all, features }), true);
        }
    }

    tracing::info!("no license loaded; falling back to community");
    (Arc::new(CommunityLicense::new()), true)
}
