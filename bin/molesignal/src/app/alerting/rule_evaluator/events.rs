// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use crate::{
    app::{
        alerting::AlertIncidentLifecycleSink,
        notify::{ALERT_RESOLVED_EVENT, NotifyEngine, alert_dispatch, triggered_event_id},
    },
    domain::alerting::incident::Incident,
};

pub(super) async fn enqueue_notify_event(
    engine: Option<&Arc<NotifyEngine>>,
    incident: &Incident,
    event_type: &str,
) {
    let Some(engine) = engine else {
        return;
    };
    if event_type == ALERT_RESOLVED_EVENT
        && let Err(error) = engine
            .acknowledge_event(
                &incident.org_id,
                &triggered_event_id(&incident.id),
                incident.resolved_at.unwrap_or(incident.created_at),
            )
            .await
    {
        tracing::warn!(
            incident_id = %incident.id,
            error = %error,
            "notify alert resolved acknowledgement update failed"
        );
    }
    if let Err(error) = engine
        .enqueue_event(alert_dispatch(incident, event_type))
        .await
    {
        tracing::warn!(
            incident_id = %incident.id,
            event_type,
            error = %error,
            "alert notify event enqueue failed"
        );
    }
}

pub(super) async fn emit_lifecycle(
    sink: Option<&Arc<dyn AlertIncidentLifecycleSink>>,
    incident: &Incident,
) {
    if let Some(sink) = sink
        && let Err(error) = sink.on_incident_changed(incident).await
    {
        tracing::warn!(
            incident_id = %incident.id,
            error = %error,
            "alert Incident lifecycle sink failed"
        );
    }
}
