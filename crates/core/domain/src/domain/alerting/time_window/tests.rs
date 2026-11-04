// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::NaiveDate;

use super::*;

fn ts(text: &str) -> TimestampMicros {
    TimestampMicros::from_datetime(
        DateTime::parse_from_rfc3339(text)
            .expect("rfc3339")
            .with_timezone(&Utc),
    )
}

fn at(text: &str) -> i64 {
    ts(text).0
}

fn clock(text: &str) -> ClockTime {
    ClockTime::parse(text).expect("clock time")
}

fn range(mask: u8, start: &str, end: &str) -> WeeklyRange {
    WeeklyRange {
        weekday_mask: mask,
        start: clock(start),
        end: clock(end),
    }
}

#[test]
fn clock_times_are_read_and_written_as_hours_and_minutes() {
    assert_eq!(clock("09:30").minutes(), 570);
    assert_eq!(clock("24:00"), ClockTime::END_OF_DAY);
    assert_eq!(clock("07:05").to_string(), "07:05");

    for bad in ["24:01", "9:30", "09:60", "ab:cd", "0930", ""] {
        assert!(ClockTime::parse(bad).is_err(), "{bad}");
    }

    let json = serde_json::to_string(&range(2, "09:30", "18:00")).unwrap();
    assert_eq!(json, r#"{"weekdayMask":2,"start":"09:30","end":"18:00"}"#);
    assert!(
        serde_json::from_str::<WeeklyRange>(r#"{"weekdayMask":2,"start":"9","end":"18:00"}"#)
            .is_err()
    );
}

#[test]
fn a_range_needs_a_day_a_start_before_midnight_and_some_length() {
    assert!(range(0, "09:00", "18:00").validate().is_err());
    assert!(range(0b1000_0000, "09:00", "18:00").validate().is_err());
    assert!(range(2, "24:00", "06:00").validate().is_err());
    assert!(range(2, "09:00", "09:00").validate().is_err());
    assert!(range(2, "00:00", "24:00").validate().is_ok());
    assert!(range(2, "22:00", "06:00").validate().is_ok());

    let many = vec![range(2, "09:00", "10:00"); MAX_WEEKLY_RANGES + 1];
    assert!(validate_ranges(&many).is_err());
}

#[test]
fn a_range_past_midnight_belongs_to_the_day_it_starts() {
    // Friday 22:00 to Saturday 06:00, Fridays only.
    let friday_night = range(1 << 5, "22:00", "06:00");

    assert!(friday_night.contains_local(5, 23 * 60));
    assert!(friday_night.contains_local(6, 5 * 60 + 59));
    assert!(!friday_night.contains_local(6, 6 * 60));
    assert!(!friday_night.contains_local(4, 23 * 60));
    // Monday 02:00 follows Sunday, which is not in the mask.
    assert!(!friday_night.contains_local(1, 2 * 60));

    let whole_monday = range(1 << 1, "00:00", "24:00");
    assert!(whole_monday.contains_local(1, 0));
    assert!(whole_monday.contains_local(1, 1439));
    assert!(!whole_monday.contains_local(2, 0));
}

#[test]
fn windows_are_minute_precise_in_their_timezone() {
    // Monday 2026-05-18, 09:30–18:00 in Shanghai is 01:30–10:00 UTC.
    let office = WeeklyWindows {
        timezone: "Asia/Shanghai".into(),
        ranges: vec![range(0b011_1110, "09:30", "18:00")],
        calendar: None,
    };

    assert!(office.validate().is_ok());
    assert!(!office.contains(ts("2026-05-18T01:29:00Z")));
    assert!(office.contains(ts("2026-05-18T01:30:00Z")));
    assert!(office.contains(ts("2026-05-18T09:59:00Z")));
    assert!(!office.contains(ts("2026-05-18T10:00:00Z")));

    assert_eq!(
        office.transitions(ts("2026-05-18T00:00:00Z"), ts("2026-05-19T00:00:00Z")),
        vec![at("2026-05-18T01:30:00Z"), at("2026-05-18T10:00:00Z")]
    );

    let unknown = WeeklyWindows {
        timezone: "Mars/Olympus".into(),
        ranges: office.ranges.clone(),
        calendar: None,
    };
    assert!(unknown.validate().is_err());
    assert!(
        WeeklyWindows {
            timezone: "UTC".into(),
            ranges: Vec::new(),
            calendar: None,
        }
        .validate()
        .is_err()
    );
}

#[test]
fn transitions_keep_to_the_local_clock_across_daylight_saving() {
    // New York moves to EDT on 2026-03-08: 09:00 is 14:00 UTC before and 13:00 UTC after.
    let daily = WeeklyWindows {
        timezone: "America/New_York".into(),
        ranges: vec![range(EVERY_DAY, "09:00", "17:00")],
        calendar: None,
    };

    assert_eq!(
        daily.transitions(ts("2026-03-07T00:00:00Z"), ts("2026-03-10T00:00:00Z")),
        vec![
            at("2026-03-07T14:00:00Z"),
            at("2026-03-07T22:00:00Z"),
            at("2026-03-08T13:00:00Z"),
            at("2026-03-08T21:00:00Z"),
            at("2026-03-09T13:00:00Z"),
            at("2026-03-09T21:00:00Z"),
        ]
    );
}

#[test]
fn local_times_in_a_gap_or_a_fold_resolve_to_one_instant() {
    let new_york = chrono_tz::America::New_York;
    let skipped = NaiveDate::from_ymd_opt(2026, 3, 8)
        .unwrap()
        .and_hms_opt(2, 30, 0)
        .unwrap();
    let repeated = NaiveDate::from_ymd_opt(2026, 11, 1)
        .unwrap()
        .and_hms_opt(1, 30, 0)
        .unwrap();

    // 02:30 does not exist: the clock goes from 02:00 EST straight to 03:00 EDT (07:00 UTC).
    assert_eq!(
        local_instant(new_york, skipped).timestamp_micros(),
        at("2026-03-08T07:00:00Z")
    );
    // 01:30 happens twice; the first time is EDT.
    assert_eq!(
        local_instant(new_york, repeated).timestamp_micros(),
        at("2026-11-01T05:30:00Z")
    );
}

#[test]
fn a_monthly_window_finds_its_day_in_each_month() {
    // First Sunday of the month, 02:00 for four hours, in Shanghai: 2026-11-01 is a Sunday.
    let maintenance = MonthlyWindow {
        timezone: "Asia/Shanghai".into(),
        day: MonthDay::Weekday {
            ordinal: 1,
            weekday: 0,
        },
        start: clock("02:00"),
        duration_minutes: 240,
    };

    assert!(maintenance.validate().is_ok());
    assert!(!maintenance.contains(ts("2026-10-31T17:59:00Z")));
    assert!(maintenance.contains(ts("2026-10-31T18:00:00Z")));
    assert!(maintenance.contains(ts("2026-10-31T21:59:00Z")));
    assert!(!maintenance.contains(ts("2026-10-31T22:00:00Z")));
    assert_eq!(
        maintenance.transitions(ts("2026-10-25T00:00:00Z"), ts("2026-11-10T00:00:00Z")),
        vec![at("2026-10-31T18:00:00Z"), at("2026-10-31T22:00:00Z")]
    );

    // The 31st falls back to the last day of February; the last Friday of October 2026 is the 30th.
    let end_of_month = MonthlyWindow {
        timezone: "UTC".into(),
        day: MonthDay::Date { day: 31 },
        start: clock("23:00"),
        duration_minutes: 60,
    };
    assert!(end_of_month.contains(ts("2026-02-28T23:30:00Z")));

    let last_friday = MonthlyWindow {
        timezone: "UTC".into(),
        day: MonthDay::Weekday {
            ordinal: -1,
            weekday: 5,
        },
        start: clock("20:00"),
        duration_minutes: 30,
    };
    assert!(last_friday.contains(ts("2026-10-30T20:10:00Z")));
    assert!(!last_friday.contains(ts("2026-10-23T20:10:00Z")));

    // A fifth Monday that a month lacks is skipped: February 2026 has only four Mondays.
    let fifth_monday = MonthDay::Weekday {
        ordinal: 5,
        weekday: 1,
    };
    assert!(fifth_monday.in_month(2026, 2).is_none());
    assert_eq!(
        fifth_monday.in_month(2026, 3),
        NaiveDate::from_ymd_opt(2026, 3, 30)
    );
}

#[test]
fn a_monthly_window_rejects_what_it_cannot_mean() {
    let base = MonthlyWindow {
        timezone: "UTC".into(),
        day: MonthDay::Date { day: 1 },
        start: clock("02:00"),
        duration_minutes: 60,
    };

    for bad in [
        MonthlyWindow {
            day: MonthDay::Date { day: 0 },
            ..base.clone()
        },
        MonthlyWindow {
            day: MonthDay::Weekday {
                ordinal: 6,
                weekday: 1,
            },
            ..base.clone()
        },
        MonthlyWindow {
            day: MonthDay::Weekday {
                ordinal: 1,
                weekday: 7,
            },
            ..base.clone()
        },
        MonthlyWindow {
            duration_minutes: 0,
            ..base.clone()
        },
        MonthlyWindow {
            duration_minutes: MAX_MONTHLY_DURATION_MINUTES + 1,
            ..base.clone()
        },
        MonthlyWindow {
            start: ClockTime::END_OF_DAY,
            ..base.clone()
        },
    ] {
        assert!(bad.validate().is_err(), "{bad:?}");
    }
}

#[test]
fn a_time_condition_holds_inside_or_outside_its_windows() {
    let windows = WeeklyWindows {
        timezone: "UTC".into(),
        ranges: vec![range(EVERY_DAY, "09:00", "18:00")],
        calendar: None,
    };
    let inside = TimeCondition {
        mode: TimeConditionMode::Inside,
        windows: windows.clone(),
    };
    let outside = TimeCondition {
        mode: TimeConditionMode::Outside,
        windows,
    };

    assert!(inside.holds(ts("2026-05-18T10:00:00Z")));
    assert!(!outside.holds(ts("2026-05-18T10:00:00Z")));
    assert!(outside.holds(ts("2026-05-18T20:00:00Z")));

    let json = serde_json::to_value(&inside).unwrap();
    assert_eq!(json["mode"], "inside");
    assert_eq!(json["timezone"], "UTC");
    assert_eq!(json["ranges"][0]["end"], "18:00");
}

/// 一份只有国庆（10 月 1 日周四、2 日周五放假）和 10 月 10 日（周六）补班的日历，记进缓存。
fn national_day() -> crate::shared::ids::Id {
    use crate::domain::alerting::holiday::{self, DayKind, HolidayCalendar, HolidayDay};

    let day = |date: &str, kind| HolidayDay {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        kind,
        name: String::new(),
    };
    let calendar = HolidayCalendar {
        id: crate::shared::ids::Id::new(),
        org_id: crate::shared::ids::Id::from_string("org"),
        name: "China".into(),
        description: String::new(),
        days: vec![
            day("2026-10-01", DayKind::Holiday),
            day("2026-10-02", DayKind::Holiday),
            day("2026-10-10", DayKind::Workday),
        ],
        created_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    };

    holiday::remember(&calendar);
    calendar.id
}

#[test]
fn holidays_leave_office_hours_and_make_up_workdays_count_as_monday() {
    let calendar_id = national_day();
    let office = WeeklyWindows {
        timezone: "Asia/Shanghai".into(),
        ranges: vec![range(0b011_1110, "09:00", "18:00")],
        calendar: Some(CalendarUse {
            calendar_id,
            holidays: HolidayTreatment::Exclude,
        }),
    };

    // 10:00 in Shanghai is 02:00 UTC.
    assert!(!office.contains(ts("2026-10-01T02:00:00Z")));
    assert!(!office.contains(ts("2026-10-02T02:00:00Z")));
    assert!(office.contains(ts("2026-10-05T02:00:00Z")));
    assert!(office.contains(ts("2026-10-10T02:00:00Z")));
    assert!(!office.contains(ts("2026-10-11T02:00:00Z")));

    // From Wednesday 30 September to Monday 5 October office hours open only on the Wednesday.
    assert_eq!(
        office.transitions(ts("2026-09-30T00:00:00Z"), ts("2026-10-05T00:00:00Z")),
        vec![at("2026-09-30T01:00:00Z"), at("2026-09-30T10:00:00Z")]
    );
}

#[test]
fn a_night_range_belongs_to_the_day_it_starts_and_only_counts_holidays() {
    let calendar_id = national_day();
    let nights = WeeklyWindows {
        timezone: "UTC".into(),
        ranges: vec![range(EVERY_DAY, "22:00", "06:00")],
        calendar: Some(CalendarUse {
            calendar_id: calendar_id.clone(),
            holidays: HolidayTreatment::Exclude,
        }),
    };

    // The night of Friday 2 October is a holiday's: Saturday 02:00 is not in it.
    assert!(!nights.contains(ts("2026-10-03T02:00:00Z")));
    assert!(nights.contains(ts("2026-10-04T02:00:00Z")));

    let holidays = WeeklyWindows {
        timezone: "UTC".into(),
        ranges: vec![ALL_DAY],
        calendar: Some(CalendarUse {
            calendar_id,
            holidays: HolidayTreatment::Only,
        }),
    };

    assert!(holidays.contains(ts("2026-10-01T00:00:00Z")));
    assert!(holidays.contains(ts("2026-10-02T23:59:00Z")));
    assert!(!holidays.contains(ts("2026-10-03T00:00:00Z")));
    assert!(!holidays.contains(ts("2026-10-10T12:00:00Z")));
    assert_eq!(
        holidays.transitions(ts("2026-09-30T12:00:00Z"), ts("2026-10-04T00:00:00Z")),
        vec![at("2026-10-01T00:00:00Z"), at("2026-10-03T00:00:00Z")]
    );

    let json = serde_json::to_value(&holidays).unwrap();
    assert_eq!(json["calendar"]["holidays"], "only");
}
