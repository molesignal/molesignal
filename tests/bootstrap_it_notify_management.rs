// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify 控制面：加密连接器、用户端点、偏好与引用保护。

mod common;

use serde_json::Value;
use sqlx::Row;

#[tokio::test]
async fn notify_connector_endpoint_and_preference_round_trip() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let create = server
        .client
        .post(format!("{}/api/v1/notify/connectors", server.base_url))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "Company SMTP",
            "connector_type": "email_smtp",
            "config": {
                "host": "smtp.example.com",
                "port": 587,
                "username": "mailer",
                "password": "notify-secret-password",
                "from": "alerts@example.com",
                "tls": "starttls",
                "timeout_secs": 10
            },
            "enabled": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status().as_u16(), 201);
    let connector: Value = create.json().await.unwrap();
    assert_eq!(connector["config"]["password"], "***");
    assert_eq!(connector["config"]["host"], "smtp.example.com");
    let connector_id = connector["id"].as_str().unwrap().to_string();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&server.settings.store.meta.dsn)
        .await
        .unwrap();
    let sealed = sqlx::query(
        "SELECT config_ciphertext, config_nonce
           FROM notify_connectors
          WHERE organization_id = $1 AND id = $2",
    )
    .bind(&server.root_org_id.0)
    .bind(&connector_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let ciphertext: Vec<u8> = sealed.try_get("config_ciphertext").unwrap();
    let nonce: Vec<u8> = sealed.try_get("config_nonce").unwrap();
    assert_eq!(nonce.len(), 12);
    assert!(!String::from_utf8_lossy(&ciphertext).contains("notify-secret-password"));

    let endpoint = server
        .client
        .post(format!(
            "{}/api/v1/users/{}/notify-endpoints",
            server.base_url, server.root_user_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "connector_id": connector_id,
            "external_identity": "root@test.example",
            "display_name": "Work email",
            "metadata": {},
            "verified": true,
            "enabled": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(endpoint.status().as_u16(), 201);
    let endpoint: Value = endpoint.json().await.unwrap();
    assert_eq!(endpoint["provider_type"], "email_smtp");
    assert_eq!(endpoint["verified"], true);
    let endpoint_id = endpoint["id"].as_str().unwrap().to_string();

    let preference = server
        .client
        .put(format!(
            "{}/api/v1/users/{}/notify-preferences/alert",
            server.base_url, server.root_user_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "enabled": true,
            "endpoint_ids": [endpoint_id],
            "quiet_hours": {
                "enabled": true,
                "timezone": "Asia/Shanghai",
                "start": "22:00",
                "end": "08:00"
            },
            "allow_critical_bypass": true
        }))
        .send()
        .await
        .unwrap();
    assert!(preference.status().is_success());
    let preference: Value = preference.json().await.unwrap();
    assert_eq!(preference["category"], "alert");
    assert_eq!(preference["steps"][0]["step_order"], 1);
    assert_eq!(preference["steps"][0]["endpoint_id"], endpoint_id);

    let team = server
        .client
        .post(format!("{}/api/v1/teams", server.base_url))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "Notify responders",
            "member_ids": [server.root_user_id]
        }))
        .send()
        .await
        .unwrap();
    assert!(team.status().is_success());
    let team: Value = team.json().await.unwrap();
    let team_id = team["id"].as_str().unwrap().to_string();

    let team_default = server
        .client
        .put(format!(
            "{}/api/v1/notify/team-defaults/{}/alert",
            server.base_url, team_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "enabled": true,
            "routes": [{
                "connector_id": connector_id,
                "target_type": "fixed_address",
                "target": "team@test.example",
                "order": 1
            }]
        }))
        .send()
        .await
        .unwrap();
    assert!(team_default.status().is_success());

    let organization_default = server
        .client
        .put(format!(
            "{}/api/v1/notify/organization-defaults/alert",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "enabled": true,
            "routes": [{
                "connector_id": connector_id,
                "target_type": "fixed_address",
                "target": "noc@test.example",
                "order": 1
            }]
        }))
        .send()
        .await
        .unwrap();
    assert!(organization_default.status().is_success());

    let template = server
        .client
        .post(format!("{}/api/v1/notify/templates", server.base_url))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "Critical alert",
            "body": "[{{severity}}] {{summary}} · {{event.id}}",
            "format": "markdown"
        }))
        .send()
        .await
        .unwrap();
    assert!(template.status().is_success());
    let template: Value = template.json().await.unwrap();
    assert_eq!(template["category"], "alert");
    let template_id = template["id"].as_str().unwrap().to_string();

    let template_fields = server
        .client
        .get(format!("{}/api/v1/notify/template-fields", server.base_url))
        .header(header_name, &header_value)
        .send()
        .await
        .unwrap();
    assert!(template_fields.status().is_success());
    let template_fields: Value = template_fields.json().await.unwrap();
    assert!(
        template_fields["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["token"] == "{{rule.name}}")
    );
    assert!(
        template_fields["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|preset| preset["key"] == "oncall_shift")
    );

    let oncall_template = server
        .client
        .post(format!("{}/api/v1/notify/templates", server.base_url))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "On-call handoff",
            "body": "{{schedule.name}}: {{oncall.current_user_id}} -> {{oncall.next_user_id}}",
            "format": "text",
            "category": "oncall"
        }))
        .send()
        .await
        .unwrap();
    assert!(oncall_template.status().is_success());
    let oncall_template: Value = oncall_template.json().await.unwrap();
    assert_eq!(oncall_template["category"], "oncall");

    let policy_input = serde_json::json!({
        "name": "Critical alert",
        "event_type": "alert.triggered",
        "category": "alert",
        "matchers": {"severity": ["critical"]},
        "recipient_resolver": "fixed_users",
        "resolver_config": {
            "user_ids": [server.root_user_id],
            "team_id": team_id
        },
        "delivery_mode": "prefer_user",
        "delivery_config": {"connector_ids": []},
        "template_id": template_id,
        "fallback_config": {
            "use_user_fallbacks": true,
            "use_team_defaults": true,
            "use_organization_defaults": true
        },
        "enabled": true,
        "priority": 10
    });
    let policy = server
        .client
        .post(format!("{}/api/v1/notify/policies", server.base_url))
        .header(header_name, &header_value)
        .json(&policy_input)
        .send()
        .await
        .unwrap();
    assert_eq!(policy.status().as_u16(), 201);
    let policy: Value = policy.json().await.unwrap();
    let policy_id = policy["id"].as_str().unwrap();

    let preview = server
        .client
        .post(format!(
            "{}/api/v1/notify/policies/{}/test",
            server.base_url, policy_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "event_id": "event-preview",
            "attributes": {"severity": "critical"}
        }))
        .send()
        .await
        .unwrap();
    assert!(preview.status().is_success());
    let preview: Value = preview.json().await.unwrap();
    assert_eq!(preview["matched"], true);
    assert_eq!(preview["recipients"][0]["resolved_by"], "fixed_users");
    assert_eq!(
        preview["recipients"][0]["delivery_plan"][0]["stage"],
        "user_primary"
    );
    assert_eq!(
        preview["recipients"][0]["delivery_plan"][1]["stage"],
        "team_fallback"
    );
    assert_eq!(
        preview["recipients"][0]["delivery_plan"][2]["stage"],
        "organization_fallback"
    );

    let mut draft_policy = policy_input.clone();
    draft_policy["name"] = Value::String("Unsaved critical alert".into());
    let draft_preview = server
        .client
        .post(format!(
            "{}/api/v1/notify/policies/preview",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "policy": draft_policy,
            "event": {
                "event_id": "event-draft-preview",
                "attributes": {"severity": "critical"}
            }
        }))
        .send()
        .await
        .unwrap();
    assert!(draft_preview.status().is_success());
    let draft_preview: Value = draft_preview.json().await.unwrap();
    assert_eq!(draft_preview["matched"], true);
    assert_eq!(
        draft_preview["recipients"][0]["delivery_plan"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let users = server
        .client
        .get(format!("{}/api/v1/notify/users", server.base_url))
        .header(header_name, &header_value)
        .send()
        .await
        .unwrap();
    assert!(users.status().is_success());
    let users: Value = users.json().await.unwrap();
    let root = users
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["user_id"].as_str() == Some(server.root_user_id.as_str()))
        .unwrap();
    assert_eq!(root["endpoints"][0]["id"], endpoint_id);

    let alert_template_update = server
        .client
        .put(format!(
            "{}/api/v1/notify/templates/{}",
            server.base_url, template_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "Critical alert",
            "body": "{{rule.name}} {{incident.summary}} {{evaluated_at}}",
            "format": "markdown"
        }))
        .send()
        .await
        .unwrap();
    assert!(alert_template_update.status().is_success());
    let alert_template_update: Value = alert_template_update.json().await.unwrap();
    assert_eq!(
        alert_template_update["body"],
        "{{rule.name}} {{incident.summary}} {{evaluated_at}}"
    );

    let delete_template_in_use = server
        .client
        .delete(format!(
            "{}/api/v1/notify/templates/{}",
            server.base_url, template_id
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_template_in_use.status().as_u16(), 409);

    let update = server
        .client
        .put(format!(
            "{}/api/v1/notify/connectors/{}",
            server.base_url, connector_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({
            "name": "Company SMTP",
            "config": connector["config"],
            "enabled": false
        }))
        .send()
        .await
        .unwrap();
    assert!(update.status().is_success());

    let endpoint_test = server
        .client
        .post(format!(
            "{}/api/v1/users/{}/notify-endpoints/{}/test",
            server.base_url, server.root_user_id, endpoint_id
        ))
        .header(header_name, &header_value)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(endpoint_test.status().as_u16(), 400);

    let delete_in_use = server
        .client
        .delete(format!(
            "{}/api/v1/notify/connectors/{}",
            server.base_url, connector_id
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .unwrap();
    assert_eq!(delete_in_use.status().as_u16(), 409);
}
