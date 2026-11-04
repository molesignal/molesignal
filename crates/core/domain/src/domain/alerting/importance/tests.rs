// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

fn medium() -> UrgencyLevel {
    UrgencyLevel {
        key: Urgency::parse("medium").unwrap(),
        name: " 中 ".into(),
        escalates: true,
        quiet_hours: QuietHours::Block,
        steps: StepGroup::Low,
        severities: vec![Severity::Error],
    }
}

fn three_levels() -> ImportanceLevels {
    let mut levels = ImportanceLevels::default();

    levels.priorities.truncate(3);
    levels.urgencies[0].severities = vec![Severity::Critical];
    levels.urgencies.insert(1, medium());
    levels
}

#[test]
fn the_built_in_levels_are_p1_to_p5_and_high_and_low() {
    let mut levels = ImportanceLevels::default();

    levels.normalize().unwrap();

    assert_eq!(levels.priority_count(), 5);
    assert_eq!(levels.by_severity(Severity::Error), Urgency::high());
    assert_eq!(levels.by_severity(Severity::Warning), Urgency::low());
    assert!(levels.urgency(&Urgency::high()).escalates);
    assert_eq!(
        levels.urgency(&Urgency::low()).quiet_hours,
        QuietHours::Block
    );
}

#[test]
fn an_organization_names_its_levels_and_adds_an_urgency() {
    let mut levels = three_levels();

    levels.normalize().unwrap();

    let medium = Urgency::parse("medium").unwrap();
    assert_eq!(levels.urgency(&medium).name, "中");
    assert_eq!(levels.by_severity(Severity::Error), medium);
    assert_eq!(levels.rank(&medium), 1);
    assert!(levels.check_priority(Priority::P3).is_ok());
    assert!(levels.check_priority(Priority::P4).is_err());
    assert_eq!(levels.fit_priority(Priority::P5), Priority::P3);
    assert!(
        levels
            .check_urgency(&Urgency::parse("night").unwrap())
            .is_err()
    );

    // An urgency that was taken away counts as the high one: nobody is paged less than asked.
    let gone = Urgency::parse("gone").unwrap();
    assert_eq!(levels.urgency(&gone).key, Urgency::high());
}

#[test]
fn levels_that_cannot_work_are_refused() {
    let mut no_low = three_levels();
    no_low.urgencies.retain(|level| !level.key.is_low());
    no_low.urgencies[1].severities = vec![Severity::Error, Severity::Warning, Severity::Info];
    assert!(no_low.normalize().is_err());

    let mut twice = three_levels();
    twice.urgencies[1].severities = vec![Severity::Error, Severity::Info];
    assert!(twice.normalize().is_err());

    let mut gap = three_levels();
    gap.priorities.remove(1);
    assert!(gap.normalize().is_err());

    let mut none = three_levels();
    none.priorities.clear();
    assert!(none.normalize().is_err());

    let mut repeated = three_levels();
    repeated.urgencies.push(medium());
    assert!(repeated.normalize().is_err());
}
