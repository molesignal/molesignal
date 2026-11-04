// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::Utc;

use super::*;
use crate::domain::alerting::time_window::ClockTime;

fn ts(s: &str) -> TimestampMicros {
    let dt = chrono::DateTime::parse_from_rfc3339(s)
        .expect("rfc3339")
        .with_timezone(&Utc);
    TimestampMicros::from_datetime(dt)
}

fn window(mask: u8, start: &str, end: &str) -> WeeklyRange {
    WeeklyRange {
        weekday_mask: mask,
        start: ClockTime::parse(start).unwrap(),
        end: ClockTime::parse(end).unwrap(),
    }
}

fn rotation(members: Vec<Id>, kind: RotationKind, start: &str) -> Rotation {
    Rotation {
        id: Id::from_string("primary"),
        name: "primary".into(),
        members,
        kind,
        active_windows: Vec::new(),
        calendar: None,
        start_at: ts(start),
        end_at: None,
    }
}

fn schedule(
    timezone: &str,
    rotations: Vec<Rotation>,
    overrides: Vec<ScheduleOverride>,
) -> Schedule {
    Schedule {
        id: Id::new(),
        org_id: Id::new(),
        name: "primary".into(),
        description: String::new(),
        team_id: None,
        timezone: timezone.into(),
        enabled: true,
        rotations,
        overrides,
        swap_needs_approval: false,
        created_by: None,
        updated_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

#[test]
fn rotation_picks_member_by_period_then_filters_window() {
    let members: Vec<Id> = (0..3).map(|_| Id::new()).collect();
    let weekly = rotation(
        members.clone(),
        RotationKind::Weekly,
        "2026-05-20T00:00:00Z",
    );

    assert_eq!(
        weekly.resolve(ts("2026-05-21T03:00:00Z")),
        Some(members[0].clone())
    );
    assert_eq!(
        weekly.resolve(ts("2026-05-28T00:00:00Z")),
        Some(members[1].clone())
    );

    // Weekdays 09:00–18:00 only: Saturday 2026-05-23 is out, Monday 2026-06-01 noon is the second week.
    let office = Rotation {
        active_windows: vec![window(0b011_1110, "09:00", "18:00")],
        ..weekly
    };

    assert_eq!(office.resolve(ts("2026-05-23T12:00:00Z")), None);
    assert_eq!(
        office.resolve(ts("2026-06-01T12:00:00Z")),
        Some(members[1].clone())
    );
}

#[test]
fn active_windows_are_minute_precise_in_the_schedule_timezone() {
    let a = Id::new();
    let mut office = rotation(vec![a.clone()], RotationKind::Daily, "2026-05-18T00:00:00Z");
    office.active_windows = vec![window(0b011_1110, "09:30", "18:00")];
    let shanghai = schedule("Asia/Shanghai", vec![office], Vec::new());

    // Monday 2026-05-18: 09:30–18:00 in Shanghai is 01:30–10:00 UTC.
    assert_eq!(shanghai.who_is_on_call(ts("2026-05-18T01:29:00Z")), None);
    assert_eq!(
        shanghai.who_is_on_call(ts("2026-05-18T01:30:00Z")),
        Some(a.clone())
    );

    let shifts = shanghai.shifts(ts("2026-05-18T00:00:00Z"), ts("2026-05-19T00:00:00Z"));

    assert_eq!(
        shifts,
        vec![Shift {
            user_id: a,
            start_at: ts("2026-05-18T01:30:00Z"),
            end_at: ts("2026-05-18T10:00:00Z"),
        }]
    );
}

#[test]
fn handoffs_keep_their_local_time_across_daylight_saving() {
    let (a, b) = (Id::new(), Id::new());
    // Daily handoff at 09:00 New York time, starting Sunday 2026-03-01 (EST, 14:00 UTC).
    let daily = rotation(
        vec![a.clone(), b.clone()],
        RotationKind::Daily,
        "2026-03-01T14:00:00Z",
    );
    let new_york = schedule("America/New_York", vec![daily], Vec::new());

    // Monday 2026-03-09 09:30 EDT is 13:30 UTC: the ninth handoff (09:00 EDT, 13:00 UTC) has happened,
    // so it is the ninth shift (index 8) and `a` is on call.
    assert_eq!(
        new_york.who_is_on_call(ts("2026-03-09T13:30:00Z")),
        Some(a.clone())
    );
    assert_eq!(
        new_york.who_is_on_call(ts("2026-03-09T12:59:00Z")),
        Some(b.clone())
    );

    // The shift over the switch to EDT (Mar 7 09:00 EST → Mar 8 09:00 EDT) lasts 23 hours.
    let shifts = new_york.shifts(ts("2026-03-07T00:00:00Z"), ts("2026-03-10T00:00:00Z"));
    let handoffs: Vec<TimestampMicros> = shifts.iter().map(|shift| shift.start_at).collect();

    assert_eq!(
        handoffs,
        vec![
            ts("2026-03-07T00:00:00Z"),
            ts("2026-03-07T14:00:00Z"),
            ts("2026-03-08T13:00:00Z"),
            ts("2026-03-09T13:00:00Z"),
        ]
    );
}

#[test]
fn paused_schedule_never_resolves_an_on_call_member() {
    let member = Id::new();
    let mut paused = schedule(
        "UTC",
        vec![rotation(
            vec![member],
            RotationKind::Daily,
            "2026-05-20T00:00:00Z",
        )],
        Vec::new(),
    );
    paused.enabled = false;

    assert_eq!(paused.who_is_on_call(ts("2026-05-21T03:00:00Z")), None);
    assert!(
        paused
            .shifts(ts("2026-05-20T00:00:00Z"), ts("2026-05-21T00:00:00Z"))
            .is_empty()
    );

    let coverage = paused.segments(ts("2026-05-20T00:00:00Z"), ts("2026-05-21T00:00:00Z"));
    assert_eq!(coverage.len(), 1);
    assert_eq!(coverage[0].source, CoverageSource::Gap);
}

#[test]
fn shifts_follow_the_rotation_and_an_override_cuts_in() {
    let (a, b, c) = (Id::new(), Id::new(), Id::new());
    let start = ts("2026-05-20T00:00:00Z");
    let weekly = rotation(
        vec![a.clone(), b.clone()],
        RotationKind::Weekly,
        "2026-05-20T00:00:00Z",
    );
    let cover = ScheduleOverride {
        id: Id::from_string("dentist"),
        user_id: c.clone(),
        start_at: ts("2026-05-22T10:00:00Z"),
        end_at: ts("2026-05-22T12:30:00Z"),
        reason: "dentist".into(),
    };
    let utc = schedule("UTC", vec![weekly], vec![cover]);

    let got: Vec<(Id, TimestampMicros, TimestampMicros)> = utc
        .shifts(start, ts("2026-06-03T00:00:00Z"))
        .into_iter()
        .map(|shift| (shift.user_id, shift.start_at, shift.end_at))
        .collect();

    assert_eq!(
        got,
        vec![
            (a.clone(), start, ts("2026-05-22T10:00:00Z")),
            (
                c.clone(),
                ts("2026-05-22T10:00:00Z"),
                ts("2026-05-22T12:30:00Z")
            ),
            (a, ts("2026-05-22T12:30:00Z"), ts("2026-05-27T00:00:00Z")),
            (b, ts("2026-05-27T00:00:00Z"), ts("2026-06-03T00:00:00Z")),
        ]
    );

    let coverage = utc.segments(ts("2026-05-22T00:00:00Z"), ts("2026-05-23T00:00:00Z"));
    let sources: Vec<(CoverageSource, Option<Id>)> = coverage
        .iter()
        .map(|segment| (segment.source, segment.override_id.clone()))
        .collect();

    assert_eq!(
        sources,
        vec![
            (CoverageSource::Rotation, None),
            (CoverageSource::Override, Some(Id::from_string("dentist"))),
            (CoverageSource::Rotation, None),
        ]
    );
    assert_eq!(coverage[0].rotation_id, Some(Id::from_string("primary")));
    assert_eq!(coverage[1].user_id, Some(c));
    // The override stands in for whoever the rotation had then.
    assert_eq!(coverage[1].replaced_user_id, coverage[0].user_id);
    assert_eq!(coverage[0].replaced_user_id, None);
}

#[test]
fn coverage_lists_the_gaps_between_windows() {
    let a = Id::new();
    let mut nights = rotation(vec![a], RotationKind::Daily, "2026-05-18T00:00:00Z");
    nights.active_windows = vec![window(0b111_1111, "22:00", "06:00")];
    let utc = schedule("UTC", vec![nights], Vec::new());

    let coverage = utc.segments(ts("2026-05-18T00:00:00Z"), ts("2026-05-19T00:00:00Z"));
    let kinds: Vec<(CoverageSource, TimestampMicros)> = coverage
        .iter()
        .map(|segment| (segment.source, segment.start_at))
        .collect();

    // The night of Sunday the 17th runs until 06:00 on Monday; Monday night starts at 22:00.
    assert_eq!(
        kinds,
        vec![
            (CoverageSource::Rotation, ts("2026-05-18T00:00:00Z")),
            (CoverageSource::Gap, ts("2026-05-18T06:00:00Z")),
            (CoverageSource::Rotation, ts("2026-05-18T22:00:00Z")),
        ]
    );
}

#[test]
fn a_schedule_names_a_real_timezone_and_sensible_rotations() {
    let member = Id::new();
    let ok = schedule(
        "Europe/London",
        vec![rotation(
            vec![member.clone()],
            RotationKind::Daily,
            "2026-05-18T00:00:00Z",
        )],
        Vec::new(),
    );
    assert!(ok.validate().is_ok());

    let mut unknown = ok.clone();
    unknown.timezone = "Europe/Atlantis".into();
    assert!(unknown.validate().is_err());

    let mut zero = ok.clone();
    zero.rotations[0].kind = RotationKind::Custom { period_secs: 0 };
    assert!(zero.validate().is_err());

    let mut ends_at_its_start = ok.clone();
    ends_at_its_start.rotations[0].end_at = Some(ts("2026-05-18T00:00:00Z"));
    assert!(ends_at_its_start.validate().is_err());

    let mut empty_window = ok;
    empty_window.rotations[0].active_windows = vec![window(0, "09:00", "18:00")];
    assert!(empty_window.validate().is_err());
}

#[test]
fn a_rotation_stops_at_its_end_and_the_next_one_takes_over() {
    let ann = Id::from_string("ann");
    let bob = Id::from_string("bob");
    let cover = Rotation {
        id: Id::from_string("cover"),
        end_at: Some(ts("2026-05-20T12:00:00Z")),
        ..rotation(
            vec![ann.clone()],
            RotationKind::Weekly,
            "2026-05-18T00:00:00Z",
        )
    };
    let base = rotation(
        vec![bob.clone()],
        RotationKind::Daily,
        "2026-05-18T00:00:00Z",
    );
    let schedule = schedule("UTC", vec![cover.clone(), base], Vec::new());

    assert_eq!(cover.resolve(ts("2026-05-20T11:59:59Z")), Some(ann.clone()));
    assert_eq!(cover.resolve(ts("2026-05-20T12:00:00Z")), None);

    assert_eq!(
        schedule
            .segments(ts("2026-05-20T00:00:00Z"), ts("2026-05-21T00:00:00Z"))
            .into_iter()
            .map(|segment| (segment.user_id, segment.start_at))
            .collect::<Vec<_>>(),
        vec![
            (Some(ann), ts("2026-05-20T00:00:00Z")),
            (Some(bob), ts("2026-05-20T12:00:00Z")),
        ]
    );

    // A rotation that ended before the stretch asked about adds nothing to it.
    assert_eq!(
        cover.boundaries(
            ts("2026-05-21T00:00:00Z").0,
            ts("2026-05-22T00:00:00Z").0,
            chrono_tz::UTC
        ),
        Vec::<i64>::new()
    );
}
