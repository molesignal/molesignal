// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    super::{fields::Detected, probe::Coverage},
    *,
};

fn probe(signal: Signal, detected: Detected, coverage: Coverage) -> Probe {
    Probe {
        name: "app_logs".into(),
        signal,
        detected,
        coverage: Some(coverage),
        values: Vec::new(),
        error: None,
    }
}

fn logs(detected: Detected, coverage: Coverage) -> Probe {
    probe(Signal::Logs, detected, coverage)
}

fn ready() -> Detected {
    Detected {
        service: Some("service_name".into()),
        environment: Some("env".into()),
        trace_id: Some("trace_id".into()),
        message: Some("message".into()),
        message_is_text: true,
        text_time: None,
        own_fields: 8,
    }
}

fn full(total: u64) -> Coverage {
    Coverage {
        total,
        service: total,
        environment: total,
        trace_id: total,
        json_messages: 0,
        time_sample: None,
    }
}

fn kinds(found: &[Finding]) -> Vec<&str> {
    found.iter().map(|finding| finding.kind).collect()
}

#[test]
fn a_stream_that_records_what_investigations_need_has_nothing_to_report() {
    assert!(stream_findings(&logs(ready(), full(100))).is_empty());
}

#[test]
fn logs_without_a_service_are_critical_and_metrics_without_one_a_warning() {
    let blind = Detected {
        service: None,
        ..ready()
    };

    let found = stream_findings(&logs(blind.clone(), full(10)));

    assert_eq!(kinds(&found), ["no_service_field"]);
    assert_eq!(found[0].severity, Severity::Critical);
    assert_eq!(found[0].stream.as_deref(), Some("app_logs"));

    let metrics = stream_findings(&probe(Signal::Metrics, blind, full(10)));

    assert_eq!(metrics[0].severity, Severity::Warning);
}

#[test]
fn a_service_under_another_key_and_rows_without_it_are_called_out() {
    let synonym = Detected {
        service: Some("app".into()),
        ..ready()
    };

    let found = stream_findings(&logs(
        synonym,
        Coverage {
            service: 60,
            ..full(100)
        },
    ));

    assert_eq!(kinds(&found), ["service_field_synonym", "service_coverage"]);
    assert!(found[0].suggestion.contains(".service_name = del(.app)"));
    assert_eq!(found[1].detail, "40% of the rows carry no `app`.");
}

#[test]
fn logs_without_trace_ids_cannot_be_followed_to_their_traces() {
    let untraced = Detected {
        trace_id: None,
        ..ready()
    };

    assert_eq!(
        kinds(&stream_findings(&logs(untraced, full(10)))),
        ["no_trace_id"]
    );

    let few = stream_findings(&logs(
        ready(),
        Coverage {
            trace_id: 2,
            ..full(10)
        },
    ));

    assert_eq!(kinds(&few), ["trace_id_coverage"]);
    assert_eq!(few[0].severity, Severity::Info);

    // Spans always carry theirs: a traces stream is not asked for one.
    let spans = Detected {
        trace_id: None,
        message: None,
        ..ready()
    };

    assert!(stream_findings(&probe(Signal::Traces, spans, full(10))).is_empty());
}

#[test]
fn json_left_as_text_and_a_message_with_nothing_beside_it_are_called_out() {
    let found = stream_findings(&logs(
        Detected {
            own_fields: 2,
            message: Some("log.body".into()),
            ..ready()
        },
        Coverage {
            json_messages: 80,
            ..full(100)
        },
    ));

    assert_eq!(kinds(&found), ["unparsed_json", "unstructured_logs"]);
    assert!(
        found[0]
            .detail
            .starts_with("80% of the messages in `log.body`")
    );
    assert!(found[0].suggestion.contains("parse_json!(.\"log.body\")"));
}

#[test]
fn a_text_time_without_its_zone_is_called_out_and_one_with_it_is_not() {
    let timed = Detected {
        text_time: Some("time".into()),
        ..ready()
    };

    let local = stream_findings(&logs(
        timed.clone(),
        Coverage {
            time_sample: Some("2026-11-11 08:00:00".into()),
            ..full(10)
        },
    ));

    assert_eq!(kinds(&local), ["time_without_zone"]);
    assert_eq!(local[0].field.as_deref(), Some("time"));

    for sample in [
        "2026-11-11T08:00:00Z",
        "2026-11-11T08:00:00+08:00",
        "2026-11-11 08:00:00 -0700",
        "1762848000",
    ] {
        let found = stream_findings(&logs(
            timed.clone(),
            Coverage {
                time_sample: Some(sample.into()),
                ..full(10)
            },
        ));

        assert!(found.is_empty(), "{sample}");
    }
}

#[test]
fn a_stream_without_rows_says_only_that() {
    assert_eq!(
        kinds(&stream_findings(&logs(Detected::default(), full(0)))),
        ["no_data"]
    );
}

#[test]
fn the_scrape_job_names_the_service_of_metrics_but_not_under_the_shared_key() {
    let found = stream_findings(&probe(
        Signal::Metrics,
        Detected {
            service: Some("job".into()),
            trace_id: None,
            message: None,
            ..ready()
        },
        full(10),
    ));

    assert_eq!(kinds(&found), ["service_field_synonym"]);
}

#[test]
fn streams_that_record_the_service_under_different_keys_are_one_finding() {
    let mut first = logs(ready(), full(10));
    first.name = "a".into();

    let mut second = logs(
        Detected {
            service: Some("app".into()),
            ..ready()
        },
        full(10),
    );

    second.name = "b".into();

    let mut third = logs(ready(), full(10));
    third.name = "c".into();

    let finding = mixed_service_fields(&[first.clone(), second, third]).unwrap();

    assert_eq!(finding.kind, "mixed_service_fields");
    assert!(
        finding
            .detail
            .contains("service_name in 2 stream(s), app in 1 stream(s)")
    );
    assert_eq!(mixed_service_fields(&[first]), None);
}

#[test]
fn spellings_of_one_environment_get_one_mapping() {
    let found = environment_findings(&[vec![
        ("prod".to_string(), 900),
        ("production".to_string(), 40),
    ]]);

    assert_eq!(found[0].kind, "environment_spellings");
    assert!(
        found[0]
            .detail
            .contains("prod (900 rows), production (40 rows)")
    );
    assert!(
        found[0]
            .suggestion
            .contains("includes([\"production\"], .env)")
    );
    assert!(found[0].suggestion.contains(".env = \"prod\""));
}

#[test]
fn metrics_without_units_are_listed_by_what_they_lack() {
    let gaps = BTreeMap::from([
        (UnitGap::Duration, vec!["rpc_latency".to_string()]),
        (UnitGap::CounterTotal, vec!["jobs_done".to_string()]),
    ]);

    let found = unit_findings(&gaps);

    assert_eq!(
        kinds(&found),
        ["metric_without_unit", "counter_without_total"]
    );
    assert_eq!(
        found[0].detail,
        "1 metric(s) measure a duration without saying its unit: rpc_latency."
    );
}

#[test]
fn long_lists_are_cut_with_how_many_more() {
    let names = (0..20).map(|index| format!("s{index}")).collect::<Vec<_>>();

    assert!(listed(&names).ends_with("s14 and 5 more"));
}
