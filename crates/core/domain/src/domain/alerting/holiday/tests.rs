// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("date")
}

fn day(text: &str, kind: DayKind) -> HolidayDay {
    HolidayDay {
        date: date(text),
        kind,
        name: format!("  {text}  "),
    }
}

fn calendar(days: Vec<HolidayDay>) -> HolidayCalendar {
    HolidayCalendar {
        id: Id::new(),
        org_id: Id::from_string("org"),
        name: "  China  ".into(),
        description: String::new(),
        days,
        created_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

#[test]
fn a_calendar_is_trimmed_and_sorted_and_names_a_day_once() {
    let mut sorted = calendar(vec![
        day("2026-10-02", DayKind::Holiday),
        day("2026-10-01", DayKind::Holiday),
        day("2026-10-10", DayKind::Workday),
    ]);

    sorted.normalize().unwrap();

    assert_eq!(sorted.name, "China");
    assert_eq!(
        sorted.days.iter().map(|day| day.date).collect::<Vec<_>>(),
        vec![date("2026-10-01"), date("2026-10-02"), date("2026-10-10")]
    );
    assert_eq!(sorted.days[0].name, "2026-10-01");

    let mut twice = calendar(vec![
        day("2026-10-01", DayKind::Holiday),
        day("2026-10-01", DayKind::Workday),
    ]);

    assert!(twice.normalize().is_err());

    let mut unnamed = calendar(Vec::new());
    unnamed.name = " ".into();

    assert!(unnamed.normalize().is_err());
}

#[test]
fn an_ics_file_gives_each_day_of_its_events_and_spots_make_up_workdays() {
    let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261001\r\n\
                DTEND;VALUE=DATE:20261004\r\nSUMMARY:国庆节\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\n\
                DTSTART;VALUE=DATE:20261010\r\nSUMMARY:国庆节（班）\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\n\
                DTSTART:20261225T000000Z\r\nSUMMARY:Christmas\r\n  Day\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    let days = parse_ics(text).unwrap();

    assert_eq!(
        days.iter()
            .map(|day| (day.date.to_string(), day.kind, day.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("2026-10-01".to_string(), DayKind::Holiday, "国庆节"),
            ("2026-10-02".to_string(), DayKind::Holiday, "国庆节"),
            ("2026-10-03".to_string(), DayKind::Holiday, "国庆节"),
            ("2026-10-10".to_string(), DayKind::Workday, "国庆节（班）"),
            ("2026-12-25".to_string(), DayKind::Holiday, "Christmas Day"),
        ]
    );
    assert!(parse_ics("just text").is_err());
}

#[test]
fn the_book_answers_for_the_calendars_it_was_told_about() {
    let mut china = calendar(vec![
        day("2026-10-01", DayKind::Holiday),
        day("2026-10-10", DayKind::Workday),
    ]);

    china.normalize().unwrap();
    remember(&china);

    assert_eq!(
        day_kind(&china.id, date("2026-10-01")),
        Some(DayKind::Holiday)
    );
    assert_eq!(
        day_kind(&china.id, date("2026-10-10")),
        Some(DayKind::Workday)
    );
    assert_eq!(day_kind(&china.id, date("2026-10-11")), None);
    assert!(belongs_to(&china.id, &Id::from_string("org")));
    assert!(!belongs_to(&china.id, &Id::from_string("other")));

    forget(&china.id);

    assert_eq!(day_kind(&china.id, date("2026-10-01")), None);
}
