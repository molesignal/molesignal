// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::field::{RumField, RumUnit};

/// What a `resource` event adds to the common columns. The timings are nanosecond offsets from the
/// start of the request, each phase as a `start` and a `duration`.
pub(super) const RESOURCE: &[RumField] = &[
    RumField::text("resource.id"),
    RumField::text("resource.type"),
    RumField::text("resource.method"),
    RumField::text("resource.url").long(4096),
    RumField::text("resource.url_host"),
    RumField::text("resource.url_path").long(2048),
    RumField::text("resource.url_path_group").long(2048),
    RumField::int("resource.status_code"),
    RumField::ns("resource.duration"),
    RumField::int("resource.size").unit(RumUnit::Bytes),
    RumField::int("resource.encoded_body_size").unit(RumUnit::Bytes),
    RumField::int("resource.decoded_body_size").unit(RumUnit::Bytes),
    RumField::int("resource.transfer_size").unit(RumUnit::Bytes),
    RumField::text("resource.render_blocking_status"),
    RumField::text("resource.protocol"),
    RumField::text("resource.delivery_type"),
    RumField::ns("resource.worker.start"),
    RumField::ns("resource.worker.duration"),
    RumField::ns("resource.redirect.start"),
    RumField::ns("resource.redirect.duration"),
    RumField::ns("resource.dns.start"),
    RumField::ns("resource.dns.duration"),
    RumField::ns("resource.connect.start"),
    RumField::ns("resource.connect.duration"),
    RumField::ns("resource.ssl.start"),
    RumField::ns("resource.ssl.duration"),
    RumField::ns("resource.first_byte.start"),
    RumField::ns("resource.first_byte.duration"),
    RumField::ns("resource.download.start"),
    RumField::ns("resource.download.duration"),
    RumField::json("resource.request.headers"),
    RumField::json("resource.response.headers"),
    RumField::text("resource.graphql.operation_type"),
    RumField::text("resource.graphql.operation_name"),
    RumField::text("resource.graphql.payload").long(4096),
    RumField::text("resource.graphql.variables").long(4096),
    RumField::text("resource.trace_id").exact().long(32),
    RumField::text("resource.span_id").long(16),
];

/// What a `long_task` event adds to the common columns. The `top_script` columns are not sent:
/// intake works them out from `scripts`.
pub(super) const LONG_TASK: &[RumField] = &[
    RumField::text("long_task.id"),
    RumField::ns("long_task.duration"),
    RumField::text("long_task.entry_type"),
    RumField::ns("long_task.start_time"),
    RumField::ns("long_task.blocking_duration"),
    RumField::ns("long_task.render_start"),
    RumField::ns("long_task.style_and_layout_start"),
    RumField::ns("long_task.first_ui_event_timestamp"),
    RumField::flag("long_task.is_frozen_frame"),
    RumField::json("long_task.scripts"),
    RumField::text("long_task.top_script.source_url").long(2048),
    RumField::text("long_task.top_script.function").long(256),
    RumField::text("long_task.top_script.invoker_type"),
];

/// What a `vital` event adds to the common columns.
pub(super) const VITAL: &[RumField] = &[
    RumField::text("vital.id"),
    RumField::text("vital.type"),
    RumField::text("vital.name").long(256),
    RumField::text("vital.description").long(2048),
    RumField::ns("vital.duration"),
    RumField::text("vital.app_launch_metric"),
    RumField::text("vital.startup_type"),
    RumField::flag("vital.is_prewarmed"),
    RumField::flag("vital.has_saved_instance_state_bundle"),
    RumField::text("vital.step_type"),
    RumField::text("vital.operation_key").long(256),
    RumField::text("vital.failure_reason"),
];

/// What a `log` event adds to the common columns. `level` and `message` have the names any Logs
/// stream has them by, so the logs explorer reads these rows as it reads any log.
pub(super) const LOG: &[RumField] = &[
    RumField::text("log.id"),
    RumField::text("level").exact(),
    RumField::text("message").full_text().long(16384),
    RumField::text("log.logger").long(256),
    RumField::text("log.origin"),
    RumField::text("log.error_type").long(256),
    RumField::text("log.stack").long(32768),
];
