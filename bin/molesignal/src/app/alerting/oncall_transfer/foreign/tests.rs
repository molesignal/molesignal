// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对照测试：fixture 按两家官方文档的响应样例手写，导入后用排班的展开逻辑算若干时刻的值班人，和对方的渲染结果
//! （PagerDuty 的 `final_schedule`、Opsgenie 的 timeline）比。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::*;
use crate::{
    app::alerting::oncall_transfer::{
        Directory, Existing, Names, TransferItem, Write, plan, portable_of,
        schedules::{SCHEDULE_DROP, ScheduleScope, schedule_from_item},
    },
    domain::alerting::{
        schedule::Schedule,
        time_window::{ClockTime, EVERY_DAY},
    },
    shared::{Result, ids::Id},
};

const PAGERDUTY_LAYERS: &str = include_str!("fixtures/pagerduty_layers.json");
const PAGERDUTY_SAMPLE: &str = include_str!("fixtures/pagerduty_doc_sample.json");
const PAGERDUTY_SHIFT_BASED: &str = include_str!("fixtures/pagerduty_shift_based.json");
const PAGERDUTY_SHIFT_BASED_SAMPLE: &str =
    include_str!("fixtures/pagerduty_shift_based_doc_sample.json");
const OPSGENIE_ROTATIONS: &str = include_str!("fixtures/opsgenie_rotations.json");
const OPSGENIE_GET_SAMPLE: &str = include_str!("fixtures/opsgenie_get_sample.json");
const OPSGENIE_LIST_SAMPLE: &str = include_str!("fixtures/opsgenie_list_sample.json");

fn at(text: &str) -> TimestampMicros {
    TimestampMicros(parse_time(text).unwrap_or_else(|| panic!("`{text}` is a time")))
}

fn fixture(text: &str) -> Value {
    serde_json::from_str(text).expect("a fixture is JSON")
}

const PEOPLE: [&str; 9] = [
    "alice", "bob", "carol", "dan", "erin", "anna", "ben", "carl", "dora",
];

/// 每个人的 id 就是 `id-名字`，方便把值班人换回来。
fn directory() -> Directory {
    Directory {
        users: Names::new(
            PEOPLE.iter().map(|name| {
                (
                    Id::from_string(format!("id-{name}")),
                    format!("{name}@example.com"),
                )
            }),
            true,
        ),
        teams: Names::new(
            [(Id::from_string("team-payments"), "Payments".to_string())],
            false,
        ),
        ..Directory::default()
    }
}

struct Imported {
    items: Vec<TransferItem>,
    writes: Vec<Write<Schedule>>,
    findings: Vec<Finding>,
}

/// 走和接口一样的流程：读对方的文件 → 预演 → 写之前的排班。
fn import_into(
    bundle: &Value,
    now: TimestampMicros,
    existing: &[Existing],
    current: &[Schedule],
) -> Imported {
    let directory = directory();
    let source = read(bundle, &directory, now).expect("recognized");
    let org = Id::from_string("org");
    let user = Id::from_string("importer");
    let scope = ScheduleScope {
        org_id: &org,
        user_id: &user,
        now,
    };

    let (mut items, writes) = plan(
        source.incoming(),
        existing,
        &directory,
        false,
        |value, id| -> Result<Schedule> {
            let before = id.and_then(|id| current.iter().find(|schedule| schedule.id == *id));

            schedule_from_item(value, id, before, &scope)
        },
        |schedule| portable_of(schedule, &SCHEDULE_DROP, &directory),
    );
    let findings = source.annotate(&mut items);

    Imported {
        items,
        writes,
        findings,
    }
}

fn import(bundle: &Value, now: TimestampMicros) -> Imported {
    import_into(bundle, now, &[], &[])
}

/// 这个时刻的值班人（邮箱）。
fn on_call(schedule: &Schedule, at: TimestampMicros) -> Option<String> {
    schedule
        .who_is_on_call(at)
        .map(|id| format!("{}@example.com", id.0.trim_start_matches("id-")))
}

fn codes(findings: &[Finding]) -> Vec<&'static str> {
    findings.iter().map(|finding| finding.code).collect()
}

/// 一段班里取开始、中间和结束前一分钟。
fn samples(start: i64, end: i64) -> [TimestampMicros; 3] {
    [start, start + (end - start) / 2, end - 60_000_000].map(TimestampMicros)
}

// ---- PagerDuty ----

#[test]
fn pagerduty_layers_keep_who_is_on_call() {
    let bundle = fixture(PAGERDUTY_LAYERS);
    let imported = import(&bundle, at("2026-10-26T08:00:00-04:00"));

    assert_eq!(imported.items.len(), 1);

    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(item.blockers.is_empty(), "{:?}", item.blockers);
    assert_eq!(
        codes(&item.findings),
        vec!["rotation_ended", "rendered_matched", "overrides_left_out"]
    );
    assert_eq!(imported.findings, vec![Finding::new("schedules_only")]);

    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.timezone, "America/New_York");
    assert_eq!(schedule.team_id, Some(Id::from_string("team-payments")));
    assert_eq!(
        schedule
            .rotations
            .iter()
            .map(|rotation| rotation.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Weekend cover", "Nights", "Base"]
    );

    let rendered = &bundle["schedules"][0];
    let names: HashMap<&str, &str> = bundle["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| {
            (
                user["id"].as_str().unwrap(),
                user["email"].as_str().unwrap(),
            )
        })
        .collect();
    let overrides = rendered["overrides_subschedule"]["rendered_schedule_entries"]
        .as_array()
        .unwrap();
    let mut checked = 0;

    for entry in rendered["final_schedule"]["rendered_schedule_entries"]
        .as_array()
        .unwrap()
    {
        let (start, end) = (
            at(entry["start"].as_str().unwrap()),
            at(entry["end"].as_str().unwrap()),
        );

        if overrides
            .iter()
            .any(|cover| at(cover["start"].as_str().unwrap()) == start)
        {
            continue;
        }

        let expected = names[entry["user"]["id"].as_str().unwrap()];

        for sample in samples(start.0, end.0) {
            assert_eq!(
                on_call(schedule, sample).as_deref(),
                Some(expected),
                "at {:?}",
                DateTime::<Utc>::from_timestamp_micros(sample.0)
            );
            checked += 1;
        }
    }

    assert_eq!(checked, 19 * 3);

    // 临时替班不导入：dan 替的那几个小时还是轮值上的 bob。
    assert_eq!(
        on_call(schedule, at("2026-10-28T10:00:00-04:00")).as_deref(),
        Some("bob@example.com")
    );

    // 夏令时结束后，交接还在本地时间：周一 09:00 是 UTC 14:00，不是 13:00。
    assert_eq!(
        on_call(schedule, at("2026-11-02T13:30:00Z")).as_deref(),
        Some("carol@example.com")
    );
    assert_eq!(
        on_call(schedule, at("2026-11-02T14:00:00Z")).as_deref(),
        Some("alice@example.com")
    );
}

