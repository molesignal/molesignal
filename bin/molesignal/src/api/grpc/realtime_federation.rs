// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster.v1.EventService/RealtimeCounts` 的接收端：实时规则的汇总集群来拉本集群的计数。
//!
//! 和事件推送一样按 per-org token 鉴权、按 org 映射把汇总集群的组织换成本地组织，只答本地组织的
//! 实时规则。答的是本集群各节点的计数之和，不再往别的集群问，联邦里不会绕圈。
//!
//! 汇总集群每秒来拉、每次最多等 800 毫秒，所以常态下这里不查库：鉴权结果缓存 [`AUTH_TTL`]（Token
//! 撤销后这个只读的计数最多再答这么久），组织的实时规则过了 [`RULES_TTL`] 先用旧的答、后台重读。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use tonic::{Request, Response, Status};

use crate::{
    api::{AppState, http::middleware::auth::authenticate_api_token},
    domain::alerting::rule::AlertRuleKind,
    protocol::cluster::v1::{RealtimeCountsRequest, RealtimeCountsResponse, RealtimeRuleCount},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一次最多问几条规则。
const MAX_RULES: usize = 10_000;

/// 组织的实时规则多久重新读一次：汇总集群每秒来拉，不必每秒读一遍组织的全部规则。
const RULES_TTL: Duration = Duration::from_secs(10);

/// 一个 Token、来源集群和组织的鉴权结果用多久。
const AUTH_TTL: Duration = Duration::from_secs(30);

/// 读的时间和那时组织的实时规则 id。
type RuleIds = (Instant, Arc<HashSet<Id>>);

/// 鉴权的时间和映射到的本地组织。
type Allowed = (Instant, Id);

/// 接收端的缓存：鉴权结果和各组织的实时规则 id。
#[derive(Default)]
pub(super) struct RealtimeFederation {
    allowed: Mutex<HashMap<blake3::Hash, Allowed>>,
    rules: Arc<Mutex<HashMap<Id, RuleIds>>>,
    /// 正在后台重读的组织：同一个组织只读一份。
    refreshing: Arc<Mutex<HashSet<Id>>>,
}

impl RealtimeFederation {
    /// 来拉的是哪个本地组织：鉴过的直接用，否则按 per-org token 和组织映射鉴权。
    async fn local_org(
        &self,
        state: &AppState,
        bearer: &str,
        source_cluster: &str,
        remote_org: &str,
    ) -> std::result::Result<Id, Status> {
        let key = blake3::hash(format!("{bearer}\n{source_cluster}\n{remote_org}").as_bytes());

        if let Some((checked_at, org)) = self.allowed.lock().get(&key)
            && checked_at.elapsed() < AUTH_TTL
        {
            return Ok(org.clone());
        }

        let token_org = authenticate_api_token(
            bearer,
            state.iam.service.as_ref(),
            state.iam.api_tokens.clone(),
        )
        .await
        .map_err(|error| Status::unauthenticated(error.to_string()))?
        .org_id;

        // 汇总集群的组织 → 本地组织；没映射的组织不答。
        let link = state
            .cluster
            .org_link
            .get_for_remote(&Id(source_cluster.to_string()), &Id(remote_org.to_string()))
            .await
            .map_err(|error| Status::unavailable(error.to_string()))?
            .ok_or_else(|| Status::permission_denied("the organization is not mapped"))?;

        if token_org != link.local_org_id {
            return Err(Status::permission_denied(
                "the token belongs to another organization",
            ));
        }

        self.allowed
            .lock()
            .insert(key, (Instant::now(), link.local_org_id.clone()));

        Ok(link.local_org_id)
    }

    /// 组织的实时规则 id。读过的过了期先照旧用，后台重读；没读过的现读。
    async fn rule_ids(&self, state: &AppState, org_id: &Id) -> Result<Arc<HashSet<Id>>> {
        let cached = self.rules.lock().get(org_id).cloned();

        match cached {
            Some((read_at, ids)) if read_at.elapsed() < RULES_TTL => Ok(ids),
            Some((_, ids)) => {
                self.refresh(state, org_id);
                Ok(ids)
            }
            None => {
                let ids = read_rule_ids(state, org_id).await?;

                self.rules
                    .lock()
                    .insert(org_id.clone(), (Instant::now(), ids.clone()));

                Ok(ids)
            }
        }
    }

    fn refresh(&self, state: &AppState, org_id: &Id) {
        if !self.refreshing.lock().insert(org_id.clone()) {
            return;
        }

        let (state, org_id) = (state.clone(), org_id.clone());
        let (rules, refreshing) = (self.rules.clone(), self.refreshing.clone());

        tokio::spawn(async move {
            match read_rule_ids(&state, &org_id).await {
                Ok(ids) => {
                    rules.lock().insert(org_id.clone(), (Instant::now(), ids));
                }
                Err(error) => tracing::debug!(%error, "real-time rules could not be read again"),
            }

            refreshing.lock().remove(&org_id);
        });
    }
}

async fn read_rule_ids(state: &AppState, org_id: &Id) -> Result<Arc<HashSet<Id>>> {
    let ids: HashSet<Id> = state
        .alerting
        .service
        .list_rules(org_id)
        .await?
        .into_iter()
        .filter(|rule| rule.kind == AlertRuleKind::RealTime)
        .map(|rule| rule.id)
        .collect();

    Ok(Arc::new(ids))
}

pub(super) async fn realtime_counts(
    state: &AppState,
    federation: &RealtimeFederation,
    request: Request<RealtimeCountsRequest>,
) -> std::result::Result<Response<RealtimeCountsResponse>, Status> {
    let bearer = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_string))
        .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;

    let request = request.into_inner();

    if request.rule_ids.len() > MAX_RULES {
        return Err(Status::invalid_argument("too many rules in one request"));
    }

    let local_org = federation
        .local_org(state, &bearer, &request.source_cluster_id, &request.org_id)
        .await?;

    let own = federation
        .rule_ids(state, &local_org)
        .await
        .map_err(|error| Status::unavailable(error.to_string()))?;

    let rule_ids: Vec<Id> = request
        .rule_ids
        .into_iter()
        .map(Id)
        .filter(|id| own.contains(id))
        .collect();

    if rule_ids.is_empty() {
        return Ok(Response::new(RealtimeCountsResponse { counts: Vec::new() }));
    }

    let counts = state
        .alerting
        .realtime_counts
        .counts(&rule_ids, TimestampMicros::now())
        .await
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(id, count)| RealtimeRuleCount {
            rule_id: id.0,
            count,
        })
        .collect();

    Ok(Response::new(RealtimeCountsResponse { counts }))
}
