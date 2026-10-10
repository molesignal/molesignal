// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 后台循环单测共用的只读仓储：固定的组织与数据流列表。

use async_trait::async_trait;

use crate::{
    domain::{
        iam::{Organization, OrganizationRepository},
        stream::{Schema, StreamDefinition, StreamRepository, StreamType},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(crate) struct FixedOrganizations(pub(crate) Vec<Organization>);

pub(crate) struct FixedStreams(pub(crate) Vec<StreamDefinition>);

/// `organizations` 个组织（ID 为 `org-N`），每个组织 `streams_each` 个日志数据流
/// （ID 为 `org-N-stream-M`）。
pub(crate) fn tenants(
    organizations: usize,
    streams_each: usize,
) -> (FixedOrganizations, FixedStreams) {
    let now = TimestampMicros::now();
    let mut orgs = Vec::new();
    let mut streams = Vec::new();

    for org_index in 0..organizations {
        let org_id = Id::from_string(format!("org-{org_index}"));

        orgs.push(Organization {
            id: org_id.clone(),
            name: format!("Organization {org_index}"),
            slug: format!("org-{org_index}"),
            system: false,
            disabled: false,
            created_at: now,
        });

        for stream_index in 0..streams_each {
            streams.push(StreamDefinition {
                id: Id::from_string(format!("org-{org_index}-stream-{stream_index}")),
                org_id: org_id.clone(),
                name: format!("stream-{stream_index}"),
                stream_type: StreamType::LOGS,
                schema: Schema { fields: Vec::new() },
                retention: None,
                created_at: now,
                updated_at: now,
            });
        }
    }

    (FixedOrganizations(orgs), FixedStreams(streams))
}

fn read_only() -> Error {
    Error::internal("fixed test repository is read-only")
}

#[async_trait]
impl OrganizationRepository for FixedOrganizations {
    async fn create(&self, _org: Organization) -> Result<Organization> {
        Err(read_only())
    }

    async fn get(&self, id: &Id) -> Result<Organization> {
        self.0
            .iter()
            .find(|org| &org.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("organization"))
    }

    async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
        self.0
            .iter()
            .find(|org| org.slug == slug)
            .cloned()
            .ok_or_else(|| Error::not_found("organization"))
    }

    async fn list(&self) -> Result<Vec<Organization>> {
        Ok(self.0.clone())
    }

    async fn update_name(&self, _id: &Id, _name: String) -> Result<Organization> {
        Err(read_only())
    }

    async fn set_disabled(&self, _id: &Id, _disabled: bool) -> Result<Organization> {
        Err(read_only())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Err(read_only())
    }
}

#[async_trait]
impl StreamRepository for FixedStreams {
    async fn create(&self, _def: StreamDefinition) -> Result<StreamDefinition> {
        Err(read_only())
    }

    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Err(read_only())
    }

    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        self.0
            .iter()
            .find(|stream| {
                &stream.org_id == org_id && stream.name == name && stream.stream_type == stream_type
            })
            .cloned()
            .ok_or_else(|| Error::not_found("stream"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(self
            .0
            .iter()
            .filter(|stream| &stream.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Err(read_only())
    }
}
