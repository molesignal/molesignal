// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

fn defaults(format: IntakeFormat, service: &str) -> IntakeDefaults {
    IntakeDefaults {
        format,
        service: service.into(),
        ..IntakeDefaults::default()
    }
}

#[test]
fn a_generic_body_is_read_by_the_pointers_the_address_names() {
    let body = json!({"app": {"name": "checkout", "tag": "2.0.0"}, "who": "ada", "env": "prod", "kind": "deployment"});
    let defaults = IntakeDefaults {
        service_field: "/app/name".into(),
        version_field: "/app/tag".into(),
        author_field: "who".into(),
        environment_field: "env".into(),
        ..IntakeDefaults::default()
    };

    let changes = read_changes(&body, &defaults).unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Deployment);
    assert_eq!(changes[0].service, "checkout");
    assert_eq!(changes[0].version, "2.0.0");
    assert_eq!(changes[0].environment, "prod");
    assert_eq!(changes[0].author.as_deref(), Some("ada"));
}

#[test]
fn a_generic_list_without_pointers_is_read_as_the_deployments_api_takes_it() {
    let body = json!([
        {"service": "checkout", "version": "2.0.0", "status": "failed", "deployedAt": 1_795_514_400_000_i64},
        {"service": "search", "kind": "scaling", "description": "3 → 6 replicas"}
    ]);

    let changes = read_changes(&body, &IntakeDefaults::default()).unwrap();

    assert_eq!(
        changes[0].status,
        Some(domain::apm::DeploymentStatus::Failed)
    );
    // Milliseconds are read as such.
    assert_eq!(changes[0].deployed_at, Some(1_795_514_400_000_000));
    assert_eq!(changes[1].kind, ChangeKind::Scaling);
}

#[test]
fn what_the_body_leaves_unsaid_comes_from_the_address_and_its_kind_wins() {
    let body = json!({"service": "", "description": "pool raised"});
    let defaults = IntakeDefaults {
        service: "checkout".into(),
        environment: "prod".into(),
        kind: Some(ChangeKind::Configuration),
        ..IntakeDefaults::default()
    };

    let change = read_changes(&body, &defaults).unwrap().remove(0);

    assert_eq!(
        (change.service.as_str(), change.environment.as_str()),
        ("checkout", "prod")
    );
    assert_eq!(change.kind, ChangeKind::Configuration);
}

#[test]
fn a_change_of_no_service_is_refused() {
    let error = read_changes(&json!({"description": "x"}), &IntakeDefaults::default()).unwrap_err();

    assert!(error.to_string().contains("?service="), "{error}");
    assert!(read_changes(&json!([]), &IntakeDefaults::default()).is_err());
}

#[test]
fn a_launchdarkly_entry_is_a_feature_flag_of_the_environment_it_acted_in() {
    let body = json!({
        "kind": "flag",
        "name": "New checkout",
        "title": "Ada turned on the flag New checkout in Production",
        "date": 1_795_514_400_000_i64,
        "member": {"email": "ada@example.com"},
        "accesses": [{"action": "updateOn", "resource": "proj/default:env/production:flag/new-checkout"}]
    });

    let change = read_changes(&body, &defaults(IntakeFormat::Launchdarkly, "checkout"))
        .unwrap()
        .remove(0);

    assert_eq!(change.kind, ChangeKind::FeatureFlag);
    assert_eq!(change.environment, "production");
    assert_eq!(change.author.as_deref(), Some("ada@example.com"));
    assert_eq!(
        change.description.as_deref(),
        Some("Ada turned on the flag New checkout in Production")
    );
}

#[test]
fn an_unleash_event_says_what_happened_to_which_toggle() {
    let body = json!({"type": "feature-environment-enabled", "featureName": "new-checkout", "environment": "production", "createdBy": "ada", "createdAt": "2026-11-24T10:00:00Z"});

    let change = read_changes(&body, &defaults(IntakeFormat::Unleash, "checkout"))
        .unwrap()
        .remove(0);

    assert_eq!(
        change.description.as_deref(),
        Some("feature environment enabled new-checkout")
    );
    assert_eq!(change.environment, "production");
    assert_eq!(change.deployed_at, Some(1_795_514_400_000_000));
}

#[test]
fn nacos_and_apollo_changes_are_configuration_changes() {
    let nacos = read_changes(
        &json!({"dataId": "checkout.yaml", "group": "DEFAULT_GROUP", "namespace": "prod"}),
        &defaults(IntakeFormat::Nacos, "checkout"),
    )
    .unwrap()
    .remove(0);

    assert_eq!(nacos.kind, ChangeKind::Configuration);
    assert_eq!(
        nacos.description.as_deref(),
        Some("checkout.yaml (DEFAULT_GROUP) changed")
    );
    assert_eq!(nacos.namespace, "prod");

    // An Apollo release names its app: no service is needed on the address.
    let apollo = read_changes(&json!({"appId": "checkout", "clusterName": "default", "namespaceName": "application", "releaseTitle": "raise pool", "operator": "ada"}), &defaults(IntakeFormat::Apollo, "")).unwrap().remove(0);

    assert_eq!(apollo.service, "checkout");
    assert_eq!(
        apollo.description.as_deref(),
        Some("application released raise pool")
    );
}