#[test]
fn pagerduty_layers_in_the_wrong_order_are_caught_by_the_final_schedule() {
    let mut bundle = fixture(PAGERDUTY_LAYERS);

    bundle["schedules"][0]["schedule_layers"]
        .as_array_mut()
        .unwrap()
        .reverse();

    let imported = import(&bundle, at("2026-10-26T08:00:00-04:00"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(item.blockers[0].code, "rendered_mismatch");
    assert_eq!(
        item.blockers[0].params["at"],
        "2026-10-26 18:00 America/New_York"
    );
    assert_eq!(item.blockers[0].params["expected"], "erin@example.com");
    assert_eq!(item.blockers[0].params["actual"], "bob@example.com");
}

#[test]
fn the_published_pagerduty_sample_reads() {
    let mut bundle = fixture(PAGERDUTY_SAMPLE);

    // 响应里的用户没有邮箱：没有 `users` 时带不过来。
    let imported = import(&bundle, at("2015-11-10T07:00:00-05:00"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["user_without_email"]);
    assert_eq!(item.blockers[0].params["user"], "Regina Phalange");
    assert_eq!(
        codes(&imported.findings),
        vec!["schedules_only", "shift_based_not_listed"]
    );

    bundle["users"] = json!([{"id": "PXPGF42", "email": "alice@example.com"}]);

    let imported = import(&bundle, at("2015-11-10T07:00:00-05:00"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rendered_matched"));
    // 团队 Engineering 这里没有，不影响谁值班。
    assert!(codes(&item.findings).contains(&"team_not_carried"));

    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.description, "Rotation schedule for engineering");
    assert_eq!(schedule.team_id, None);
    assert_eq!(schedule.rotations[0].id, Id::from_string("PG68P1M"));
    assert!(matches!(schedule.rotations[0].kind, RotationKind::Daily));
    assert_eq!(
        schedule.rotations[0].active_windows[0].start,
        ClockTime::new(8, 0).unwrap()
    );
    assert_eq!(
        schedule.rotations[0].active_windows[0].end,
        ClockTime::new(17, 0).unwrap()
    );

    for (time, expected) in [
        ("2015-11-10T08:00:00-05:00", Some("alice@example.com")),
        ("2015-11-10T16:59:00-05:00", Some("alice@example.com")),
        ("2015-11-10T17:00:00-05:00", None),
        ("2015-11-10T07:59:00-05:00", None),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }
}

fn pagerduty_layer(layer: Value) -> Value {
    json!({
        "users": [
            {"id": "PALICE1", "email": "alice@example.com"},
            {"id": "PBOB001", "email": "bob@example.com"},
            {"id": "PCAROL1", "email": "carol@example.com"}
        ],
        "schedule": {
            "type": "schedule",
            "name": "Later",
            "time_zone": "UTC",
            "schedule_layers": [layer]
        }
    })
}

fn three_people() -> Value {
    json!([
        {"user": {"id": "PALICE1"}},
        {"user": {"id": "PBOB001"}},
        {"user": {"id": "PCAROL1"}}
    ])
}

#[test]
fn a_pagerduty_layer_that_starts_later_turns_to_whoever_is_due_then() {
    // 轮换从 1 月 1 日算，每天交接；这一层 1 月 11 日 12:00 才生效，那天是第 10 班（bob）。
    let bundle = pagerduty_layer(json!({
        "id": "PLATER",
        "name": "Later",
        "start": "2027-01-11T12:00:00Z",
        "end": "2027-01-20T00:00:00Z",
        "rotation_virtual_start": "2027-01-01T00:00:00Z",
        "rotation_turn_length_seconds": 86400,
        "users": three_people(),
        "restrictions": []
    }));
    let imported = import(&bundle, at("2027-01-05T00:00:00Z"));
    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.rotations.len(), 2);

    for (time, expected) in [
        ("2027-01-11T11:59:00Z", None),
        ("2027-01-11T12:00:00Z", Some("bob@example.com")),
        ("2027-01-11T23:59:00Z", Some("bob@example.com")),
        ("2027-01-12T00:00:00Z", Some("carol@example.com")),
        ("2027-01-13T00:00:00Z", Some("alice@example.com")),
        // 1 月 19 日是第 18 班，又轮回 alice。
        ("2027-01-19T23:59:00Z", Some("alice@example.com")),
        ("2027-01-20T00:00:00Z", None),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }

    // 正好在交接时刻生效的不用多一条。
    let bundle = pagerduty_layer(json!({
        "id": "PLATER",
        "start": "2027-01-11T00:00:00Z",
        "rotation_virtual_start": "2027-01-01T00:00:00Z",
        "rotation_turn_length_seconds": 86400,
        "users": three_people()
    }));
    let imported = import(&bundle, at("2027-01-05T00:00:00Z"));
    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.rotations.len(), 1);
    assert_eq!(schedule.rotations[0].name, "Layer 1");
    assert_eq!(on_call(schedule, at("2027-01-10T23:59:00Z")), None);
    assert_eq!(
        on_call(schedule, at("2027-01-11T00:00:00Z")).as_deref(),
        Some("bob@example.com")
    );
    assert_eq!(
        on_call(schedule, at("2027-01-12T00:00:00Z")).as_deref(),
        Some("carol@example.com")
    );
}

#[test]
fn a_pagerduty_rotation_that_began_before_the_layer_started_needs_nothing_more() {
    // 生效之前的那一段已经过去，轮换的起点照旧。
    let bundle = pagerduty_layer(json!({
        "id": "PPAST",
        "start": "2026-01-11T12:00:00Z",
        "rotation_virtual_start": "2026-01-01T00:00:00Z",
        "rotation_turn_length_seconds": 86400,
        "users": three_people()
    }));
    let imported = import(&bundle, at("2026-06-01T00:00:00Z"));
    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.rotations.len(), 1);
    assert_eq!(schedule.rotations[0].start_at, at("2026-01-01T00:00:00Z"));
}

#[test]
fn what_pagerduty_layers_cannot_carry_is_listed() {
    for (layer, code) in [
        (
            json!({"start": "2027-01-01T00:00:00Z", "rotation_virtual_start": "2027-02-01T00:00:00Z",
                   "rotation_turn_length_seconds": 86400, "users": three_people()}),
            "starts_before_handoff",
        ),
        (
            json!({"start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
                   "rotation_turn_length_seconds": 0, "users": three_people()}),
            "rotation_length_invalid",
        ),
        (
            json!({"start": "not a time", "rotation_virtual_start": "2026-01-01T00:00:00Z",
                   "rotation_turn_length_seconds": 86400, "users": three_people()}),
            "time_invalid",
        ),
        (
            json!({"start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
                   "rotation_turn_length_seconds": 86400, "users": three_people(),
                   "restrictions": [{"type": "monthly_restriction", "start_time_of_day": "08:00:00", "duration_seconds": 3600}]}),
            "restriction_unsupported",
        ),
        (
            json!({"start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
                   "rotation_turn_length_seconds": 86400, "users": three_people(),
                   "restrictions": [{"type": "daily_restriction", "start_time_of_day": "08:00:30", "duration_seconds": 3600}]}),
            "restriction_unsupported",
        ),
    ] {
        let imported = import(&pagerduty_layer(layer), at("2026-06-01T00:00:00Z"));
        let item = &imported.items[0];

        assert_eq!(item.action, ImportAction::Error, "{code}");
        assert_eq!(codes(&item.blockers), vec![code]);
    }
}

#[test]
fn restrictions_rendered_in_another_timezone_are_refused() {
    // `GET /schedules` 不传 `time_zone` 时按调用者的时区渲染限制，排班自己的时区不用 +09:00。
    let mut bundle = pagerduty_layer(json!({
        "start": "2026-01-01T09:00:00+09:00",
        "rotation_virtual_start": "2026-01-01T09:00:00+09:00",
        "rotation_turn_length_seconds": 86400,
        "users": three_people(),
        "restrictions": [{"type": "daily_restriction", "start_time_of_day": "09:00:00", "duration_seconds": 28800}]
    }));

    bundle["schedule"]["time_zone"] = json!("America/New_York");

    let imported = import(&bundle, at("2026-06-01T00:00:00Z"));

    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["rendered_in_other_zone"]
    );
}

#[test]
fn pagerduty_shift_based_schedules_the_file_only_names_are_skipped() {
    let bundle = json!({
        "users": [{"id": "PALICE1", "email": "alice@example.com"}],
        "schedules": [
            {"id": "PS1A2B3C", "type": "schedule_v3", "name": "Engineering On-Call",
             "time_zone": "America/New_York", "self": "https://api.pagerduty.com/v3/schedules/PS1A2B3C",
             "rotations": [{"id": "ABC", "type": "schedule_rotation", "events": []}]},
            {"id": "PS0A0B0C", "type": "schedule_v3_reference", "summary": "Also in the list"},
            {"type": "schedule", "name": "Layered", "time_zone": "UTC", "schedule_layers": [{
                "start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
                "rotation_turn_length_seconds": 604800, "users": [{"user": {"id": "PALICE1"}}]
            }]}
        ],
        "shift_based_schedules": [
            {"id": "PS1A2B3C", "type": "schedule_v3_reference", "summary": "Engineering On-Call"},
            {"id": "PS0A0B0C", "type": "schedule_v3_reference", "summary": "Also in the list"},
            {"id": "PS4D5E6F", "type": "schedule_v3_reference", "summary": "Shifted"},
            {"id": "PS7G8H9I", "type": 7}
        ]
    });
    let imported = import(&bundle, at("2026-06-01T00:00:00Z"));
    let actions: Vec<(&str, ImportAction)> = imported
        .items
        .iter()
        .map(|item| (item.name.as_str(), item.action))
        .collect();

    assert_eq!(
        actions,
        vec![
            ("Engineering On-Call", ImportAction::Create),
            ("Layered", ImportAction::Create),
            ("Also in the list", ImportAction::Skip),
            ("Shifted", ImportAction::Skip),
            ("PS7G8H9I", ImportAction::Skip),
        ]
    );
    assert_eq!(
        codes(&imported.items[2].findings),
        vec!["shift_based_not_fetched"]
    );
    assert_eq!(imported.writes.len(), 2);
    assert_eq!(imported.findings, vec![Finding::new("schedules_only")]);
}

