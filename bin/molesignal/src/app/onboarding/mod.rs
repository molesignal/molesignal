// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped starter queries; stable identities make retries safe.

use domain::{
    query::QueryLanguage,
    saved_view::{SavedView, SavedViewRepository},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};

const QUERIES: [(&str, &str, &str); 4] = [
    (
        "errors",
        "样例 · 错误日志",
        "SELECT * FROM sample_app_logs WHERE level = 'error'",
    ),
    (
        "payment",
        "样例 · 支付服务",
        "SELECT * FROM sample_app_logs WHERE service = 'payment'",
    ),
    (
        "correlation",
        "样例 · 日志关联链路",
        "SELECT trace_id, service, level, message FROM sample_app_logs WHERE trace_id IS NOT NULL ORDER BY trace_id, service",
    ),
    (
        "services",
        "样例 · 服务日志分布",
        "SELECT service, level, COUNT(*) AS count FROM sample_app_logs GROUP BY service, level",
    ),
];

fn view_id(org_id: &Id, key: &str) -> Id {
    Id::from_string(format!("sample-view-{key}-{}", org_id.0))
}

pub async fn views_ready(repo: &dyn SavedViewRepository, org: &Id) -> Result<bool> {
    for (key, _, _) in QUERIES {
        match repo.get(org, &view_id(org, key)).await {
            Ok(_) => {}
            Err(Error::NotFound(_)) => return Ok(false),
            Err(error) => return Err(error),
        }
    }

    Ok(true)
}

pub async fn install_views(repo: &dyn SavedViewRepository, org: &Id, user: &Id) -> Result<Vec<Id>> {
    let mut ids = Vec::new();

    for (key, name, statement) in QUERIES {
        let id = view_id(org, key);

        match repo.get(org, &id).await {
            Ok(_) => {
                ids.push(id);
                continue;
            }
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        let now = TimestampMicros::now();
        let view = SavedView {
            id: id.clone(),
            org_id: org.clone(),
            owner_user_id: user.clone(),
            name: name.into(),
            language: QueryLanguage::Sql,
            statement: statement.into(),
            time_range_secs: 10800,
            stream: Some("sample_app_logs".into()),
            tags: vec!["sample".into(), "onboarding-v1".into()],
            pinned: true,
            created_at: now,
            updated_at: now,
        };

        match repo.create(view).await {
            Ok(_) => {}
            Err(Error::Conflict(_)) => {
                repo.get(org, &id).await?;
            }
            Err(error) => return Err(error),
        }

        ids.push(id);
    }

    Ok(ids)
}

#[cfg(test)]
mod tests;
