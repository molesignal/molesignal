// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::NotifyTemplatePreset;
use crate::domain::notify::preference::NotifyCategory;

fn preset(
    key: &str,
    category: NotifyCategory,
    event_type: &str,
    format: &str,
    body: &str,
) -> NotifyTemplatePreset {
    NotifyTemplatePreset {
        key: key.into(),
        category,
        event_type: event_type.into(),
        format: format.into(),
        body: body.into(),
    }
}

pub fn notify_template_preset_catalog() -> Vec<NotifyTemplatePreset> {
    use NotifyCategory::{Alert, Escalation, Oncall, Report, Security, System};

    vec![
        preset(
            "alert_text",
            Alert,
            "alert.triggered",
            "text",
            "[{{severity}}] {{rule.name}}\nstatus={{incident.status}} service={{labels.service}}\nvalue={{value}} threshold={{threshold}}\nfingerprint={{incident.fingerprint}}",
        ),
        preset(
            "alert_markdown",
            Alert,
            "alert.triggered",
            "markdown",
            "**[{{severity}}] {{rule.name}}**\n\n- Status: `{{incident.status}}`\n- Service: `{{labels.service}}`\n- Value: `{{value}}` / `{{threshold}}`\n- Fingerprint: `{{incident.fingerprint}}`",
        ),
        preset(
            "alert_html",
            Alert,
            "alert.triggered",
            "html",
            "<h3>[{{severity}}] {{rule.name}}</h3>\n<p><strong>Status:</strong> {{incident.status}}</p>\n<p><strong>Service:</strong> {{labels.service}}</p>\n<p><code>{{incident.fingerprint}}</code></p>",
        ),
        preset(
            "oncall_shift",
            Oncall,
            "oncall.shift.starting",
            "markdown",
            "**{{message.title}}**\n\n- Schedule: `{{schedule.name}}`\n- Current: `{{oncall.current_user_id}}`\n- Next: `{{oncall.next_user_id}}`\n- Transition: `{{oncall.transition_at}}`\n- Timezone: `{{schedule.timezone}}`",
        ),
        preset(
            "oncall_override",
            Oncall,
            "oncall.override.created",
            "markdown",
            "**On-call override · {{schedule.name}}**\n\n- Original: `{{override.original_user_id}}`\n- Substitute: `{{override.user_id}}`\n- Window: `{{override.start_at}}` → `{{override.end_at}}`\n- Reason: {{override.reason}}",
        ),
        preset(
            "oncall_coverage",
            Oncall,
            "oncall.coverage.missing",
            "text",
            "[Coverage missing] {{schedule.name}}\n{{message.text}}\ntimezone={{schedule.timezone}} team={{schedule.team_id}}",
        ),
        preset(
            "oncall_html",
            Oncall,
            "oncall.shift.starting",
            "html",
            "<h3>{{message.title}}</h3>\n<p><strong>Schedule:</strong> {{schedule.name}}</p>\n<p><strong>Current:</strong> {{oncall.current_user_id}}</p>\n<p><strong>Next:</strong> {{oncall.next_user_id}}</p>\n<p><strong>Transition:</strong> {{oncall.transition_at}}</p>",
        ),
        preset(
            "escalation_text",
            Escalation,
            "alert.escalated",
            "text",
            "[ESCALATION · {{severity}}] {{rule.name}}\nstatus={{incident.status}} service={{labels.service}}\nvalue={{value}} threshold={{threshold}}\nfingerprint={{incident.fingerprint}}",
        ),
        preset(
            "escalation_markdown",
            Escalation,
            "alert.escalated",
            "markdown",
            "**[ESCALATION · {{severity}}] {{rule.name}}**\n\n- Status: `{{incident.status}}`\n- Service: `{{labels.service}}`\n- Value: `{{value}}` / `{{threshold}}`\n- Fingerprint: `{{incident.fingerprint}}`",
        ),
        preset(
            "escalation_html",
            Escalation,
            "alert.escalated",
            "html",
            "<h3>[ESCALATION · {{severity}}] {{rule.name}}</h3>\n<p><strong>Status:</strong> {{incident.status}}</p>\n<p><strong>Service:</strong> {{labels.service}}</p>\n<p><code>{{incident.fingerprint}}</code></p>",
        ),
        preset(
            "report_text",
            Report,
            "report.ready",
            "text",
            "{{message.title}}\n{{message.text}}\nreport={{event.attributes.report_name}}\nperiod={{event.attributes.period_start}} - {{event.attributes.period_end}}\ndownload={{event.attributes.download_url}}",
        ),
        preset(
            "report_markdown",
            Report,
            "report.ready",
            "markdown",
            "**{{message.title}}**\n\n{{message.text}}\n\n- Report: `{{event.attributes.report_name}}`\n- Period: `{{event.attributes.period_start}}` – `{{event.attributes.period_end}}`\n- [Download report]({{event.attributes.download_url}})",
        ),
        preset(
            "report_html",
            Report,
            "report.ready",
            "html",
            "<h3>{{message.title}}</h3>\n<p>{{message.text}}</p>\n<p><strong>Report:</strong> {{event.attributes.report_name}}</p>\n<p><strong>Period:</strong> {{event.attributes.period_start}} – {{event.attributes.period_end}}</p>\n<p><a href=\"{{event.attributes.download_url}}\">Download report</a></p>",
        ),
        preset(
            "security_text",
            Security,
            "security.access.detected",
            "text",
            "{{message.title}}\n{{message.text}}\naction={{event.attributes.action}}\nactor={{event.attributes.actor}}\nresource={{event.attributes.resource}}\nip={{event.attributes.ip_address}}",
        ),
        preset(
            "security_markdown",
            Security,
            "security.access.detected",
            "markdown",
            "**{{message.title}}**\n\n{{message.text}}\n\n- Action: `{{event.attributes.action}}`\n- Actor: `{{event.attributes.actor}}`\n- Resource: `{{event.attributes.resource}}`\n- IP: `{{event.attributes.ip_address}}`",
        ),
        preset(
            "security_html",
            Security,
            "security.access.detected",
            "html",
            "<h3>{{message.title}}</h3>\n<p>{{message.text}}</p>\n<p><strong>Action:</strong> {{event.attributes.action}}</p>\n<p><strong>Actor:</strong> {{event.attributes.actor}}</p>\n<p><strong>Resource:</strong> {{event.attributes.resource}}</p>\n<p><strong>IP:</strong> {{event.attributes.ip_address}}</p>",
        ),
        preset(
            "system_text",
            System,
            "system.health.changed",
            "text",
            "{{message.title}}\n{{message.text}}\ncomponent={{event.attributes.component}}\nstatus={{event.attributes.status}}\nregion={{event.attributes.region}}",
        ),
        preset(
            "system_markdown",
            System,
            "system.health.changed",
            "markdown",
            "**{{message.title}}**\n\n{{message.text}}\n\n- Component: `{{event.attributes.component}}`\n- Status: `{{event.attributes.status}}`\n- Region: `{{event.attributes.region}}`",
        ),
        preset(
            "system_html",
            System,
            "system.health.changed",
            "html",
            "<h3>{{message.title}}</h3>\n<p>{{message.text}}</p>\n<p><strong>Component:</strong> {{event.attributes.component}}</p>\n<p><strong>Status:</strong> {{event.attributes.status}}</p>\n<p><strong>Region:</strong> {{event.attributes.region}}</p>",
        ),
    ]
}
