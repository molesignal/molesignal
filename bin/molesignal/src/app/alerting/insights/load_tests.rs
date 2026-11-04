// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::domain::alerting::schedule::{Rotation, RotationKind};

fn ts(value: &str) -> TimestampMicros {
    TimestampMicros::from_datetime(
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("rfc3339")
            .with_timezone(&Utc),
    )
}

fn schedule(id: &str, timezone: &str, members: &[&str], start: &str) -> Schedule {
    Schedule {
        id: Id::from_string(id),
        org_id: Id::from_string("org-a"),
        name: id.into(),
        description: String::new(),
        team_id: None,
        timezone: timezone.into(),
        enabled: true,
        rotations: vec![Rotation {
            id: Id::new(),
            name: "primary".into(),
            members: members
                .iter()
                .map(|member| Id::from_string(*member))
                .collect(),
            kind: RotationKind::Weekly,
            active_windows: Vec::new(),
            calendar: None,
            start_at: ts(start),
            end_at: None,
        }],
        overrides: Vec::new(),
        swap_needs_approval: false,
        created_by: None,
        updated_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn page(user: &str, event: &str, at: &str) -> PageRecord {
    PageRecord {
        user_id: Id::from_string(user),
        event_id: event.into(),
        at: ts(at),
    }
}

#[test]
fn night_and_weekend_pages_are_judged_in_the_schedule_timezone() {
    let shanghai = schedule(
        "sch-pay",
        "Asia/Shanghai",
        &["u1", "u2"],
        "2026-05-18T00:00:00Z",
    );
    let pages = [
        // Friday 23:30 in Shanghai: a night, not a weekend.
        page("u1", "alert.triggered:a", "2026-05-22T15:30:00Z"),
        // Saturday 02:00 in Shanghai: the same night (Friday's), and a weekend.
        page("u1", "alert.escalated:b", "2026-05-22T18:00:00Z"),
        // Saturday 15:00 in Shanghai: a weekend day.
        page("u1", "alert.triggered:c", "2026-05-23T07:00:00Z"),
        // Someone in no schedule: judged in UTC, 03:00 on a Monday.
        page("u9", "alert.triggered:d", "2026-05-25T03:00:00Z"),
    ];

    let loads = oncall_load(
        &[shanghai],
        &pages,
        &HashMap::new(),
        ts("2026-05-18T00:00:00Z"),
        ts("2026-06-01T00:00:00Z"),
        None,
    );

    let u1 = loads
        .iter()
        .find(|load| load.user_id.0 == "u1")
        .expect("u1");
    assert_eq!(u1.pages, 3);
    assert_eq!(u1.night_pages, 2);
    assert_eq!(u1.weekend_pages, 2);
    assert_eq!(u1.interrupted_nights, 1);
    assert_eq!(u1.oncall_secs, 7 * 86_400);
    assert_eq!(u1.timezone, "Asia/Shanghai");

    let u2 = loads
        .iter()
        .find(|load| load.user_id.0 == "u2")
        .expect("u2");
    assert_eq!((u2.pages, u2.oncall_secs), (0, 7 * 86_400));

    let u9 = loads
        .iter()
        .find(|load| load.user_id.0 == "u9")
        .expect("u9");
    assert_eq!((u9.night_pages, u9.timezone.as_str()), (1, "UTC"));
    assert_eq!(loads[0].user_id.0, "u1", "most paged first");
}

#[test]
fn a_quiet_hours_timezone_stands_in_and_one_schedule_can_be_asked_alone() {
    let pages = [page("u9", "alert.triggered:a", "2026-05-25T03:00:00Z")];
    let zones = HashMap::from([(Id::from_string("u9"), "America/New_York".to_string())]);

    // 03:00 UTC is 23:00 the evening before in New York.
    let loads = oncall_load(
        &[],
        &pages,
        &zones,
        ts("2026-05-18T00:00:00Z"),
        ts("2026-06-01T00:00:00Z"),
        None,
    );
    assert_eq!(
        (loads[0].night_pages, loads[0].timezone.as_str()),
        (1, "America/New_York")
    );

    let only = oncall_load(
        &[schedule("sch-pay", "UTC", &["u1"], "2026-05-18T00:00:00Z")],
        &pages,
        &zones,
        ts("2026-05-18T00:00:00Z"),
        ts("2026-05-19T00:00:00Z"),
        Some(&Id::from_string("sch-pay")),
    );
    assert_eq!(
        only.len(),
        1,
        "pages of people outside the schedule are left out"
    );
    assert_eq!(
        (only[0].user_id.0.as_str(), only[0].oncall_secs),
        ("u1", 86_400)
    );
}
