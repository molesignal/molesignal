// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a status page writes to its customers by itself: its emails and the
//! update it publishes when maintenance starts at the planned time. The copy
//! comes from the backend language pack, in the default language of the page.

use serde_json::Value;

use super::StatusPageService;
use crate::{
    domain::{i18n_language::LanguagePack, status_page::StatusPageNotificationMessage},
    shared::{Error, Result, time::TimestampMicros},
};

/// Every entry this copy is written from has a key under this prefix.
const LANGUAGE_KEY_PREFIX: &str = "status_page.";

/// Subject and plain-text body of one email.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct EmailCopy {
    pub(super) title: String,
    pub(super) text: String,
}

impl EmailCopy {
    pub(super) fn with_payload(self, payload: Value) -> StatusPageNotificationMessage {
        StatusPageNotificationMessage {
            title: self.title,
            text: self.text,
            payload,
        }
    }
}

/// An update on an incident or on maintenance, as its email tells it.
pub(super) struct EventUpdate<'a> {
    pub(super) page_name: &'a str,
    pub(super) page_url: &'a str,
    pub(super) maintenance: bool,
    pub(super) title: &'a str,
    pub(super) status: &'a str,
    pub(super) message: &'a str,
    /// The planned start of maintenance, written for the time zone of the page.
    pub(super) scheduled_for: Option<String>,
}

impl StatusPageService {
    /// The entries customer copy is written from, in the language of a page.
    pub(super) async fn language_pack(&self, language_code: &str) -> Result<LanguagePack> {
        let i18n_language = self
            .i18n_language
            .as_ref()
            .ok_or_else(|| Error::unavailable("status-page email copy is not configured"))?;

        i18n_language
            .load_pack(LANGUAGE_KEY_PREFIX, language_code)
            .await
    }
}

pub(super) fn subscription_confirmation(
    pack: &LanguagePack,
    page_name: &str,
    confirm_url: &str,
    unsubscribe_url: &str,
) -> Result<EmailCopy> {
    let fills = [
        ("page_name", page_name),
        ("confirm_url", confirm_url),
        ("unsubscribe_url", unsubscribe_url),
    ];

    email(pack, "status_page.email.subscription_confirmation", &fills)
}

pub(super) fn access_link(
    pack: &LanguagePack,
    page_name: &str,
    access_url: &str,
    valid_minutes: i64,
) -> Result<EmailCopy> {
    let minutes = valid_minutes.to_string();

    let fills = [
        ("page_name", page_name),
        ("access_url", access_url),
        ("minutes", minutes.as_str()),
    ];

    email(pack, "status_page.email.access_link", &fills)
}

pub(super) fn component_status_changed(
    pack: &LanguagePack,
    page_name: &str,
    page_url: &str,
    component_name: &str,
    status: &str,
) -> Result<EmailCopy> {
    let fills = [
        ("page_name", page_name),
        ("page_url", page_url),
        ("component_name", component_name),
        ("status", status_name(pack, "component_status", status)),
    ];

    email(pack, "status_page.email.component_status", &fills)
}

pub(super) fn event_update(pack: &LanguagePack, update: &EventUpdate<'_>) -> Result<EmailCopy> {
    let statuses = if update.maintenance {
        "maintenance_status"
    } else {
        "incident_status"
    };

    let mut fills = vec![
        ("page_name", update.page_name),
        ("page_url", update.page_url),
        ("title", update.title),
        ("status", status_name(pack, statuses, update.status)),
        ("message", update.message),
    ];

    // Only the maintenance email has a line for the planned time.
    match update.scheduled_for.as_deref() {
        Some(scheduled_for) => {
            fills.push(("scheduled_for", scheduled_for));
            email(pack, "status_page.email.maintenance_update", &fills)
        }
        None => email(pack, "status_page.email.incident_update", &fills),
    }
}

