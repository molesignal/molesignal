// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    call::{CallLine, QueuedCall},
    connector::{ConnectorTestStatus, NotifyConnector},
    delivery::{DeliveryClaim, DeliveryCompletion, DeliveryFilter, NotifyDelivery, PageRecord},
    endpoint::UserNotifyEndpoint,
    event::{NotifyEventClaim, NotifyEventRecord, NotifyEventStatus},
    follow_up::NotifyFollowUp,
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
    /// 只凭 ID 找连接器：服务商的回调（短信回复、电话按键）不带组织，由回调自己的凭据鉴权。
    async fn find(&self, id: &Id) -> Result<Option<NotifyConnector>> {
        let _ = id;
        Ok(None)
    }
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
    /// 事件被确认：送达的投递记为已确认，排在电话线路上还没拨的也记为已确认，不再拨。
    async fn acknowledge_event(
        &self,
        organization_id: &Id,
        event_id: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64>;
    /// 同上，对 ID 以 `event_id_prefix` 开头的全部事件（一个告警事件的各次升级）。
    async fn acknowledge_event_prefix(
        &self,
        organization_id: &Id,
        event_id_prefix: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64>;
    /// 一个告警事件的全部通知投递：触发、各次升级与直接通知、确认、解决，按时间先后，最多 `limit`
    /// 条。只给事件的时间线用；测试替身可以不实现。
    async fn list_for_alert(
        &self,
        _organization_id: &Id,
        _incident_id: &Id,
        _limit: u32,
    ) -> Result<Vec<NotifyDelivery>> {
        Ok(Vec::new())
    }
    /// 这段时间里送到成员手上的告警通知，每个成员每个通知一条，按时间先后：值班负担用。测试替身
    /// 可以不实现。
    async fn pages_between(
        &self,
        _organization_id: &Id,
        _from: TimestampMicros,
        _to: TimestampMicros,
    ) -> Result<Vec<PageRecord>> {
        Ok(Vec::new())
    }
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
pub trait NotifyFollowUpRepository: Send + Sync {
    /// 排一个后续步骤。同一事件、同一策略、同一个人、同一等待时间的已经排过时什么也不做。
    async fn schedule(&self, follow_up: NotifyFollowUp) -> Result<()>;
    /// 领取到期的后续步骤；领取后别的节点拿不到，领取后太久没处理完的会被重新领取。
    async fn claim_due(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyFollowUp>>;
    async fn finish(&self, follow_up: &NotifyFollowUp) -> Result<()>;
    /// 把领到的后续步骤放回去，到 `due_at` 再领。
    async fn postpone(&self, follow_up: &NotifyFollowUp, due_at: TimestampMicros) -> Result<()>;
    /// 事件被确认：取消它还没到期的后续步骤。
    async fn cancel(&self, organization_id: &Id, event_id: &str) -> Result<u64>;
    /// 同上，对 ID 以 `event_id_prefix` 开头的全部事件。
    async fn cancel_prefix(&self, organization_id: &Id, event_id_prefix: &str) -> Result<u64>;
}

#[async_trait]
pub trait NotifyCallRepository: Send + Sync {
    /// 线路在窗口里没拨过就占下它、记下这次拨打，返回上次拨打的时刻（从没拨过是 0）；占不到返回 `None`。
    async fn take_line(
        &self,
        line: &CallLine,
        now: TimestampMicros,
    ) -> Result<Option<TimestampMicros>>;
    /// 占下线路却没有要拨的：放回上次拨打的时刻，下一条通知不必等窗口结束。只在线路仍是 `taken_at`
    /// 那次占下时生效。
    async fn release_line(
        &self,
        line: &CallLine,
        taken_at: TimestampMicros,
        previous: TimestampMicros,
    ) -> Result<()>;
    /// 线路占着时，把通知排在线路上。
    async fn enqueue(&self, call: QueuedCall) -> Result<()>;
    /// 有通知在排、窗口已经过去的线路。
    async fn due_lines(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<CallLine>>;
    /// 取走线路上排着的全部通知，按排队先后。
    async fn drain(&self, line: &CallLine) -> Result<Vec<QueuedCall>>;
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
    /// 把领到的事件放回去等到 `next_attempt_at`，这次领取不算一次尝试；`note` 写明它在等什么。
    async fn postpone(
        &self,
        organization_id: &Id,
        id: &str,
        next_attempt_at: TimestampMicros,
        note: &str,
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
    /// 把从内置策略复制来的策略恢复成那条内置策略的样子。
    async fn restore_builtin(&self, organization_id: &Id, id: &Id) -> Result<NotifyPolicy>;
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
    /// 组织的全部模板，按名称排。
    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyTemplate>>;
}
