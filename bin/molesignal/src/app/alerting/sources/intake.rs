// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 接入外部告警：认出来源，按来源的类型读请求体，每条告警落到事件上，记下这次请求。
//!
//! - 来源的接收集群不是本集群时，请求原样转给接收集群（[`AlertForwarder`]），接收集群连不上时回
//!   503，发送方会重试。
//! - 同一个去重键上的告警一条一条处理（跨节点的锁），比这个键上次用到的发送方时间早的消息是晚到的
//!   旧消息，直接忽略。所以发送方重试、好几个节点同时收到同一个分组都不会重复开事件。

use serde_json::{Value, json};

use super::{AlertSources, ForwardedRequest, formats, token};
use crate::{
    app::alerting::rule_evaluator::external_fingerprint,
    domain::alerting::source::{
        AlertOutcome, AlertSource, ExternalAlert, IntakeAlert, IntakeRequest, IntakeStatus,
        MAX_BODY_EXCERPT_BYTES, MAX_RECORDED_ALERTS, SourceIncident,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 回给发送方的答复。
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeReply {
    pub status: u16,
    pub body: Value,
}

impl IntakeReply {
    fn error(error: &Error) -> Self {
        Self {
            status: error.http_status_code(),
            body: json!({"error": error.http_error_code(), "message": message_of(error)}),
        }
    }
}

/// 给人看的那句话，不带 `invalid argument:` 这样的前缀。
fn message_of(error: &Error) -> String {
    match error {
        Error::InvalidArgument(message)
        | Error::Conflict(message)
        | Error::Unavailable(message)
        | Error::Unauthorized(message)
        | Error::Forbidden(message)
        | Error::NotFound(message)
        | Error::PayloadTooLarge(message) => message.clone(),
        Error::Internal(_) | Error::Other(_) => "internal error".into(),
        other => other.to_string(),
    }
}

impl AlertSources {
    /// 按请求带的 Token 认出来源。Token 不对、来源不存在都一样回 401，不透露哪个来源存在。
    pub async fn authenticate(&self, presented: Option<&str>) -> Result<AlertSource> {
        let refused = || Error::unauthorized("invalid alert source token");
        let (prefix, secret) = presented.and_then(token::split).ok_or_else(refused)?;

        let source = self
            .sources
            .find_by_token_prefix(prefix)
            .await?
            .ok_or_else(refused)?;

        if !token::matches(secret, &source.token_hash) {
            return Err(refused());
        }

        Ok(source)
    }

    /// 本集群直接收到的一次请求：接收集群是本集群就处理，不是就转过去。
    pub async fn intake(
        &self,
        source: &AlertSource,
        content_type: Option<&str>,
        body: &[u8],
    ) -> IntakeReply {
        let local = match self.local_cluster().await {
            Ok(local) => local,
            Err(error) => {
                tracing::warn!(source_id = %source.id, %error, "read the federation cluster id");
                return IntakeReply::error(&Error::unavailable("try again in a moment"));
            }
        };

        if !source.opens_incidents_on(&local) {
            return self.forward(source, &local, content_type, body).await;
        }

        self.process(source, "", content_type, body).await
    }

    /// 请求到不了读内容那一步（SNS 的签名不对）：记下这次请求，按错误回答。
    pub async fn refuse(&self, source: &AlertSource, error: &Error, body: &[u8]) -> IntakeReply {
        let reply = IntakeReply::error(error);

        let mut record = blank_record(
            source,
            "",
            IntakeStatus::Rejected,
            reply.status,
            TimestampMicros::now(),
        );

        record.message = Some(message_of(error));
        record.body_excerpt = Some(excerpt(body));
        self.record(record).await;
        reply
    }

    /// 联邦里别的集群转来的一次请求。本集群不是来源的接收集群（来源的设置还在同步）时回 503，
    /// 不再往别处转。
    pub async fn intake_forwarded(
        &self,
        org_id: &Id,
        source_id: &Id,
        via_cluster: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> IntakeReply {
        let source = match self.get(org_id, source_id).await {
            Ok(source) => source,
            Err(error) => return IntakeReply::error(&error),
        };

        match self.local_cluster().await {
            Ok(local) if source.opens_incidents_on(&local) => {}
            Ok(_) => {
                return IntakeReply::error(&Error::unavailable(
                    "this cluster is not the receiving cluster of the alert source",
                ));
            }
            Err(error) => {
                tracing::warn!(%source_id, %error, "read the federation cluster id");
                return IntakeReply::error(&Error::unavailable("try again in a moment"));
            }
        }

        self.process(&source, via_cluster, content_type, body).await
    }

    async fn forward(
        &self,
        source: &AlertSource,
        local: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> IntakeReply {
        let request = ForwardedRequest {
            home_cluster: &source.home_cluster,
            local_cluster: local,
            org_id: &source.org_id,
            source_id: &source.id,
            content_type,
            body,
        };

        let forwarded = match &self.forwarder {
            Some(forwarder) => forwarder.forward(request).await,
            None => Err(Error::unavailable("this node cannot reach other clusters")),
        };

        let now = TimestampMicros::now();

        match forwarded {
            Ok(reply) => {
                let mut record =
                    blank_record(source, "", IntakeStatus::Forwarded, reply.status, now);
                record.message = Some(format!("forwarded to cluster {}", source.home_cluster));
                self.record(record).await;
                reply
            }
            Err(error) => {
                tracing::warn!(source_id = %source.id, home = %source.home_cluster, %error, "forward alerts");

                let error = Error::unavailable(format!(
                    "the receiving cluster {} of this alert source cannot be reached; retry later",
                    source.home_cluster
                ));

                let mut record = blank_record(source, "", IntakeStatus::Rejected, 503, now);
                record.message = Some(message_of(&error));
                self.record(record).await;
                IntakeReply::error(&error)
            }
        }
    }

    /// 在本集群处理：读请求体，逐条落到事件上，记下这次请求。
    pub(super) async fn process(
        &self,
        source: &AlertSource,
        via_cluster: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> IntakeReply {
        let now = TimestampMicros::now();

        if let Err(error) = self.sources.touch_intake(&source.id, now).await {
            tracing::warn!(source_id = %source.id, %error, "record the intake time of an alert source");
        }

        if !source.enabled {
            let error = Error::conflict("this alert source is turned off");
            let mut record = blank_record(source, via_cluster, IntakeStatus::Rejected, 409, now);
            record.message = Some(message_of(&error));
            self.record(record).await;
            return IntakeReply::error(&error);
        }

        let parsed = match formats::parse(source, content_type, body) {
            Ok(parsed) => parsed,
            Err(error) => {
                let mut record =
                    blank_record(source, via_cluster, IntakeStatus::Rejected, 400, now);
                record.message = Some(message_of(&error));
                record.body_excerpt = Some(excerpt(body));
                self.record(record).await;
                return IntakeReply::error(&error);
            }
        };

        let alert_count = parsed.alerts.len();
        let mut results = Vec::with_capacity(alert_count);
        let mut failure = None;

        for alert in parsed.alerts {
            match self.settle(source, alert, now).await {
                Ok(result) => results.push(result),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }

        let replies: Vec<Value> = results.iter().map(reply_of).collect();

        let (status, reply) = match failure {
            Some(error) => {
                tracing::warn!(source_id = %source.id, %error, "settle external alerts");
                let error = Error::unavailable("the alerts could not all be handled; retry");
                (IntakeStatus::Rejected, IntakeReply::error(&error))
            }
            None => (
                IntakeStatus::Accepted,
                IntakeReply {
                    status: 200,
                    body: json!({"alerts": replies, "message": parsed.message}),
                },
            ),
        };

        let mut record = blank_record(source, via_cluster, status, reply.status, now);
        record.alert_count = u32::try_from(alert_count).unwrap_or(u32::MAX);
        record.alerts = results.into_iter().take(MAX_RECORDED_ALERTS).collect();

        record.message = match status {
            IntakeStatus::Accepted => parsed.message,
            _ => reply.body["message"].as_str().map(str::to_string),
        };

        self.record(record).await;
        reply
    }

    /// 一条告警：在它的去重键上加锁，再落到事件上。
    async fn settle(
        &self,
        source: &AlertSource,
        alert: ExternalAlert,
        now: TimestampMicros,
    ) -> Result<IntakeAlert> {
        let fingerprint = external_fingerprint(&source.id, &alert.dedup_key);
        let lock = self
            .source_incidents
            .lock(&format!("{}|{fingerprint}", source.org_id))
            .await?;

        let settled = self.settle_locked(source, alert, &fingerprint, now).await;

        if let Err(error) = lock.release().await {
            tracing::warn!(source_id = %source.id, %error, "release the dedup key lock");
        }

        settled
    }

    async fn settle_locked(
        &self,
        source: &AlertSource,
        alert: ExternalAlert,
        fingerprint: &str,
        now: TimestampMicros,
    ) -> Result<IntakeAlert> {
        let latest = self
            .incidents
            .find_by_fingerprint(&source.org_id, fingerprint)
            .await?;

        let side = match &latest {
            Some(incident) => self.source_incidents.get(&incident.id).await?,
            None => None,
        };

        let last_event_at = side.as_ref().and_then(|state| state.last_event_at);

        // 晚到的旧消息：比这个去重键上次用到的发送方时间早。
        if let (Some(at), Some(last)) = (alert.occurred_at, last_event_at)
            && at < last
        {
            return Ok(IntakeAlert {
                action: alert.action,
                dedup_key: alert.dedup_key,
                summary: alert.summary,
                severity: alert.severity,
                outcome: AlertOutcome::Ignored,
                reason: Some("older than the last message of this alert".into()),
                incident_id: latest.map(|incident| incident.id),
            });
        }

        let latest_id = latest.as_ref().map(|incident| incident.id.clone());
        let settled = self
            .evaluator
            .settle_external(source, &alert, latest, now)
            .await?;

        let mut outcome = settled.outcome;
        let members = if source.kind.groups_alerts() {
            alert.members.clone()
        } else {
            Vec::new()
        };

        // 来源侧的状态记在告警落到的事件上；什么事件都没动时记在最新的那个上，晚到的旧消息还认得出。
        if let Some(incident_id) = settled.incident_id.clone().or(latest_id) {
            let same_incident = side
                .as_ref()
                .filter(|state| state.incident_id == incident_id);

            if outcome == AlertOutcome::Unchanged
                && same_incident.is_some_and(|state| state.members != members)
            {
                outcome = AlertOutcome::Updated;
            }

            self.source_incidents
                .upsert(SourceIncident {
                    incident_id,
                    org_id: source.org_id.clone(),
                    source_id: source.id.clone(),
                    dedup_key: alert.dedup_key.clone(),
                    last_event_at: alert.occurred_at,
                    members,
                    last_intake_at: now,
                })
                .await?;
        }

        Ok(IntakeAlert {
            action: alert.action,
            dedup_key: alert.dedup_key,
            summary: alert.summary,
            severity: alert.severity,
            outcome,
            reason: settled.reason,
            incident_id: settled.incident_id,
        })
    }

    /// 记下这次请求；记不下只记日志，不影响回给发送方的答复。
    pub(super) async fn record(&self, request: IntakeRequest) {
        let source_id = request.source_id.clone();

        if let Err(error) = self.requests.record(request).await {
            tracing::warn!(%source_id, %error, "record an alert intake request");
        }
    }
}

/// 一次请求的记录，结果之外的部分由调用方补上。
pub(super) fn blank_record(
    source: &AlertSource,
    via_cluster: &str,
    status: IntakeStatus,
    http_status: u16,
    received_at: TimestampMicros,
) -> IntakeRequest {
    IntakeRequest {
        id: Id::new(),
        org_id: source.org_id.clone(),
        source_id: source.id.clone(),
        received_at,
        via_cluster: via_cluster.to_string(),
        status,
        http_status,
        message: None,
        alert_count: 0,
        alerts: Vec::new(),
        body_excerpt: None,
    }
}

fn reply_of(alert: &IntakeAlert) -> Value {
    json!({
        "dedupKey": alert.dedup_key,
        "outcome": alert.outcome,
        "incidentId": alert.incident_id,
        "reason": alert.reason,
    })
}

/// 被拒的请求体开头，按字符边界截到 [`MAX_BODY_EXCERPT_BYTES`] 以内。
fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);

    if text.len() <= MAX_BODY_EXCERPT_BYTES {
        return text.into_owned();
    }

    let end = (0..=MAX_BODY_EXCERPT_BYTES)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);

    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reply_says_what_went_wrong_without_its_variant() {
        let reply = IntakeReply::error(&Error::invalid("the request body is not JSON"));

        assert_eq!(reply.status, 400);
        assert_eq!(reply.body["error"], "invalid_argument");
        assert_eq!(reply.body["message"], "the request body is not JSON");
        assert_eq!(
            IntakeReply::error(&Error::internal("db down")).body["message"],
            "internal error"
        );
    }

    #[test]
    fn a_long_body_is_cut_on_a_character_boundary() {
        let body = "磁".repeat(MAX_BODY_EXCERPT_BYTES);
        let cut = excerpt(body.as_bytes());

        assert!(cut.len() <= MAX_BODY_EXCERPT_BYTES);
        assert!(cut.chars().all(|c| c == '磁'));
        assert_eq!(excerpt(b"short"), "short");
    }
}