/// The reminder that goes out before planned maintenance starts.
pub(super) fn maintenance_reminder(
    pack: &LanguagePack,
    page_name: &str,
    page_url: &str,
    title: &str,
    scheduled_for: &str,
) -> Result<EmailCopy> {
    let fills = [
        ("page_name", page_name),
        ("page_url", page_url),
        ("title", title),
        ("scheduled_for", scheduled_for),
    ];

    email(pack, "status_page.email.maintenance_reminder", &fills)
}

/// The text of the update a page publishes when maintenance starts by itself.
pub(super) fn maintenance_started_update(pack: &LanguagePack) -> Result<String> {
    pack.render("status_page.maintenance.started_update", &[])
}

/// Wall-clock time in the time zone of the page, with the UTC offset in force
/// at that instant: `2026-05-25 12:00 UTC+02:00 (Europe/Berlin)`.
pub(super) fn zoned_time(at: TimestampMicros, timezone: &str) -> String {
    let zone = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    let local = at.to_datetime().with_timezone(&zone);

    if zone == chrono_tz::UTC {
        return local.format("%Y-%m-%d %H:%M UTC").to_string();
    }

    let wall_clock = local.format("%Y-%m-%d %H:%M UTC%:z");
    format!("{wall_clock} ({})", zone.name())
}

fn email(pack: &LanguagePack, key: &str, fills: &[(&str, &str)]) -> Result<EmailCopy> {
    Ok(EmailCopy {
        title: pack.render(&format!("{key}.subject"), fills)?,
        text: pack.render(&format!("{key}.body"), fills)?,
    })
}

