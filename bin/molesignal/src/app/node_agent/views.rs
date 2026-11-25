// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The saved queries of the host events: OOM, network, hardware and every error. Each lists the
//! events with all their fields, so that the logs page they open in filters them further by host,
//! container, event and severity with a click on a value. Their ids are made of the organization,
//! so installing again only adds what is missing and never writes over a person's change.

use domain::{
    query::QueryLanguage,
    saved_view::{SavedView, SavedViewRepository},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};

use super::{EVENTS_STREAM, TAG};

/// A saved query of the host events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostView {
    pub key: &'static str,
    pub name: &'static str,
    /// The condition on the events, written as the logs page writes one.
    pub condition: &'static str,
}

pub const VIEWS: [HostView; 4] = [
    HostView {
        key: "oom",
        name: "主机 · OOM 事件",
        condition: r#""event.name" = 'oom'"#,
    },
    HostView {
        key: "network",
        name: "主机 · 网络事件",
        condition: r#""event.name" IN ('dropwatch', 'tcp_retransmit', 'netdev_events', 'net_rx_latency', 'netdev_txqueue_timeout', 'lacp')"#,
    },
    HostView {
        key: "hardware",
        name: "主机 · RAS 与内核卡顿",
        condition: r#""event.name" IN ('ras', 'hungtask', 'softlockup')"#,
    },
    HostView {
        key: "errors",
        name: "主机 · 错误事件",
        condition: "severity_text = 'ERROR'",
    },
];

/// How far back a view looks when it is opened: a day.
const TIME_RANGE_SECS: u32 = 86_400;

impl HostView {
    /// The id the view has in an organization.
    pub fn id(&self, org_id: &Id) -> Id {
        Id::from_string(format!("node-agent-view-{}-{}", self.key, org_id.0))
    }

    /// The newest events that meet the condition, with all their fields.
    pub fn statement(&self) -> String {
        format!(
            "SELECT * FROM \"{EVENTS_STREAM}\"\nWHERE {}\nORDER BY _timestamp DESC\nLIMIT 200",
            self.condition
        )
    }
}

/// The views the organization has, by key.
pub async fn installed(repo: &dyn SavedViewRepository, org: &Id) -> Result<Vec<&'static str>> {
    let mut keys = Vec::new();

    for view in VIEWS {
        match repo.get(org, &view.id(org)).await {
            Ok(_) => keys.push(view.key),
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(keys)
}

/// Saves the views the organization does not have yet, owned by `user`; the ones it has are left
/// as they are. Answers the ids of every view.
pub async fn install(repo: &dyn SavedViewRepository, org: &Id, user: &Id) -> Result<Vec<Id>> {
    let mut ids = Vec::new();

    for view in VIEWS {
        let id = view.id(org);

        match repo.get(org, &id).await {
            Ok(_) => {
                ids.push(id);
                continue;
            }
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        let now = TimestampMicros::now();

        let saved = SavedView {
            id: id.clone(),
            org_id: org.clone(),
            owner_user_id: user.clone(),
            name: view.name.into(),
            language: QueryLanguage::Sql,
            statement: view.statement(),
            time_range_secs: TIME_RANGE_SECS,
            stream: Some(EVENTS_STREAM.into()),
            tags: vec![TAG.into(), "host".into()],
            pinned: true,
            created_at: now,
            updated_at: now,
        };

        match repo.create(saved).await {
            Ok(_) => {}
            // Two installs at once converge on the same id.
            Err(Error::Conflict(_)) => {
                repo.get(org, &id).await?;
            }
            Err(error) => return Err(error),
        }

        ids.push(id);
    }

    Ok(ids)
}