#[test]
fn a_pagerduty_list_without_layers_or_a_real_timezone_is_refused() {
    let imported = import(
        &json!({"schedules": [{"type": "schedule", "name": "Brief", "time_zone": "UTC"}]}),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(codes(&imported.items[0].blockers), vec!["layers_missing"]);

    let imported = import(
        &json!({"schedule": {"type": "schedule", "name": "Named zone",
                             "time_zone": "Eastern Time (US & Canada)", "schedule_layers": []}}),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(codes(&imported.items[0].blockers), vec!["timezone_unknown"]);
}

// ---- PagerDuty shift-based ----

const A_MONDAY: &str = "2026-01-05T09:00:00Z";
const A_WEEK_LATER: &str = "2026-01-12T09:00:00Z";
const A_MONDAY_EVENING: &str = "2026-01-05T17:00:00Z";

fn user_members(ids: &[&str]) -> Value {
    ids.iter()
        .map(|id| json!({"type": "user_member", "user_id": id}))
        .collect()
}

/// alice、bob、carol 轮流；时区都是 UTC。
fn shift_based_event(name: &str, recurrence: &str, start: &str, end: &str, shifts: i64) -> Value {
    json!({
        "id": name,
        "type": "schedule_event",
        "name": name,
        "start_time": {"date_time": start, "time_zone": "UTC"},
        "end_time": {"date_time": end, "time_zone": "UTC"},
        "effective_since": start,
        "effective_until": null,
        "recurrence": [recurrence],
        "assignment_strategy": {
            "type": "rotating_member_assignment_strategy",
            "shifts_per_member": shifts,
            "members": user_members(&["PALICE1", "PBOB001", "PCAROL1"])
        }
    })
}

fn weekly_event(name: &str) -> Value {
    shift_based_event(name, "RRULE:FREQ=WEEKLY", A_MONDAY, A_WEEK_LATER, 1)
}

fn shift_based_bundle(events: Vec<Value>) -> Value {
    json!({
        "users": [
            {"id": "PALICE1", "email": "alice@example.com"},
            {"id": "PBOB001", "email": "bob@example.com"},
            {"id": "PCAROL1", "email": "carol@example.com"}
        ],
        "schedule": {
            "id": "PS1A2B3",
            "type": "schedule_v3",
            "name": "Shifts",
            "time_zone": "UTC",
            "rotations": [{"id": "R1", "type": "schedule_rotation", "events": events}]
        }
    })
}

fn assignment(start: &str, end: &str, user: Option<&str>, source: &str) -> Value {
    let member = match user {
        Some(id) => json!({"type": "user_member", "user_id": id}),
        None => json!({"type": "empty_member"}),
    };

    json!({
        "type": "computed_shift_assignment",
        "start_time": start,
        "end_time": end,
        "member": member,
        "source": {"type": source}
    })
}

/// 一周一班的排班，带上对方排出来的值班表。
fn weekly_with_final_schedule(assignments: Vec<Value>) -> Value {
    let mut bundle = shift_based_bundle(vec![weekly_event("Weekly")]);

    bundle["schedule"]["final_schedule"] = json!({
        "type": "final_schedule",
        "rendered_coverage_percentage": 100,
        "computed_shift_assignments": assignments
    });
    bundle
}

#[test]
fn pagerduty_shift_based_events_keep_who_is_on_call() {
    let bundle = fixture(PAGERDUTY_SHIFT_BASED);
    let imported = import(&bundle, at("2026-10-26T08:00:00-04:00"));
    let actions: Vec<(&str, ImportAction)> = imported
        .items
        .iter()
        .map(|item| (item.name.as_str(), item.action))
        .collect();

    assert_eq!(
        actions,
        vec![
            ("Payments on-call", ImportAction::Create),
            ("Platform primary", ImportAction::Create),
            ("Not fetched", ImportAction::Skip),
        ]
    );
    assert_eq!(imported.findings, vec![Finding::new("schedules_only")]);
    assert_eq!(
        codes(&imported.items[0].findings),
        vec!["rotation_ended", "rendered_matched", "overrides_left_out"]
    );
    assert_eq!(codes(&imported.items[1].findings), vec!["rendered_matched"]);

    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.timezone, "America/New_York");
    assert_eq!(schedule.description, "Payments rotation");
    assert_eq!(schedule.team_id, Some(Id::from_string("team-payments")));
    assert_eq!(
        schedule
            .rotations
            .iter()
            .map(|rotation| rotation.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Business hours", "Weeknights", "Weekends"]
    );

    // 每人五个工作日是每人一周。
    assert!(matches!(schedule.rotations[0].kind, RotationKind::Weekly));
    assert!(matches!(schedule.rotations[1].kind, RotationKind::Weekly));
    assert!(matches!(schedule.rotations[2].kind, RotationKind::Weekly));

    let emails: HashMap<&str, &str> = bundle["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| {
            (
                user["id"].as_str().unwrap(),
                user["email"].as_str().unwrap(),
            )
        })
        .collect();
    let mut checked = 0;

    for (index, schedule) in imported.writes.iter().map(|write| &write.value).enumerate() {
        for entry in bundle["schedules"][index]["final_schedule"]["computed_shift_assignments"]
            .as_array()
            .unwrap()
        {
            // 替班不导入，它占的时段下面单独看。
            if entry["source"]["type"] != "schedule_rotation" {
                continue;
            }

            let expected = emails[entry["member"]["user_id"].as_str().unwrap()];
            let (start, end) = (
                at(entry["start_time"].as_str().unwrap()),
                at(entry["end_time"].as_str().unwrap()),
            );

            for sample in samples(start.0, end.0) {
                assert_eq!(
                    on_call(schedule, sample).as_deref(),
                    Some(expected),
                    "at {:?}",
                    DateTime::<Utc>::from_timestamp_micros(sample.0)
                );
                checked += 1;
            }
        }
    }

    assert_eq!(checked, 23 * 3);

    // 临时替班不导入：dan 替的那几个小时还是轮值上的 alice。
    assert_eq!(
        on_call(schedule, at("2026-10-28T15:00:00Z")).as_deref(),
        Some("alice@example.com")
    );

    // 夏令时结束后，交接还在本地时间：周一 09:00 是 UTC 14:00，不是 13:00。
    assert_eq!(
        on_call(schedule, at("2026-11-02T13:30:00Z")).as_deref(),
        Some("erin@example.com")
    );
    assert_eq!(
        on_call(schedule, at("2026-11-02T14:00:00Z")).as_deref(),
        Some("bob@example.com")
    );
}

#[test]
fn a_shift_based_final_schedule_that_disagrees_stops_the_import() {
    let mut bundle = fixture(PAGERDUTY_SHIFT_BASED);

    bundle["schedules"][1]["rotations"][0]["events"][0]["assignment_strategy"]["members"] =
        user_members(&["PBOB001", "PALICE1", "PCAROL1"]);

    let imported = import(&bundle, at("2026-10-26T08:00:00-04:00"));
    let item = &imported.items[1];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(item.blockers[0].code, "rendered_mismatch");
    assert_eq!(item.blockers[0].params["expected"], "alice@example.com");
    assert_eq!(item.blockers[0].params["actual"], "bob@example.com");
    assert_eq!(imported.items[0].action, ImportAction::Create);
}

#[test]
fn importing_a_shift_based_file_again_changes_nothing() {
    let bundle = fixture(PAGERDUTY_SHIFT_BASED);
    let now = at("2026-10-26T08:00:00-04:00");
    let directory = directory();
    let imported = import(&bundle, now);
    let saved: Vec<Schedule> = imported
        .writes
        .iter()
        .enumerate()
        .map(|(index, write)| Schedule {
            id: Id::from_string(format!("saved-{index}")),
            ..write.value.clone()
        })
        .collect();
    let existing: Vec<Existing> = saved
        .iter()
        .map(|schedule| Existing {
            id: schedule.id.clone(),
            name: schedule.name.clone(),
            portable: portable_of(schedule, &SCHEDULE_DROP, &directory),
        })
        .collect();
    let again = import_into(&bundle, now, &existing, &saved);

    assert_eq!(again.items[0].action, ImportAction::Unchanged);
    assert_eq!(again.items[1].action, ImportAction::Unchanged);
    assert!(again.writes.is_empty());
}

#[test]
fn the_published_shift_based_sample_reads() {
    let mut bundle = fixture(PAGERDUTY_SHIFT_BASED_SAMPLE);

    // 响应里的成员只有 id：没有 `users` 时带不过来。
    let imported = import(&bundle, at("2026-04-29T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["user_without_email"]);
    assert_eq!(item.blockers[0].params["user"], "PUSER01");
    assert_eq!(
        item.blockers[0].params["rotation"],
        "Weekday business hours"
    );
    assert_eq!(
        codes(&imported.findings),
        vec!["schedules_only", "shift_based_not_listed"]
    );

    bundle["users"] = json!([{"id": "PUSER01", "email": "alice@example.com"}]);

    let imported = import(&bundle, at("2026-04-29T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rendered_matched"));

    let schedule = &imported.writes[0].value;
    let rotation = &schedule.rotations[0];

    assert_eq!(schedule.timezone, "America/Los_Angeles");
    assert_eq!(rotation.id, Id::from_string("ABCDEFGHIJKLMNOPQRSTUVWXY3"));
    assert_eq!(rotation.name, "Weekday business hours");
    assert_eq!(rotation.active_windows[0].weekday_mask, 0b11_1110);
    assert_eq!(
        rotation.active_windows[0].start,
        ClockTime::new(9, 0).unwrap()
    );
    assert_eq!(
        rotation.active_windows[0].end,
        ClockTime::new(17, 0).unwrap()
    );

    for (time, expected) in [
        ("2026-04-29T15:59:00Z", None),
        ("2026-04-29T16:00:00Z", Some("alice@example.com")),
        ("2026-04-29T23:59:00Z", Some("alice@example.com")),
        ("2026-04-30T00:00:00Z", None),
        // 周六。
        ("2026-05-02T18:00:00Z", None),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }
}

#[test]
fn the_published_shift_based_event_examples_read() {
    let bundle = |event: Value| {
        let mut bundle = shift_based_bundle(vec![event]);

        bundle["users"] = json!([
            {"id": "PUSER123", "email": "alice@example.com"},
            {"id": "PUSER456", "email": "bob@example.com"}
        ]);
        bundle["schedule"]["time_zone"] = json!("America/New_York");
        bundle
    };
    let two_people = user_members(&["PUSER123", "PUSER456"]);

    // OpenAPI 的 `rotatingWeekly`：第一班就跨了夏令时切换，实际经过的时间正好七天。
    let rotating = bundle(json!({
        "name": "Weekly On-Call",
        "start_time": {"date_time": "2025-03-03T09:00:00Z", "time_zone": "America/New_York"},
        "end_time": {"date_time": "2025-03-10T09:00:00Z", "time_zone": "America/New_York"},
        "effective_since": "2025-03-03T00:00:00Z",
        "recurrence": ["RRULE:FREQ=WEEKLY"],
        "assignment_strategy": {
            "type": "rotating_member_assignment_strategy",
            "shifts_per_member": 1,
            "members": two_people
        }
    }));
    let imported = import(&rotating, at("2025-03-01T00:00:00Z"));
    let schedule = &imported.writes[0].value;

    assert_eq!(
        imported.items[0].action,
        ImportAction::Create,
        "{:?}",
        imported.items[0]
    );
    assert!(matches!(schedule.rotations[0].kind, RotationKind::Weekly));
    assert!(schedule.rotations[0].active_windows.is_empty());

    // 交接在纽约时间周一 04:00。
    for (time, expected) in [
        ("2025-03-03T08:59:00Z", None),
        ("2025-03-03T09:00:00Z", Some("alice@example.com")),
        ("2025-03-10T07:59:00Z", Some("alice@example.com")),
        ("2025-03-10T08:00:00Z", Some("bob@example.com")),
        ("2025-03-17T08:00:00Z", Some("alice@example.com")),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }

    // OpenAPI 的 `everyMember`：还在生效时带不过来，结束以后就只是跳过。
    let every = bundle(json!({
        "name": "All-Hands Coverage",
        "start_time": {"date_time": "2025-03-03T00:00:00Z", "time_zone": "UTC"},
        "end_time": {"date_time": "2025-03-04T00:00:00Z", "time_zone": "UTC"},
        "effective_since": "2025-03-03T00:00:00Z",
        "effective_until": "2025-06-01T00:00:00Z",
        "recurrence": ["RRULE:FREQ=WEEKLY;BYDAY=MO"],
        "assignment_strategy": {
            "type": "every_member_assignment_strategy",
            "members": two_people
        }
    }));
    let imported = import(&every, at("2025-03-01T00:00:00Z"));

    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["every_member_on_call"]
    );

    let imported = import(&every, at("2025-07-01T00:00:00Z"));

    assert_eq!(imported.items[0].action, ImportAction::Create);
    assert!(codes(&imported.items[0].findings).contains(&"rotation_ended"));
    assert!(imported.writes[0].value.rotations.is_empty());
}

#[test]
fn shift_based_recurrences_become_rotations() {
    let two_weeks = "Custom { period_secs: 1209600 }";
    let three_days = "Custom { period_secs: 259200 }";
    let half_a_day = "Custom { period_secs: 43200 }";
    let evening = A_MONDAY_EVENING;
    let next_day = "2026-01-06T09:00:00Z";
    let two_weeks_later = "2026-01-19T09:00:00Z";
    let weekdays = "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR";
    let all_days = "RRULE:FREQ=WEEKLY;BYDAY=SU,MO,TU,WE,TH,FR,SA";
    let weekdays_daily = "RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR";
    let always: &[(u8, &str, &str)] = &[];
    let monday: &[(u8, &str, &str)] = &[(0b10, "09:00", "17:00")];
    let business_hours: &[(u8, &str, &str)] = &[(0b11_1110, "09:00", "17:00")];
    let every_day: &[(u8, &str, &str)] = &[(EVERY_DAY, "09:00", "17:00")];

    let cases = [
        // 一班接一班，一直有人值班。
        ("RRULE:FREQ=WEEKLY", A_WEEK_LATER, 1, "Weekly", always),
        ("RRULE:FREQ=WEEKLY", A_WEEK_LATER, 2, two_weeks, always),
        (
            "RRULE:FREQ=WEEKLY;INTERVAL=2",
            two_weeks_later,
            1,
            two_weeks,
            always,
        ),
        (
            "RRULE:FREQ=WEEKLY;BYDAY=MO",
            A_WEEK_LATER,
            1,
            "Weekly",
            always,
        ),
        (
            "rrule:freq=weekly;wkst=mo",
            A_WEEK_LATER,
            1,
            "Weekly",
            always,
        ),
        ("RRULE:FREQ=DAILY", next_day, 1, "Daily", always),
        ("RRULE:FREQ=DAILY", next_day, 7, "Weekly", always),
        ("RRULE:FREQ=DAILY", next_day, 3, three_days, always),
        (all_days, next_day, 1, "Daily", always),
        (
            "RRULE:FREQ=HOURLY;INTERVAL=12",
            "2026-01-05T21:00:00Z",
            1,
            half_a_day,
            always,
        ),
        // 一周里的固定时段：`BYDAY` 的每一天算一次重复。
        ("RRULE:FREQ=DAILY", evening, 7, "Weekly", every_day),
        ("RRULE:FREQ=DAILY", evening, 1, "Daily", every_day),
        (weekdays, evening, 5, "Weekly", business_hours),
        (weekdays, evening, 10, two_weeks, business_hours),
        (weekdays_daily, evening, 5, "Weekly", business_hours),
        ("RRULE:FREQ=WEEKLY", evening, 1, "Weekly", monday),
    ];

    for (recurrence, end, shifts, kind, windows) in cases {
        let label = format!("{recurrence} {end} {shifts}");
        let imported = import(
            &shift_based_bundle(vec![shift_based_event(
                "Event", recurrence, A_MONDAY, end, shifts,
            )]),
            at("2026-01-01T00:00:00Z"),
        );
        let item = &imported.items[0];

        assert_eq!(item.action, ImportAction::Create, "{label}: {item:?}");

        let rotation = &imported.writes[0].value.rotations[0];
        let written: Vec<(u8, String, String)> = rotation
            .active_windows
            .iter()
            .map(|range| {
                (
                    range.weekday_mask,
                    range.start.to_string(),
                    range.end.to_string(),
                )
            })
            .collect();
        let expected: Vec<(u8, String, String)> = windows
            .iter()
            .map(|(mask, from, to)| (*mask, from.to_string(), to.to_string()))
            .collect();

        assert_eq!(format!("{:?}", rotation.kind), kind, "{label}");
        assert_eq!(written, expected, "{label}");
        assert_eq!(rotation.start_at, at(A_MONDAY), "{label}");
    }
}

#[test]
fn shift_based_turns_count_the_shifts_of_each_member() {
    // 每人五个工作日：周五下班之后没人，下周一 09:00 换人。
    let imported = import(
        &shift_based_bundle(vec![shift_based_event(
            "Days",
            "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            A_MONDAY,
            A_MONDAY_EVENING,
            5,
        )]),
        at("2026-01-01T00:00:00Z"),
    );
    let schedule = &imported.writes[0].value;

    for (time, expected) in [
        ("2026-01-05T08:59:00Z", None),
        ("2026-01-05T09:00:00Z", Some("alice@example.com")),
        ("2026-01-05T17:00:00Z", None),
        ("2026-01-09T16:59:00Z", Some("alice@example.com")),
        ("2026-01-10T12:00:00Z", None),
        ("2026-01-12T09:00:00Z", Some("bob@example.com")),
        ("2026-01-19T09:00:00Z", Some("carol@example.com")),
        ("2026-01-26T09:00:00Z", Some("alice@example.com")),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }

    // 只有一个人时交接没有意义，每人几班都一样。
    let mut alone = shift_based_event(
        "Alone",
        "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
        A_MONDAY,
        A_MONDAY_EVENING,
        1,
    );

    alone["assignment_strategy"]["members"] = user_members(&["PALICE1"]);

    let mut all_of_one = alone.clone();

    all_of_one["assignment_strategy"] = json!({
        "type": "every_member_assignment_strategy",
        "members": user_members(&["PBOB001"])
    });

    for (event, expected) in [
        (alone, "alice@example.com"),
        (all_of_one, "bob@example.com"),
    ] {
        let imported = import(&shift_based_bundle(vec![event]), at("2026-01-01T00:00:00Z"));
        let schedule = &imported.writes[0].value;

        assert_eq!(
            on_call(schedule, at("2026-01-09T16:59:00Z")).as_deref(),
            Some(expected)
        );
        assert_eq!(on_call(schedule, at("2026-01-10T12:00:00Z")), None);
    }

    // 全是空位的事件不会有人值班。
    let mut vacant = weekly_event("Vacant");

    vacant["assignment_strategy"]["members"] = json!([{"type": "empty_member"}]);

    let imported = import(
        &shift_based_bundle(vec![vacant]),
        at("2026-01-01T00:00:00Z"),
    );

    assert_eq!(imported.items[0].action, ImportAction::Create);
    assert_eq!(
        codes(&imported.items[0].findings),
        vec!["rotation_empty", "rendered_missing"]
    );
}

#[test]
fn what_shift_based_events_cannot_carry_is_listed() {
    let recurrence = |rule: &str, end: &str, shifts: i64| {
        shift_based_event("Event", rule, A_MONDAY, end, shifts)
    };
    let lines = |lines: Value| {
        let mut event = weekly_event("Event");

        event["recurrence"] = lines;
        event
    };
    let strategy = |strategy: Value| {
        let mut event = weekly_event("Event");

        event["assignment_strategy"] = strategy;
        event
    };
    let off_the_minute = shift_based_event(
        "Event",
        "RRULE:FREQ=DAILY",
        "2026-01-05T09:00:30Z",
        A_MONDAY_EVENING,
        1,
    );
    let weekend = shift_based_event(
        "Event",
        "RRULE:FREQ=WEEKLY;BYDAY=SA,SU",
        "2026-01-10T00:00:00Z",
        "2026-01-11T23:59:00Z",
        2,
    );
    let mut bad_time = weekly_event("Event");
    let mut bad_zone = weekly_event("Event");

    bad_time["start_time"]["date_time"] = json!("not a time");
    bad_zone["start_time"]["time_zone"] = json!("Eastern Time (US & Canada)");

    let several = user_members(&["PALICE1", "PBOB001"]);
    let cases = vec![
        // 多人同时值班、空着的一班、认不出的策略。
        (
            strategy(json!({"type": "every_member_assignment_strategy", "members": several})),
            "every_member_on_call",
        ),
        (
            strategy(json!({
                "type": "rotating_member_assignment_strategy",
                "shifts_per_member": 1,
                "members": [
                    {"type": "empty_member"},
                    {"type": "user_member", "user_id": "PALICE1"}
                ]
            })),
            "participant_none",
        ),
        (
            strategy(json!({"type": "some_new_strategy", "members": several})),
            "rotation_type_unsupported",
        ),
        (
            strategy(json!({
                "type": "rotating_member_assignment_strategy",
                "shifts_per_member": 0,
                "members": several
            })),
            "rotation_length_invalid",
        ),
        (
            strategy(json!({
                "type": "rotating_member_assignment_strategy",
                "members": several
            })),
            "rotation_length_invalid",
        ),
        (
            strategy(json!({
                "type": "rotating_member_assignment_strategy",
                "shifts_per_member": 1,
                "members": [{"type": "user_member", "user_id": "PNOBODY"}]
            })),
            "user_without_email",
        ),
        (
            strategy(json!({
                "type": "rotating_member_assignment_strategy",
                "shifts_per_member": 1,
                "members": [{"type": "robot_member"}]
            })),
            "participant_unknown",
        ),
        // 增减某一次的。
        (
            lines(json!(["RRULE:FREQ=WEEKLY", "EXDATE:20260112T090000Z"])),
            "recurrence_unsupported",
        ),
        (
            lines(json!(["RRULE:FREQ=WEEKLY", "RDATE:20260114T090000Z"])),
            "recurrence_unsupported",
        ),
        (lines(json!([])), "recurrence_unsupported"),
        (
            lines(json!(["RRULE:FREQ=WEEKLY", "RRULE:FREQ=DAILY"])),
            "recurrence_unsupported",
        ),
        // 不是每周重复的，或者这里不认识的写法。
        (
            recurrence("RRULE:FREQ=MONTHLY", "2026-02-05T09:00:00Z", 1),
            "recurrence_unsupported",
        ),
        (
            recurrence("RRULE:FREQ=WEEKLY;COUNT=10", A_WEEK_LATER, 1),
            "recurrence_unsupported",
        ),
        (
            recurrence("RRULE:FREQ=WEEKLY;UNTIL=20261231T000000Z", A_WEEK_LATER, 1),
            "recurrence_unsupported",
        ),
        (
            recurrence("RRULE:FREQ=WEEKLY;BYDAY=1MO", A_MONDAY_EVENING, 1),
            "recurrence_unsupported",
        ),
        (
            recurrence("FREQ=WEEKLY", A_WEEK_LATER, 1),
            "recurrence_unsupported",
        ),
        // 隔一周才有的一班、隔几小时才有的一小段。
        (
            recurrence("RRULE:FREQ=WEEKLY;INTERVAL=2", A_WEEK_LATER, 1),
            "recurrence_unsupported",
        ),
        (
            recurrence(
                "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TU",
                A_MONDAY_EVENING,
                2,
            ),
            "recurrence_unsupported",
        ),
        (
            recurrence("RRULE:FREQ=HOURLY;INTERVAL=6", "2026-01-05T10:00:00Z", 1),
            "recurrence_unsupported",
        ),
        // 第一班不在 `BYDAY` 的那几天里。
        (
            recurrence("RRULE:FREQ=WEEKLY;BYDAY=TU", A_MONDAY_EVENING, 1),
            "recurrence_unsupported",
        ),
        // 前一班没结束下一班就开始了。
        (
            recurrence("RRULE:FREQ=DAILY", "2026-01-06T10:00:00Z", 1),
            "recurrence_unsupported",
        ),
        (weekend, "recurrence_unsupported"),
        // 不是整分写不成时段，结束不晚于开始也说不清。
        (off_the_minute, "recurrence_unsupported"),
        (
            recurrence("RRULE:FREQ=DAILY", A_MONDAY, 1),
            "recurrence_unsupported",
        ),
        // 周五的下一班隔着周末，交接不在固定的周期上。
        (
            recurrence(
                "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
                A_MONDAY_EVENING,
                1,
            ),
            "shifts_per_member_unsupported",
        ),
        (bad_time, "time_invalid"),
        (bad_zone, "timezone_unknown"),
    ];

    for (event, code) in cases {
        let imported = import(
            &shift_based_bundle(vec![event.clone()]),
            at("2026-01-01T00:00:00Z"),
        );
        let item = &imported.items[0];

        assert_eq!(item.action, ImportAction::Error, "{code}: {event}");
        assert_eq!(codes(&item.blockers), vec![code], "{event}");
    }
}

#[test]
fn every_email_of_a_shift_based_event_is_listed_even_when_it_cannot_come_over() {
    let mut event = weekly_event("Event");

    event["assignment_strategy"] = json!({
        "type": "every_member_assignment_strategy",
        "members": user_members(&["PNEW001", "PALICE1"])
    });

    let mut bundle = shift_based_bundle(vec![event]);

    bundle["users"] = json!([
        {"id": "PNEW001", "email": "new.one@example.com"},
        {"id": "PALICE1", "email": "alice@example.com"}
    ]);

    let imported = import(&bundle, at("2026-01-01T00:00:00Z"));

    assert_eq!(
        imported.items[0].unknown_members,
        vec!["new.one@example.com".to_string()]
    );
    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["every_member_on_call"]
    );
}

#[test]
fn overlapping_shift_based_events_are_refused_since_both_would_be_on_call() {
    let mut bundle = shift_based_bundle(vec![weekly_event("Primary")]);

    // 两条轮换也一样：事件之间没有优先级。
    bundle["schedule"]["rotations"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({"id": "R2", "type": "schedule_rotation", "events": [shift_based_event(
                "Daytime",
                "RRULE:FREQ=DAILY",
                A_MONDAY,
                A_MONDAY_EVENING,
                1,
            )]}),
        );

    let imported = import(&bundle, at("2026-06-01T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["rotations_overlap"]);
    assert_eq!(item.blockers[0].params["rotation"], "Primary");
    assert_eq!(item.blockers[0].params["other"], "Daytime");
    assert_eq!(item.blockers[0].params["at"], "2026-06-01 09:00 UTC");
}

#[test]
fn shift_based_events_that_take_turns_are_not_overlapping() {
    // 白天一个，晚上一个。
    let imported = import(
        &shift_based_bundle(vec![
            shift_based_event("Day", "RRULE:FREQ=DAILY", A_MONDAY, A_MONDAY_EVENING, 1),
            shift_based_event(
                "Night",
                "RRULE:FREQ=DAILY",
                A_MONDAY_EVENING,
                "2026-01-06T09:00:00Z",
                1,
            ),
        ]),
        at("2026-01-01T00:00:00Z"),
    );

    assert_eq!(
        imported.items[0].action,
        ImportAction::Create,
        "{:?}",
        imported.items[0]
    );

    // PagerDuty 改成员的做法：旧事件停在某一刻，新事件照抄 `start_time`，从那一刻接上。
    let cutover = "2026-07-01T00:00:00Z";
    let mut old = weekly_event("Old");
    let mut new = weekly_event("New");

    old["effective_until"] = json!(cutover);
    new["effective_since"] = json!(cutover);
    new["assignment_strategy"]["members"] = user_members(&["PALICE1", "PCAROL1", "PBOB001"]);

    let imported = import(
        &shift_based_bundle(vec![old, new]),
        at("2026-06-01T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");

    let schedule = &imported.writes[0].value;

    // 从 `start_time` 数，7 月 1 日是第 25 班，新的顺序里轮到 carol。
    for (time, expected) in [
        ("2026-06-30T23:59:00Z", Some("bob@example.com")),
        (cutover, Some("carol@example.com")),
        ("2026-07-06T08:59:00Z", Some("carol@example.com")),
        ("2026-07-06T09:00:00Z", Some("bob@example.com")),
        ("2026-07-13T09:00:00Z", Some("alice@example.com")),
    ] {
        assert_eq!(on_call(schedule, at(time)).as_deref(), expected, "{time}");
    }

    // `effective_since` 已经过去时不影响轮到谁。
    let mut clamped = weekly_event("Clamped");

    clamped["effective_since"] = json!("2026-03-03T10:00:00Z");

    let imported = import(
        &shift_based_bundle(vec![clamped]),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(imported.writes[0].value.rotations[0].start_at, at(A_MONDAY));
}

#[test]
fn shift_based_events_keep_their_own_timezone() {
    let in_new_york = |name: &str, start: &str, end: &str| {
        let mut event = shift_based_event(name, "RRULE:FREQ=WEEKLY", start, end, 1);

        event["start_time"]["time_zone"] = json!("America/New_York");
        event["end_time"]["time_zone"] = json!("America/New_York");
        event
    };

    // 周一 09:00（纽约）交接；排班自己写的是 UTC。
    let imported = import(
        &shift_based_bundle(vec![in_new_york(
            "Weekly",
            "2026-10-19T13:00:00Z",
            "2026-10-26T13:00:00Z",
        )]),
        at("2026-10-01T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert_eq!(item.findings[0].code, "timezone_from_events");
    assert_eq!(item.findings[0].params["schedule_timezone"], "UTC");
    assert_eq!(item.findings[0].params["timezone"], "America/New_York");

    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.timezone, "America/New_York");

    // 夏令时结束后还是当地 09:00。
    assert_eq!(
        on_call(schedule, at("2026-11-02T13:30:00Z")).as_deref(),
        Some("bob@example.com")
    );
    assert_eq!(
        on_call(schedule, at("2026-11-02T14:00:00Z")).as_deref(),
        Some("carol@example.com")
    );

    // 已经结束的事件不算；还在生效的事件用了两个时区时一份排班写不下。
    let mut ended = weekly_event("Ended");

    ended["effective_until"] = json!("2026-02-01T00:00:00Z");

    let imported = import(
        &shift_based_bundle(vec![
            ended,
            in_new_york("Weekly", "2026-10-19T13:00:00Z", "2026-10-26T13:00:00Z"),
        ]),
        at("2026-10-01T00:00:00Z"),
    );

    assert_eq!(imported.writes[0].value.timezone, "America/New_York");

    let imported = import(
        &shift_based_bundle(vec![
            weekly_event("Here"),
            in_new_york("There", "2026-10-19T13:00:00Z", "2026-10-26T13:00:00Z"),
        ]),
        at("2026-10-01T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(codes(&item.blockers), vec!["event_timezones_differ"]);
    assert_eq!(item.blockers[0].params["rotation"], "Here");
    assert_eq!(item.blockers[0].params["timezone"], "UTC");
    assert_eq!(item.blockers[0].params["other"], "There");
    assert_eq!(
        item.blockers[0].params["other_timezone"],
        "America/New_York"
    );
}

#[test]
fn a_shift_based_file_without_events_is_refused() {
    let brief = json!({"schedule": {"id": "PS1A2B3C", "type": "schedule_v3",
        "name": "Engineering On-Call", "time_zone": "America/New_York",
        "self": "https://api.pagerduty.com/v3/schedules/PS1A2B3C"}});
    let imported = import(&brief, at("2026-06-01T00:00:00Z"));

    assert_eq!(codes(&imported.items[0].blockers), vec!["events_missing"]);

    let no_events = json!({"schedule": {"type": "schedule_v3", "name": "Rotation only",
        "time_zone": "UTC", "rotations": [{"id": "R1", "type": "schedule_rotation"}]}});
    let imported = import(&no_events, at("2026-06-01T00:00:00Z"));

    assert_eq!(codes(&imported.items[0].blockers), vec!["events_missing"]);

    let unknown_zone = json!({"schedule": {"type": "schedule_v3", "name": "Named zone",
        "time_zone": "Eastern Time (US & Canada)", "rotations": []}});
    let imported = import(&unknown_zone, at("2026-06-01T00:00:00Z"));

    assert_eq!(codes(&imported.items[0].blockers), vec!["timezone_unknown"]);

    let plain_times = json!({"schedule": {"type": "schedule_v3", "name": "Plain times",
        "time_zone": "UTC", "rotations": [{"events": [{"name": "Event",
            "start_time": "2026-01-05T09:00:00Z", "end_time": "2026-01-12T09:00:00Z",
            "assignment_strategy": {"type": "rotating_member_assignment_strategy"}}]}]}});
    let imported = import(&plain_times, at("2026-06-01T00:00:00Z"));

    assert_eq!(imported.items[0].name, "Plain times");
    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["schedule_unreadable"]
    );
}

#[test]
fn a_shift_based_final_schedule_is_compared_without_overrides_and_gaps() {
    // 5 月 25 日那一周是第 20 班（carol），6 月 1 日那一周是第 21 班（alice），中间有 bob 的替班。
    let bundle = weekly_with_final_schedule(vec![
        assignment(
            "2026-05-25T09:00:00Z",
            "2026-06-01T09:00:00Z",
            Some("PCAROL1"),
            "schedule_rotation",
        ),
        assignment(
            "2026-06-01T09:00:00Z",
            "2026-06-02T09:00:00Z",
            Some("PALICE1"),
            "schedule_rotation",
        ),
        assignment(
            "2026-06-02T09:00:00Z",
            "2026-06-03T09:00:00Z",
            Some("PBOB001"),
            "schedule_rotation_override",
        ),
        assignment(
            "2026-06-03T09:00:00Z",
            "2026-06-08T09:00:00Z",
            Some("PALICE1"),
            "schedule_rotation",
        ),
        assignment(
            "2026-05-26T09:00:00Z",
            "2026-05-27T09:00:00Z",
            Some("PBOB001"),
            "schedule_rotation_override",
        ),
    ]);
    let imported = import(&bundle, at("2026-05-30T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert_eq!(
        codes(&item.findings),
        vec!["rendered_matched", "overrides_left_out"]
    );

    // 过去的替班不算：只有 6 月 2 日那一个。
    assert_eq!(item.findings[1].params["count"], "1");

    // 空位和没有这一段是一个意思。
    let mut gaps = shift_based_bundle(vec![shift_based_event(
        "Days",
        "RRULE:FREQ=DAILY",
        A_MONDAY,
        A_MONDAY_EVENING,
        1,
    )]);

    gaps["schedule"]["final_schedule"] = json!({"computed_shift_assignments": [
        assignment("2026-06-01T00:00:00Z", "2026-06-01T09:00:00Z", None, "schedule_rotation"),
        // 第 147 天，147 除以 3 余 0。
        assignment("2026-06-01T09:00:00Z", "2026-06-01T17:00:00Z", Some("PALICE1"), "schedule_rotation"),
        assignment("2026-06-01T17:00:00Z", "2026-06-02T00:00:00Z", None, "schedule_rotation"),
    ]});

    let imported = import(&gaps, at("2026-05-30T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert_eq!(codes(&item.findings), vec!["rendered_matched"]);

    // 认不出的来历也拿去比，对不上就挡住。
    let imported = import(
        &weekly_with_final_schedule(vec![assignment(
            "2026-06-01T09:00:00Z",
            "2026-06-08T09:00:00Z",
            Some("PCAROL1"),
            "some_new_source",
        )]),
        at("2026-05-30T00:00:00Z"),
    );

    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["rendered_mismatch"]
    );
}

#[test]
fn custom_shifts_that_are_still_ahead_stop_the_import() {
    let rotation = assignment(
        "2026-06-01T09:00:00Z",
        "2026-06-08T09:00:00Z",
        Some("PALICE1"),
        "schedule_rotation",
    );
    let custom =
        |start: &str, end: &str, source: &str| assignment(start, end, Some("PCAROL1"), source);

    let imported = import(
        &weekly_with_final_schedule(vec![
            rotation.clone(),
            custom(
                "2026-06-03T09:00:00Z",
                "2026-06-03T12:00:00Z",
                "custom_shift",
            ),
            custom(
                "2026-06-02T09:00:00Z",
                "2026-06-02T12:00:00Z",
                "custom_shift_override",
            ),
            custom(
                "2026-05-02T09:00:00Z",
                "2026-05-02T12:00:00Z",
                "custom_shift",
            ),
        ]),
        at("2026-05-30T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["custom_shifts"]);
    assert_eq!(item.blockers[0].params["count"], "2");
    assert_eq!(item.blockers[0].params["at"], "2026-06-02 09:00 UTC");
    assert_eq!(item.blockers[0].params["until"], "2026-06-02 12:00 UTC");
    assert_eq!(item.blockers[0].params["user"], "carol@example.com");

    // 已经过去的不影响现在往后谁值班。
    let imported = import(
        &weekly_with_final_schedule(vec![
            rotation,
            custom(
                "2026-05-02T09:00:00Z",
                "2026-05-02T12:00:00Z",
                "custom_shift",
            ),
        ]),
        at("2026-06-01T09:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rendered_matched"));
}

#[test]
fn a_shift_based_final_schedule_with_two_people_at_once_stops_the_import() {
    let imported = import(
        &weekly_with_final_schedule(vec![
            assignment(
                "2026-06-01T09:00:00Z",
                "2026-06-08T09:00:00Z",
                Some("PALICE1"),
                "schedule_rotation",
            ),
            assignment(
                "2026-06-03T09:00:00Z",
                "2026-06-03T12:00:00Z",
                Some("PCAROL1"),
                "schedule_rotation",
            ),
        ]),
        at("2026-05-30T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["rendered_several_on_call"]);
    assert_eq!(item.blockers[0].params["at"], "2026-06-03 09:00 UTC");
    assert_eq!(item.blockers[0].params["until"], "2026-06-03 12:00 UTC");
    assert_eq!(item.blockers[0].params["first"], "alice@example.com");
    assert_eq!(item.blockers[0].params["second"], "carol@example.com");

    // 同一个人的重叠只是一个人；已经过去的重叠不影响现在往后谁值班。
    let imported = import(
        &weekly_with_final_schedule(vec![
            assignment(
                "2026-06-01T09:00:00Z",
                "2026-06-08T09:00:00Z",
                Some("PALICE1"),
                "schedule_rotation",
            ),
            assignment(
                "2026-06-03T09:00:00Z",
                "2026-06-03T12:00:00Z",
                Some("PALICE1"),
                "schedule_rotation",
            ),
            assignment(
                "2026-05-02T09:00:00Z",
                "2026-05-02T12:00:00Z",
                Some("PCAROL1"),
                "schedule_rotation",
            ),
            assignment(
                "2026-05-02T10:00:00Z",
                "2026-05-02T11:00:00Z",
                Some("PALICE1"),
                "schedule_rotation",
            ),
        ]),
        at("2026-06-01T09:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rendered_matched"));
}

// ---- Opsgenie ----

#[test]
fn opsgenie_rotations_keep_who_is_on_call() {
    let bundle = fixture(OPSGENIE_ROTATIONS);
    let imported = import(&bundle, at("2026-10-19T06:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(item.blockers.is_empty(), "{:?}", item.blockers);
    assert_eq!(
        codes(&item.findings),
        vec!["team_not_carried", "rotation_ended", "rendered_matched"]
    );

    let schedule = &imported.writes[0].value;

    assert_eq!(schedule.timezone, "Europe/Berlin");
    assert_eq!(schedule.rotations.len(), 2);
    assert!(matches!(schedule.rotations[0].kind, RotationKind::Weekly));
    assert!(matches!(
        schedule.rotations[1].kind,
        RotationKind::Custom {
            period_secs: 172_800
        }
    ));

    let timeline = &bundle["timelines"][0];
    let mut checked = 0;

    for rotation in timeline["baseTimeline"]["rotations"].as_array().unwrap() {
        for period in rotation["periods"].as_array().unwrap() {
            let expected = period["recipient"]["name"].as_str();
            let (start, end) = (
                at(period["startDate"].as_str().unwrap()),
                at(period["endDate"].as_str().unwrap()),
            );

            for sample in samples(start.0, end.0) {
                assert_eq!(
                    on_call(schedule, sample).as_deref(),
                    expected,
                    "at {:?}",
                    DateTime::<Utc>::from_timestamp_micros(sample.0)
                );
                checked += 1;
            }
        }
    }

    assert_eq!(checked, 22 * 3);

    // 两条轮换的交接处一分钟都不差：周一 09:00（已是冬令时）之前是 dora，之后是 ben。
    assert_eq!(
        on_call(schedule, at("2026-10-26T07:59:00Z")).as_deref(),
        Some("dora@example.com")
    );
    assert_eq!(
        on_call(schedule, at("2026-10-26T08:00:00Z")).as_deref(),
        Some("ben@example.com")
    );
}

#[test]
fn an_opsgenie_timeline_that_disagrees_stops_the_import() {
    let mut bundle = fixture(OPSGENIE_ROTATIONS);

    bundle["data"][0]["rotations"][1]["length"] = json!(1);

    let imported = import(&bundle, at("2026-10-19T06:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(item.blockers[0].code, "rendered_mismatch");
    assert!(codes(&item.findings).contains(&"rotation_ended"));
}

#[test]
fn only_a_final_opsgenie_timeline_leaves_its_overrides_out_of_the_comparison() {
    let mut bundle = fixture(OPSGENIE_ROTATIONS);

    bundle["timelines"][0]
        .as_object_mut()
        .unwrap()
        .remove("baseTimeline");

    let imported = import(&bundle, at("2026-10-19T06:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rendered_matched"));
}

#[test]
fn the_published_opsgenie_samples_name_what_they_cannot_carry() {
    let bundle = fixture(OPSGENIE_GET_SAMPLE);

    // 样例的轮换里有「没有人」，还在生效时带不过来。
    let imported = import(&bundle, at("2017-02-10T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["participant_none"]);
    assert_eq!(item.blockers[0].params["rotation"], "First Rotation");

    // 结束以后就只是跳过。
    let imported = import(&bundle, at("2017-03-01T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"rotation_ended"));
    assert!(imported.writes[0].value.rotations.is_empty());

    // 用户在这里没有账号，团队作参与者：一条条都列出来。
    let imported = import(&fixture(OPSGENIE_LIST_SAMPLE), at("2026-06-01T00:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["participant_team"]);
    assert_eq!(item.blockers[0].params["name"], "ops_team");
    assert_eq!(item.unknown_members, vec!["user@opsgenie.com".to_string()]);
}

fn opsgenie_rotation(name: &str, restriction: Value, end: Option<&str>) -> Value {
    let mut rotation = json!({
        "id": name,
        "name": name,
        "startDate": "2026-01-05T08:00:00Z",
        "type": "weekly",
        "participants": [{"type": "user", "username": "anna@example.com"}]
    });

    if !restriction.is_null() {
        rotation["timeRestriction"] = restriction;
    }

    if let Some(end) = end {
        rotation["endDate"] = json!(end);
    }

    rotation
}

fn opsgenie_schedule(rotations: Vec<Value>) -> Value {
    json!({"data": {"id": "s", "name": "Two", "timezone": "UTC", "enabled": true, "rotations": rotations}})
}

fn time_of_day(start: u32, end: u32) -> Value {
    json!({
        "type": "time-of-day",
        "restriction": {"startHour": start, "startMin": 0, "endHour": end, "endMin": 0}
    })
}

#[test]
fn overlapping_opsgenie_rotations_are_refused_since_both_would_be_on_call() {
    let imported = import(
        &opsgenie_schedule(vec![
            opsgenie_rotation("Primary", Value::Null, None),
            opsgenie_rotation("Secondary", time_of_day(9, 17), None),
        ]),
        at("2026-06-01T00:00:00Z"),
    );
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(codes(&item.blockers), vec!["rotations_overlap"]);
    assert_eq!(item.blockers[0].params["rotation"], "Primary");
    assert_eq!(item.blockers[0].params["other"], "Secondary");
    assert_eq!(item.blockers[0].params["at"], "2026-06-01 09:00 UTC");
}

#[test]
fn opsgenie_rotations_that_take_turns_are_not_overlapping() {
    // 时段不重叠。
    let imported = import(
        &opsgenie_schedule(vec![
            opsgenie_rotation("Day", time_of_day(9, 17), None),
            opsgenie_rotation("Night", time_of_day(17, 9), None),
        ]),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(imported.items[0].action, ImportAction::Create);

    // 一条结束之后另一条才开始。
    let mut later = opsgenie_rotation("Later", Value::Null, None);

    later["startDate"] = json!("2026-07-01T00:00:00Z");

    let imported = import(
        &opsgenie_schedule(vec![
            opsgenie_rotation("Now", Value::Null, Some("2026-07-01T00:00:00Z")),
            later,
        ]),
        at("2026-06-01T00:00:00Z"),
    );
    let schedule = &imported.writes[0].value;

    assert_eq!(
        imported.items[0].action,
        ImportAction::Create,
        "{:?}",
        imported.items[0]
    );
    assert_eq!(
        schedule.rotations[0].end_at,
        Some(at("2026-07-01T00:00:00Z"))
    );
    assert_eq!(codes(&imported.items[0].findings), vec!["rendered_missing"]);
}

#[test]
fn opsgenie_lengths_and_participants_map_one_by_one() {
    let mut hourly = opsgenie_rotation("Hourly", Value::Null, None);

    hourly["type"] = json!("hourly");
    hourly["length"] = json!(6);

    let imported = import(&opsgenie_schedule(vec![hourly]), at("2026-06-01T00:00:00Z"));

    assert!(matches!(
        imported.writes[0].value.rotations[0].kind,
        RotationKind::Custom {
            period_secs: 21_600
        }
    ));

    let mut odd = opsgenie_rotation("Odd", Value::Null, None);

    odd["type"] = json!("monthly");
    odd["participants"] = json!([
        {"type": "user", "id": "only-an-id"},
        {"type": "escalation", "name": "Follow up"},
        {"type": "robot"}
    ]);

    let imported = import(&opsgenie_schedule(vec![odd]), at("2026-06-01T00:00:00Z"));

    assert_eq!(
        codes(&imported.items[0].blockers),
        vec![
            "user_without_email",
            "participant_escalation",
            "participant_unknown",
            "rotation_type_unsupported"
        ]
    );
}

#[test]
fn a_disabled_opsgenie_schedule_comes_over_paused_without_a_comparison() {
    let mut bundle = fixture(OPSGENIE_ROTATIONS);

    bundle["data"][0]["enabled"] = json!(false);
    bundle["timelines"][0]["baseTimeline"]["rotations"] = json!([]);

    let imported = import(&bundle, at("2026-10-19T06:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Create, "{item:?}");
    assert!(codes(&item.findings).contains(&"schedule_paused"));
    assert!(!imported.writes[0].value.enabled);
    assert_eq!(
        on_call(&imported.writes[0].value, at("2026-10-20T12:00:00Z")),
        None
    );
}

#[test]
fn a_file_with_too_much_in_one_schedule_is_refused() {
    let rotations: Vec<Value> = (0..200)
        .map(|index| {
            let mut rotation = opsgenie_rotation(&format!("R{index}"), time_of_day(1, 2), None);

            rotation["startDate"] = json!(format!("20{:02}-01-05T08:00:00Z", 30 + index % 60));
            rotation["endDate"] = json!(format!("20{:02}-01-06T08:00:00Z", 30 + index % 60));
            rotation
        })
        .collect();
    let imported = import(&opsgenie_schedule(rotations), at("2026-06-01T00:00:00Z"));

    assert!(codes(&imported.items[0].blockers).contains(&"too_many_rotations"));

    let restrictions = vec![
        json!({"type": "daily_restriction", "start_time_of_day": "08:00:00", "duration_seconds": 3600});
        65
    ];
    let imported = import(
        &pagerduty_layer(json!({
            "start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
            "rotation_turn_length_seconds": 86400, "users": three_people(),
            "restrictions": restrictions
        })),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(
        codes(&imported.items[0].blockers),
        vec!["restriction_unsupported"]
    );
}

// ---- 走完预演和导入 ----

#[test]
fn every_email_without_a_member_is_listed_and_nothing_is_written() {
    let mut bundle = fixture(OPSGENIE_ROTATIONS);

    bundle["data"][0]["rotations"][0]["participants"] = json!([
        {"type": "user", "username": "new.one@example.com"},
        {"type": "user", "username": "anna@example.com"},
        {"type": "user", "username": "new.two@example.com"}
    ]);

    let imported = import(&bundle, at("2026-10-19T06:00:00Z"));
    let item = &imported.items[0];

    assert_eq!(item.action, ImportAction::Error);
    assert_eq!(
        item.unknown_members,
        vec![
            "new.one@example.com".to_string(),
            "new.two@example.com".to_string()
        ]
    );
    assert!(imported.writes.is_empty());
}

#[test]
fn importing_the_same_file_again_changes_nothing_and_keeps_local_settings() {
    let bundle = fixture(PAGERDUTY_LAYERS);
    let now = at("2026-10-26T08:00:00-04:00");
    let directory = directory();
    let imported = import(&bundle, now);
    let mut saved = imported.writes[0].value.clone();

    saved.id = Id::from_string("saved");
    saved.enabled = false;
    saved.swap_needs_approval = true;
    saved.overrides = vec![crate::domain::alerting::schedule::ScheduleOverride {
        id: Id::from_string("cover"),
        user_id: Id::from_string("id-dan"),
        start_at: at("2026-10-28T09:00:00-04:00"),
        end_at: at("2026-10-28T13:00:00-04:00"),
        reason: "dentist".into(),
    }];

    let existing = vec![Existing {
        id: saved.id.clone(),
        name: saved.name.clone(),
        portable: portable_of(&saved, &SCHEDULE_DROP, &directory),
    }];
    let current = vec![saved];
    let again = import_into(&bundle, now, &existing, &current);

    assert_eq!(
        again.items[0].action,
        ImportAction::Unchanged,
        "{:?}",
        again.items[0]
    );
    assert!(again.writes.is_empty());

    // 轮值变了才更新；对方没说的设置和替班留着。
    let mut edited = bundle.clone();

    edited["schedules"][0]["schedule_layers"][0]["name"] = json!("Weekends");
    edited["schedules"][0]
        .as_object_mut()
        .unwrap()
        .remove("final_schedule");

    let updated = import_into(&edited, now, &existing, &current);

    assert_eq!(
        updated.items[0].action,
        ImportAction::Update,
        "{:?}",
        updated.items[0]
    );
    assert_eq!(updated.items[0].changes, vec!["rotations".to_string()]);

    let value = &updated.writes[0].value;

    assert!(!value.enabled);
    assert!(value.swap_needs_approval);
    assert_eq!(value.overrides.len(), 1);
    assert_eq!(value.id, Id::from_string("saved"));
}

#[test]
fn the_comparison_says_what_it_did_not_cover() {
    // 没有渲染结果。
    let imported = import(
        &pagerduty_layer(json!({
            "start": "2026-01-01T00:00:00Z", "rotation_virtual_start": "2026-01-01T00:00:00Z",
            "rotation_turn_length_seconds": 86400, "users": three_people()
        })),
        at("2026-06-01T00:00:00Z"),
    );

    assert_eq!(codes(&imported.items[0].findings), vec!["rendered_missing"]);

    // 渲染结果全在过去。
    let bundle = fixture(PAGERDUTY_LAYERS);
    let imported = import(&bundle, at("2026-12-01T00:00:00Z"));

    assert!(codes(&imported.items[0].findings).contains(&"rendered_stale"));

    // 渲染范围没跨过夏令时切换：比过的部分一致，之后的没比。
    let mut bundle = fixture(PAGERDUTY_LAYERS);

    bundle["until"] = json!("2026-10-30T09:00:00-04:00");

    let imported = import(&bundle, at("2026-10-26T08:00:00-04:00"));

    assert_eq!(
        codes(&imported.items[0].findings),
        vec![
            "rotation_ended",
            "rendered_matched",
            "clock_change_not_compared",
            "overrides_left_out"
        ]
    );
}

#[test]
fn other_files_are_not_taken_for_foreign_schedules() {
    let directory = directory();
    let now = at("2026-06-01T00:00:00Z");

    assert!(
        read(
            &json!({"format": "molesignal.schedules/v1", "schedules": []}),
            &directory,
            now
        )
        .is_none()
    );
    assert!(read(&json!({"hello": "world"}), &directory, now).is_none());
    assert!(read(&json!({"data": {"finalTimeline": {}}}), &directory, now).is_none());
    assert_eq!(
        read(&json!({"data": []}), &directory, now).unwrap().format,
        OPSGENIE_FORMAT
    );
    assert_eq!(
        read(&json!({"schedules": []}), &directory, now)
            .unwrap()
            .format,
        PAGERDUTY_FORMAT
    );
}