/// The name of a status as the customer-facing page shows it. A status without
/// a name in the pack keeps its stored value.
fn status_name<'a>(pack: &'a LanguagePack, statuses: &str, status: &'a str) -> &'a str {
    pack.get(&format!("{LANGUAGE_KEY_PREFIX}{statuses}.{status}"))
        .unwrap_or(status)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::domain::status_page::{ComponentStatus, PublicIncidentStatus};

    /// 2026-05-25 10:00 UTC.
    const PLANNED_START: i64 = 1_779_703_200_000_000;

    /// The seeded entries status page emails are written from.
    pub(in crate::app::status_page) fn seeded_pack(language_code: &str) -> LanguagePack {
        crate::shared::i18n_language_seed::seeded_pack(LANGUAGE_KEY_PREFIX, language_code)
    }

    fn maintenance_update(status: &str, scheduled_for: Option<String>) -> EventUpdate<'_> {
        EventUpdate {
            page_name: "Acme Cloud",
            page_url: "https://status.acme.example/",
            maintenance: true,
            title: "Database upgrade",
            status,
            message: "Writes can take a few seconds longer.",
            scheduled_for,
        }
    }

    #[test]
    fn subscription_confirmation_is_written_in_the_page_language() {
        let confirm = "https://status.acme.example/#confirm";
        let unsubscribe = "https://status.acme.example/#unsubscribe";

        let english =
            subscription_confirmation(&seeded_pack("en-us"), "Acme Cloud", confirm, unsubscribe)
                .unwrap();
        assert_eq!(english.title, "Confirm updates from Acme Cloud");
        assert_eq!(
            english.text,
            "Confirm your subscription to Acme Cloud:\nhttps://status.acme.example/#confirm\n\nIf you did not request this, no action is required. You can unsubscribe with:\nhttps://status.acme.example/#unsubscribe"
        );

        let chinese =
            subscription_confirmation(&seeded_pack("zh-cn"), "Acme Cloud", confirm, unsubscribe)
                .unwrap();
        assert_eq!(chinese.title, "确认订阅 Acme Cloud 的状态更新");
        assert_eq!(
            chinese.text,
            "打开以下链接，确认订阅 Acme Cloud 的状态更新：\nhttps://status.acme.example/#confirm\n\n如果没有申请过订阅，无需任何操作。退订链接：\nhttps://status.acme.example/#unsubscribe"
        );
    }

    #[test]
    fn access_link_states_how_long_the_link_is_valid() {
        let url = "https://status.acme.example/#access";

        let english = access_link(&seeded_pack("en-us"), "Acme Cloud", url, 15).unwrap();
        assert_eq!(english.title, "Access Acme Cloud");
        assert_eq!(
            english.text,
            "Use this one-time link within 15 minutes to access Acme Cloud:\nhttps://status.acme.example/#access"
        );

        let chinese = access_link(&seeded_pack("zh-cn"), "Acme Cloud", url, 15).unwrap();
        assert_eq!(chinese.title, "访问 Acme Cloud");
        assert_eq!(
            chinese.text,
            "请在 15 分钟内使用以下一次性链接访问 Acme Cloud：\nhttps://status.acme.example/#access"
        );
    }

    #[test]
    fn component_change_names_the_status_as_the_page_does() {
        let page_url = "https://status.acme.example/";

        let english = component_status_changed(
            &seeded_pack("en-us"),
            "Acme Cloud",
            page_url,
            "API",
            "degraded_performance",
        )
        .unwrap();
        assert_eq!(english.title, "Acme Cloud: API · Degraded performance");
        assert_eq!(
            english.text,
            "API changed to Degraded performance.\n\nhttps://status.acme.example/"
        );

        let chinese = component_status_changed(
            &seeded_pack("zh-cn"),
            "Acme Cloud",
            page_url,
            "API",
            "degraded_performance",
        )
        .unwrap();
        assert_eq!(chinese.title, "Acme Cloud：API · 性能下降");
        assert_eq!(
            chinese.text,
            "API 的状态已变为“性能下降”。\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn maintenance_update_states_the_planned_time() {
        let scheduled_for = zoned_time(TimestampMicros(PLANNED_START), "Europe/Berlin");

        let english = event_update(
            &seeded_pack("en-us"),
            &maintenance_update("scheduled", Some(scheduled_for.clone())),
        )
        .unwrap();
        assert_eq!(english.title, "Acme Cloud: Database upgrade · Scheduled");
        assert_eq!(
            english.text,
            "Database upgrade\nScheduled\nScheduled for: 2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );

        let chinese = event_update(
            &seeded_pack("zh-cn"),
            &maintenance_update("scheduled", Some(scheduled_for)),
        )
        .unwrap();
        assert_eq!(chinese.title, "Acme Cloud：Database upgrade · 已计划");
        assert_eq!(
            chinese.text,
            "Database upgrade\n已计划\n计划时间：2026-05-25 12:00 UTC+02:00 (Europe/Berlin)\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn maintenance_reminder_states_the_planned_time() {
        let scheduled_for = zoned_time(TimestampMicros(PLANNED_START), "Asia/Shanghai");
        let page_url = "https://status.acme.example/";

        let english = maintenance_reminder(
            &seeded_pack("en-us"),
            "Acme Cloud",
            page_url,
            "Database upgrade",
            &scheduled_for,
        )
        .unwrap();
        assert_eq!(english.title, "Acme Cloud: Database upgrade · Reminder");
        assert_eq!(
            english.text,
            "Reminder of planned maintenance:\n\nDatabase upgrade\nScheduled for: 2026-05-25 18:00 UTC+08:00 (Asia/Shanghai)\n\nhttps://status.acme.example/"
        );

        let chinese = maintenance_reminder(
            &seeded_pack("zh-cn"),
            "Acme Cloud",
            page_url,
            "Database upgrade",
            &scheduled_for,
        )
        .unwrap();
        assert_eq!(chinese.title, "Acme Cloud：Database upgrade · 提醒");
        assert_eq!(
            chinese.text,
            "计划维护提醒：\n\nDatabase upgrade\n计划时间：2026-05-25 18:00 UTC+08:00 (Asia/Shanghai)\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn automatic_start_update_is_written_in_the_page_language() {
        assert_eq!(
            maintenance_started_update(&seeded_pack("en-us")).unwrap(),
            "The maintenance has started as planned."
        );
        assert_eq!(
            maintenance_started_update(&seeded_pack("zh-cn")).unwrap(),
            "维护已按计划开始。"
        );
    }

    #[test]
    fn work_in_progress_is_named_by_the_kind_of_event() {
        let pack = seeded_pack("zh-cn");
        let maintenance = event_update(&pack, &maintenance_update("in_progress", None)).unwrap();
        assert_eq!(maintenance.title, "Acme Cloud：Database upgrade · 进行中");

        let incident = event_update(
            &pack,
            &EventUpdate {
                maintenance: false,
                ..maintenance_update("in_progress", None)
            },
        )
        .unwrap();
        assert_eq!(incident.title, "Acme Cloud：Database upgrade · 处理中");
        assert_eq!(
            incident.text,
            "Database upgrade\n处理中\n\nWrites can take a few seconds longer.\n\nhttps://status.acme.example/"
        );
    }

    #[test]
    fn a_status_without_a_name_keeps_its_stored_value() {
        let update = EventUpdate {
            maintenance: false,
            ..maintenance_update("postponed", None)
        };

        let copy = event_update(&seeded_pack("en-us"), &update).unwrap();
        assert_eq!(copy.title, "Acme Cloud: Database upgrade · postponed");
    }

    #[test]
    fn a_language_without_entries_reads_english() {
        let copy = access_link(&seeded_pack("fr-fr"), "Acme Cloud", "https://a.example", 15);
        assert_eq!(copy.unwrap().title, "Access Acme Cloud");
    }

    #[test]
    fn every_status_has_a_name_in_both_page_languages() {
        let component = [
            ComponentStatus::Operational,
            ComponentStatus::DegradedPerformance,
            ComponentStatus::PartialOutage,
            ComponentStatus::MajorOutage,
            ComponentStatus::Maintenance,
        ];

        let incident = [
            PublicIncidentStatus::Investigating,
            PublicIncidentStatus::Identified,
            PublicIncidentStatus::InProgress,
            PublicIncidentStatus::Monitoring,
            PublicIncidentStatus::Resolved,
        ];

        let maintenance = [
            PublicIncidentStatus::Scheduled,
            PublicIncidentStatus::InProgress,
            PublicIncidentStatus::Completed,
            PublicIncidentStatus::Cancelled,
        ];

        let stored: Vec<(&str, &str)> = component
            .iter()
            .map(|status| ("component_status", status.as_str()))
            .chain(
                incident
                    .iter()
                    .map(|status| ("incident_status", status.as_str())),
            )
            .chain(
                maintenance
                    .iter()
                    .map(|status| ("maintenance_status", status.as_str())),
            )
            .collect();

        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(language);

            for (statuses, status) in &stored {
                let name = status_name(&pack, statuses, status);
                assert_ne!(
                    name, *status,
                    "{language} has no name for {statuses}.{status}"
                );
            }
        }
    }

    #[test]
    fn zoned_time_follows_the_offset_in_force_at_that_instant() {
        let summer = TimestampMicros(PLANNED_START);
        // 2026-01-15 09:30 UTC.
        let winter = TimestampMicros(1_768_469_400_000_000);

        assert_eq!(zoned_time(summer, "UTC"), "2026-05-25 10:00 UTC");
        assert_eq!(
            zoned_time(summer, "Europe/Berlin"),
            "2026-05-25 12:00 UTC+02:00 (Europe/Berlin)"
        );
        assert_eq!(
            zoned_time(winter, "Europe/Berlin"),
            "2026-01-15 10:30 UTC+01:00 (Europe/Berlin)"
        );
        assert_eq!(
            zoned_time(summer, "Asia/Shanghai"),
            "2026-05-25 18:00 UTC+08:00 (Asia/Shanghai)"
        );
        assert_eq!(
            zoned_time(summer, "America/St_Johns"),
            "2026-05-25 07:30 UTC-02:30 (America/St_Johns)"
        );
        assert_eq!(zoned_time(summer, "Not/AZone"), "2026-05-25 10:00 UTC");
    }
}
