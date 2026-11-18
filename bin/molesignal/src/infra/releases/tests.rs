// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

#[test]
fn versions_compare_numerically_and_accept_historical_tags() {
    let parse = |s| ProductVersion::parse(s).unwrap();
    assert!(parse("v26.10.0.0") > parse("26.2.0.0"));
    assert!(parse("26.0.0.0") > parse("v26.0.0.0.alpha-1"));
    assert!(parse("26.4.0.0") > parse("v26.4.0.0-alpha"));
    assert!(parse("v26.4.0.0-alpha") > parse("26.3.0.2"));
    assert!(parse("26.0.0.0-rc.10") > parse("26.0.0.0-rc.2"));

    for value in [
        "latest",
        "26.0.0",
        "26.0.0.0/other",
        "26.0.0.0.",
        "26.0.0.0+bad",
        "26.-1.0.0",
    ] {
        assert!(ProductVersion::parse(value).is_none(), "{value}");
    }
}

#[test]
fn picks_highest_version_ignoring_order_and_prerelease_flag() {
    let releases = serde_json::from_value::<Vec<Release>>(serde_json::json!([
        {"tag_name": "v26.1.0.0", "draft": false, "prerelease": false},
        {"tag_name": "v26.10.0.0", "draft": false, "prerelease": true},
        {"tag_name": "v99.0.0.0", "draft": true},
        {"tag_name": "unrelated", "draft": false}
    ]))
    .unwrap();

    assert_eq!(newest(releases).unwrap().tag_name, "v26.10.0.0");
}

#[test]
fn refresh_and_failure_are_throttled_but_success_lives_six_hours() {
    let mut cache = Cache::default();
    assert!(!cache.fresh(false));
    cache.attempted_at = Some(Instant::now());
    assert!(cache.fresh(true));
    cache.attempted_at = Some(Instant::now() - Duration::from_secs(301));
    assert!(!cache.fresh(false));
    cache.info.status = "ok";
    assert!(cache.fresh(false));
    assert!(!cache.fresh(true));
}
