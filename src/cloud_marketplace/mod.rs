// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloud marketplace subscriptions（spec cloud-marketplace，付费版独占）。
//!
//! 提供：
//! - AWS / Azure marketplace 通知解析（signature 校验由调用方注入）
//! - 订阅状态机 `pending → active → suspended → cancelled`
//! - 内存 metering 累计器（hourly aggregator 由调用方 tick）
//!
//! 真正的 AWS Marketplace Metering API 调用（`MeterUsage`）/ Azure usageEvent
//! 由 OSS 注入 `MarketplaceClient` impl；本 crate 不直接 link AWS / Azure SDK。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub mod stripe;

pub const MARKETPLACE_FEATURE: &str = "cloud_marketplace";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionState {
    Pending,
    Active,
    Suspended,
    Cancelled,
}

impl SubscriptionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "suspended" => Self::Suspended,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    /// 状态机：允许的转移。`Cancelled` 是终态。
    pub fn can_transition_to(self, next: SubscriptionState) -> bool {
        use SubscriptionState::*;
        matches!(
            (self, next),
            (Pending, Active)
                | (Pending, Cancelled)
                | (Active, Suspended)
                | (Active, Cancelled)
                | (Suspended, Active)
                | (Suspended, Cancelled)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marketplace {
    Aws,
    Azure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Id,
    pub org_id: Id,
    pub marketplace: Marketplace,
    pub customer_id: String,
    pub product_id: String,
    pub state: SubscriptionState,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    async fn upsert(&self, s: Subscription) -> Result<Subscription>;
    async fn get_by_customer(
        &self,
        marketplace: Marketplace,
        customer_id: &str,
    ) -> Result<Option<Subscription>>;
    async fn list_active(&self) -> Result<Vec<Subscription>>;
}

/// AWS SNS 通知 payload（spec 仅核心字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsNotification {
    pub action: String, // subscribe-success / unsubscribe-pending / subscribe-fail
    pub customer_identifier: String,
    pub product_code: String,
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// Azure marketplace webhook payload（核心字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureNotification {
    pub action: String, // Reinstate / Unsubscribe / Suspend / ...
    pub subscription_id: String,
    pub plan_id: String,
    #[serde(default)]
    pub time_stamp: Option<String>,
}

/// 计费门禁判定：给定某 org 的全部订阅状态，是否应拦截写入（402）。
///
/// 规则：org 有订阅记录、但**没有任何一条处于可服务态**（[`SubscriptionState::Active`]
/// 或 [`SubscriptionState::Pending`]）→ 拦截。无任何订阅记录 → 不拦截（未接入计量，
/// 由 license 兜底）。Pending 视为可服务，避免在开户/首次付款过程中误拦。
pub fn subscriptions_block(states: impl IntoIterator<Item = SubscriptionState>) -> bool {
    let mut any = false;
    let mut serviceable = false;
    for s in states {
        any = true;
        if matches!(s, SubscriptionState::Active | SubscriptionState::Pending) {
            serviceable = true;
        }
    }
    any && !serviceable
}

/// AWS action → SubscriptionState 映射。
pub fn aws_action_to_state(action: &str) -> Result<SubscriptionState> {
    match action {
        "subscribe-success" => Ok(SubscriptionState::Active),
        "subscribe-fail" => Ok(SubscriptionState::Cancelled),
        "unsubscribe-pending" => Ok(SubscriptionState::Suspended),
        "unsubscribe-success" => Ok(SubscriptionState::Cancelled),
        other => Err(Error::invalid(format!(
            "unknown AWS marketplace action: {other}"
        ))),
    }
}

/// Azure action → SubscriptionState 映射。
pub fn azure_action_to_state(action: &str) -> Result<SubscriptionState> {
    match action {
        "Reinstate" | "ChangeQuantity" => Ok(SubscriptionState::Active),
        "Suspend" => Ok(SubscriptionState::Suspended),
        "Unsubscribe" => Ok(SubscriptionState::Cancelled),
        other => Err(Error::invalid(format!(
            "unknown Azure marketplace action: {other}"
        ))),
    }
}

/// 通用 metering 上报客户端（OSS 注入 reqwest 实现）。
#[async_trait]
pub trait MarketplaceClient: Send + Sync {
    /// AWS `MeterUsage(dimension, quantity)` 或 Azure usageEvent。
    async fn report_usage(
        &self,
        marketplace: Marketplace,
        customer_id: &str,
        dimension: &str,
        quantity: i64,
    ) -> Result<()>;
}

/// 进程内 metering 聚合器：累计 (subscription_id, dimension) → quantity。
/// hourly aggregator tick 时 `drain()` 一次性上报。
#[derive(Default)]
pub struct MeteringAggregator {
    inner: Arc<Mutex<HashMap<(String, String), i64>>>,
}

impl MeteringAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, subscription_id: &str, dimension: &str, qty: i64) {
        let mut g = self.inner.lock();
        let key = (subscription_id.to_string(), dimension.to_string());
        *g.entry(key).or_insert(0) += qty;
    }

    /// 取出所有累计并清空（hourly tick 调用）。
    pub fn drain(&self) -> Vec<(String, String, i64)> {
        let mut g = self.inner.lock();
        let out = g
            .iter()
            .map(|(k, v)| (k.0.clone(), k.1.clone(), *v))
            .collect();
        g.clear();
        out
    }

    pub fn pending_count(&self) -> usize {
        self.inner.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_transitions_legal() {
        use SubscriptionState::*;
        assert!(Pending.can_transition_to(Active));
        assert!(Pending.can_transition_to(Cancelled));
        assert!(Active.can_transition_to(Suspended));
        assert!(Active.can_transition_to(Cancelled));
        assert!(Suspended.can_transition_to(Active));
        assert!(!Cancelled.can_transition_to(Active)); // 终态
        assert!(!Pending.can_transition_to(Suspended)); // 必须先 Active
    }

    #[test]
    fn aws_action_mapping() {
        assert_eq!(
            aws_action_to_state("subscribe-success").unwrap(),
            SubscriptionState::Active
        );
        assert_eq!(
            aws_action_to_state("unsubscribe-success").unwrap(),
            SubscriptionState::Cancelled
        );
        assert!(aws_action_to_state("garbage").is_err());
    }

    #[test]
    fn azure_action_mapping() {
        assert_eq!(
            azure_action_to_state("Reinstate").unwrap(),
            SubscriptionState::Active
        );
        assert_eq!(
            azure_action_to_state("Suspend").unwrap(),
            SubscriptionState::Suspended
        );
        assert_eq!(
            azure_action_to_state("Unsubscribe").unwrap(),
            SubscriptionState::Cancelled
        );
    }

    #[test]
    fn subscriptions_block_only_when_no_serviceable() {
        use SubscriptionState::*;
        // 无订阅 → 不拦截。
        assert!(!subscriptions_block(std::iter::empty()));
        // 有 active → 放行。
        assert!(!subscriptions_block([Cancelled, Active]));
        // pending 视为可服务。
        assert!(!subscriptions_block([Pending]));
        // 全部停服/取消 → 拦截。
        assert!(subscriptions_block([Suspended]));
        assert!(subscriptions_block([Cancelled, Suspended]));
    }

    #[test]
    fn metering_aggregator_accumulates_and_drains() {
        let agg = MeteringAggregator::new();
        agg.record("sub-1", "ingest_gib", 5);
        agg.record("sub-1", "ingest_gib", 3);
        agg.record("sub-1", "query_count", 100);
        agg.record("sub-2", "ingest_gib", 10);

        let mut drained = agg.drain();
        drained.sort();
        assert_eq!(
            drained,
            vec![
                ("sub-1".into(), "ingest_gib".into(), 8),
                ("sub-1".into(), "query_count".into(), 100),
                ("sub-2".into(), "ingest_gib".into(), 10),
            ]
        );
        assert_eq!(agg.pending_count(), 0); // drained
    }
}
