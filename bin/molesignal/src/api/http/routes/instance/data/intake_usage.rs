// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 接入用量的平台设置：小时记录保留多久，一个组织每小时最多分别记下多少个服务。各节点每隔半分钟读
//! 一次服务数上限，改它的节点立即生效；保留天数在 compactor 主节点每天清理时现读。

use axum::{Extension, Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::audit;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{
        MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG, MAX_INTAKE_USAGE_RETENTION_DAYS,
        MIN_INTAKE_USAGE_RETENTION_DAYS, permission,
    },
    shared::{Error, Result, time::TimestampMicros},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeUsageSettings {
    pub retention_days: u32,
    pub max_services_per_org: u32,
}

fn validate(settings: &IntakeUsageSettings) -> Result<()> {
    if !(MIN_INTAKE_USAGE_RETENTION_DAYS..=MAX_INTAKE_USAGE_RETENTION_DAYS)
        .contains(&settings.retention_days)
    {
        return Err(Error::invalid(format!(
            "retention_days must be between {MIN_INTAKE_USAGE_RETENTION_DAYS} and {MAX_INTAKE_USAGE_RETENTION_DAYS}"
        )));
    }

    if !(1..=MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG).contains(&settings.max_services_per_org) {
        return Err(Error::invalid(format!(
            "max_services_per_org must be between 1 and {MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG}"
        )));
    }

    Ok(())
}

#[permission("sys.settings.limits.read")]
pub(super) async fn get(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<IntakeUsageSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(IntakeUsageSettings {
        retention_days: settings.intake_usage_retention_days,
        max_services_per_org: settings.intake_usage_max_services_per_org,
    }))
}

#[permission("sys.settings.limits.manage")]
pub(super) async fn put(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<IntakeUsageSettings>,
) -> Result<Json<IntakeUsageSettings>> {
    validate(&request)?;

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;

    let previous = (
        settings.intake_usage_retention_days,
        settings.intake_usage_max_services_per_org,
    );

    settings.intake_usage_retention_days = request.retention_days;
    settings.intake_usage_max_services_per_org = request.max_services_per_org;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    state
        .platform
        .intake_meter
        .set_max_services_per_org(updated.intake_usage_max_services_per_org as usize);

    if (request.retention_days, request.max_services_per_org) != previous {
        audit(
            &state,
            &ctx,
            "intake_usage_settings.update",
            json!({
                "retentionDays": updated.intake_usage_retention_days,
                "maxServicesPerOrg": updated.intake_usage_max_services_per_org,
                "previousRetentionDays": previous.0,
                "previousMaxServicesPerOrg": previous.1,
            }),
        )
        .await;
    }

    Ok(Json(IntakeUsageSettings {
        retention_days: updated.intake_usage_retention_days,
        max_services_per_org: updated.intake_usage_max_services_per_org,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(retention_days: u32, max_services_per_org: u32) -> IntakeUsageSettings {
        IntakeUsageSettings {
            retention_days,
            max_services_per_org,
        }
    }

    #[test]
    fn the_settings_keep_to_the_table_constraints() {
        assert!(validate(&settings(90, 1000)).is_ok());
        assert!(validate(&settings(MIN_INTAKE_USAGE_RETENTION_DAYS, 1)).is_ok());
        assert!(
            validate(&settings(
                MAX_INTAKE_USAGE_RETENTION_DAYS,
                MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG
            ))
            .is_ok()
        );

        // A shorter retention could not read back a month of budget.
        assert!(validate(&settings(MIN_INTAKE_USAGE_RETENTION_DAYS - 1, 1000)).is_err());
        assert!(validate(&settings(MAX_INTAKE_USAGE_RETENTION_DAYS + 1, 1000)).is_err());
        assert!(validate(&settings(90, 0)).is_err());
        assert!(validate(&settings(90, MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG + 1)).is_err());
    }
}
