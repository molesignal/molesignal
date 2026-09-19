// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    connector::{ConnectorTestStatus, NotifyConnector},
    delivery::{DeliveryClaim, DeliveryCompletion, DeliveryFilter, NotifyDelivery},
    endpoint::UserNotifyEndpoint,
    event::{NotifyEventClaim, NotifyEventRecord, NotifyEventStatus},
    policy::NotifyPolicy,
    preference::{NotifyCategory, UserNotifyPreference},
    routing::{OrganizationNotifyDefault, TeamNotifyDefault},
    template::NotifyTemplate,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait NotifyConnectorRepository: Send + Sync {
    async fn create(&self, connector: NotifyConnector) -> Result<NotifyConnector>;
    async fn update(&self, connector: NotifyConnector) -> Result<NotifyConnector>;
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyConnector>;
    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyConnector>>;
    async fn record_test_result(
        &self,
        organization_id: &Id,
        id: &Id,
        tested_at: TimestampMicros,
        status: ConnectorTestStatus,
        error: Option<String>,
    ) -> Result<NotifyConnector>;
    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait UserNotifyEndpointRepository: Send + Sync {
    async fn create(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint>;
    async fn update(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint>;
    async fn get(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<UserNotifyEndpoint>;
    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyEndpoint>>;
    async fn list_for_organization(&self, organization_id: &Id) -> Result<Vec<UserNotifyEndpoint>>;
    async fn count_for_connector(&self, organization_id: &Id, connector_id: &Id) -> Result<u64>;
    async fn delete(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait UserNotifyPreferenceRepository: Send + Sync {
    async fn get(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<UserNotifyPreference>>;
    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyPreference>>;
    async fn list_for_organization(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<UserNotifyPreference>>;
    async fn upsert(&self, preference: UserNotifyPreference) -> Result<UserNotifyPreference>;
}

#[async_trait]
pub trait NotifyDeliveryRepository: Send + Sync {
    /// 幂等插入；同一个 `idempotency_key` 已存在时返回已有记录。
    async fn record_once(&self, delivery: NotifyDelivery) -> Result<NotifyDelivery>;
    /// 原子抢占一次发送。已成功或正在发送的幂等键返回 `acquired=false`；
    /// 失败或跳过的记录会递增 attempt 后重新抢占。
    async fn claim(&self, delivery: NotifyDelivery) -> Result<DeliveryClaim>;
    async fn complete(
        &self,
        organization_id: &Id,
        id: &Id,
        completion: DeliveryCompletion,
    ) -> Result<NotifyDelivery>;
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyDelivery>;
    async fn find_by_idempotency_key(
        &self,
        organization_id: &Id,
        idempotency_key: &str,
    ) -> Result<Option<NotifyDelivery>>;
    async fn list(
        &self,
        organization_id: &Id,
        filter: &DeliveryFilter,
    ) -> Result<Vec<NotifyDelivery>>;
    async fn acknowledge_event(
        &self,
        organization_id: &Id,
        event_id: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64>;
    async fn list_due_ack(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyDelivery>>;
    async fn mark_escalated(
        &self,
        organization_id: &Id,
        id: &Id,
        escalated_at: TimestampMicros,
    ) -> Result<NotifyDelivery>;
}

#[async_trait]
pub trait NotifyEventRepository: Send + Sync {
    /// 幂等写入事件；重复业务事件保留首次消息和发生时间。
    async fn enqueue(&self, record: NotifyEventRecord) -> Result<NotifyEventRecord>;
    async fn get(&self, organization_id: &Id, id: &str) -> Result<NotifyEventRecord>;
    async fn claim(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim>;
    /// Operator-triggered retry. Claims any non-processing record immediately;
    /// a stale processing claim may also be recovered.
    async fn claim_retry(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim>;
    async fn claim_pending(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyEventRecord>>;
    async fn finish(
        &self,
        organization_id: &Id,
        id: &str,
        status: NotifyEventStatus,
        next_attempt_at: TimestampMicros,
        error: Option<String>,
        now: TimestampMicros,
    ) -> Result<NotifyEventRecord>;
}

#[async_trait]
pub trait NotifyPolicyRepository: Send + Sync {
    async fn create(&self, policy: NotifyPolicy) -> Result<NotifyPolicy>;
    async fn update(&self, policy: NotifyPolicy) -> Result<NotifyPolicy>;
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyPolicy>;
    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyPolicy>>;
    async fn list_enabled_for_event(
        &self,
        organization_id: &Id,
        event_type: &str,
    ) -> Result<Vec<NotifyPolicy>>;
    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait TeamNotifyDefaultRepository: Send + Sync {
    async fn get(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<TeamNotifyDefault>>;
    async fn list(&self, organization_id: &Id, team_id: &Id) -> Result<Vec<TeamNotifyDefault>>;
    async fn upsert(&self, route: TeamNotifyDefault) -> Result<TeamNotifyDefault>;
    async fn delete(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<()>;
}

#[async_trait]
pub trait OrganizationNotifyDefaultRepository: Send + Sync {
    async fn get(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<OrganizationNotifyDefault>>;
    async fn list(&self, organization_id: &Id) -> Result<Vec<OrganizationNotifyDefault>>;
    async fn upsert(&self, route: OrganizationNotifyDefault) -> Result<OrganizationNotifyDefault>;
    async fn delete(&self, organization_id: &Id, category: NotifyCategory) -> Result<()>;
}

#[async_trait]
pub trait NotifyRouteReferenceRepository: Send + Sync {
    async fn count_for_connector(&self, organization_id: &Id, connector_id: &Id) -> Result<u64>;
}

#[async_trait]
pub trait NotifyTemplateRepository: Send + Sync {
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyTemplate>;
}
