// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::quota::{QuotaUsage, TaskQuota};

fn quota() -> TaskQuota {
    TaskQuota {
        max_active_per_org: 3,
        max_active_per_node: 2,
        max_created_per_hour_per_org: 10,
        max_created_per_hour_per_node: 5,
    }
}

#[test]
fn each_limit_refuses_the_task_that_would_exceed_it() {
    let quota = quota();
    assert!(quota.admit(QuotaUsage::default()).is_ok());

    for usage in [
        QuotaUsage {
            org_active: 3,
            ..QuotaUsage::default()
        },
        QuotaUsage {
            node_active: 2,
            ..QuotaUsage::default()
        },
        QuotaUsage {
            org_created_last_hour: 10,
            ..QuotaUsage::default()
        },
        QuotaUsage {
            node_created_last_hour: 5,
            ..QuotaUsage::default()
        },
    ] {
        let error = quota.admit(usage).unwrap_err();
        assert_eq!(error.http_status_code(), 429, "{usage:?}");
    }
}

#[test]
fn limits_are_validated() {
    assert!(TaskQuota::default().validate().is_ok());

    let mut zero = quota();
    zero.max_active_per_node = 0;
    assert!(zero.validate().is_err());

    let mut wider_node = quota();
    wider_node.max_active_per_node = 4;
    assert!(wider_node.validate().is_err());
}

#[test]
fn node_slots_follow_the_smaller_of_the_quota_and_the_agent() {
    let quota = quota();
    assert_eq!(quota.node_slots(8, 0), 2);
    assert_eq!(quota.node_slots(1, 0), 1);
    assert_eq!(quota.node_slots(0, 0), 1);
    assert_eq!(quota.node_slots(8, 2), 0);
    assert_eq!(quota.node_slots(8, 5), 0);
}
