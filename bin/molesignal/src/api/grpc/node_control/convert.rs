// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `molesignal.node.control.v1` messages to and from the control plane's values.

use domain::node_agent::{
    Capabilities, CollectorState, ConfigState, Health, HealthState, LeaseKey, RejectReason,
    TaskStatus,
};
use kernel::{Error, ids::Id, time::TimestampMicros};
use protocol::node_control::v1::{self as wire, control_message::Payload};
use tonic::Status;

use crate::app::node_agent::Outbound;

/// Points in time travel as Unix seconds.
pub(super) fn seconds(at: TimestampMicros) -> u64 {
    u64::try_from(at.0.div_euclid(1_000_000)).unwrap_or_default()
}

pub(super) fn capabilities(value: Option<wire::Capabilities>) -> Capabilities {
    let value = value.unwrap_or_default();
    Capabilities::from_declared(&value.task_kinds, value.features, value.max_running_tasks)
}

pub(super) fn lease_key(task_id: String, attempt: u32) -> LeaseKey {
    LeaseKey {
        task_id: Id::from_string(task_id),
        attempt,
    }
}

pub(super) fn lease_keys(values: Vec<wire::LeaseKey>) -> Vec<LeaseKey> {
    values
        .into_iter()
        .map(|key| lease_key(key.task_id, key.attempt))
        .collect()
}

pub(super) fn health(value: Option<wire::Health>) -> Health {
    let value = value.unwrap_or_default();

    let state = match wire::HealthState::try_from(value.state) {
        Ok(wire::HealthState::Healthy) => HealthState::Healthy,
        Ok(wire::HealthState::Degraded) => HealthState::Degraded,
        _ => HealthState::Unknown,
    };

    Health {
        state,
        messages: value
            .messages
            .into_iter()
            .take(16)
            .map(|message| message.chars().take(512).collect())
            .collect(),
    }
}

pub(super) fn collector_states(values: Vec<wire::CollectorState>) -> Vec<CollectorState> {
    values
        .into_iter()
        .map(|value| CollectorState {
            name: value.name.chars().take(128).collect(),
            state: value.state.chars().take(32).collect(),
            message: value.message.chars().take(512).collect(),
        })
        .collect()
}

pub(super) fn task_status(value: i32) -> Result<TaskStatus, Status> {
    match wire::TaskStatus::try_from(value) {
        Ok(wire::TaskStatus::Succeeded) => Ok(TaskStatus::Succeeded),
        Ok(wire::TaskStatus::Failed) => Ok(TaskStatus::Failed),
        Ok(wire::TaskStatus::Cancelled) => Ok(TaskStatus::Cancelled),
        Ok(wire::TaskStatus::LeaseExpired) => Ok(TaskStatus::LeaseExpired),
        _ => Err(Status::invalid_argument("TaskResult.status is required")),
    }
}

pub(super) fn reject_reason(value: i32) -> Result<RejectReason, Status> {
    match wire::RejectReason::try_from(value) {
        Ok(wire::RejectReason::UnsupportedKind) => Ok(RejectReason::UnsupportedKind),
        Ok(wire::RejectReason::Quota) => Ok(RejectReason::Quota),
        Ok(wire::RejectReason::Draining) => Ok(RejectReason::Draining),
        Ok(wire::RejectReason::Expired) => Ok(RejectReason::Expired),
        _ => Err(Status::invalid_argument("LeaseReject.reason is required")),
    }
}

pub(super) fn config_state(value: i32) -> Result<ConfigState, Status> {
    match wire::ConfigState::try_from(value) {
        Ok(wire::ConfigState::Applied) => Ok(ConfigState::Applied),
        Ok(wire::ConfigState::Rejected) => Ok(ConfigState::Rejected),
        Ok(wire::ConfigState::RolledBack) => Ok(ConfigState::RolledBack),
        _ => Err(Status::invalid_argument("ConfigStatus.state is required")),
    }
}

/// What a message turns into on the stream.
pub(super) enum Frame {
    Send(wire::ControlMessage),
    End,
    Fail(Status),
}

fn message(payload: Payload) -> Frame {
    Frame::Send(wire::ControlMessage {
        payload: Some(payload),
    })
}

pub(super) fn frame(outbound: Outbound) -> Frame {
    match outbound {
        Outbound::Welcome {
            protocol_version,
            heartbeat_interval_secs,
            desired_config_revision,
            server_time,
        } => message(Payload::Welcome(wire::Welcome {
            protocol_version,
            heartbeat_interval_seconds: heartbeat_interval_secs,
            desired_config_revision,
            server_time: seconds(server_time),
        })),
        Outbound::HeartbeatAck {
            seq,
            desired_config_revision,
            renewals,
            server_time,
        } => message(Payload::HeartbeatAck(wire::HeartbeatAck {
            seq,
            desired_config_revision,
            lease_renewals: renewals
                .into_iter()
                .map(|(key, expires)| wire::LeaseRenewal {
                    task_id: key.task_id.0,
                    attempt: key.attempt,
                    lease_expires_at: seconds(expires),
                })
                .collect(),
            server_time: seconds(server_time),
        })),
        Outbound::ConfigUpdate {
            revision,
            config,
            signature,
        } => message(Payload::ConfigUpdate(wire::ConfigUpdate {
            revision,
            config: config.into(),
            signature: signature.into(),
        })),
        Outbound::LeaseOffer { task, server_time } => {
            let spec = serde_json::to_vec(&task.spec).unwrap_or_default();

            message(Payload::LeaseOffer(wire::LeaseOffer {
                task_id: task.id.0,
                attempt: task.attempt,
                kind: task.kind.as_str().into(),
                spec: spec.into(),
                lease_expires_at: task.lease_expires_at.map(seconds).unwrap_or_default(),
                deadline: seconds(task.deadline),
                server_time: seconds(server_time),
            }))
        }
        Outbound::LeaseCancel { key, reason } => message(Payload::LeaseCancel(wire::LeaseCancel {
            task_id: key.task_id.0,
            attempt: key.attempt,
            reason,
        })),
        Outbound::ResultAck { result_id } => {
            message(Payload::ResultAck(wire::ResultAck { result_id }))
        }
        Outbound::Drain { deadline } => message(Payload::Drain(wire::Drain {
            deadline: seconds(deadline),
        })),
        Outbound::Revoke { reason } => message(Payload::Revoke(wire::Revoke { reason })),
        Outbound::End => Frame::End,
        Outbound::Close(error) => Frame::Fail(status(error)),
    }
}

pub(super) fn status(error: Error) -> Status {
    match error {
        Error::NotFound(message) => Status::not_found(message),
        Error::Conflict(message) => Status::failed_precondition(message),
        Error::InvalidArgument(message) | Error::Validation { message, .. } => {
            Status::invalid_argument(message)
        }
        Error::Unauthorized(message) => Status::unauthenticated(message),
        Error::Forbidden(message) => Status::permission_denied(message),
        Error::ResourceExhausted(message) | Error::PayloadTooLarge(message) => {
            Status::resource_exhausted(message)
        }
        Error::Cancelled(message) => Status::cancelled(message),
        Error::Unavailable(message) => Status::unavailable(message),
        Error::Internal(message) => {
            tracing::error!(error = %message, "node control gRPC internal error");
            Status::internal("internal node control error")
        }
        Error::Other(error) => {
            tracing::error!(%error, "node control gRPC internal error");
            Status::internal("internal node control error")
        }
    }
}
