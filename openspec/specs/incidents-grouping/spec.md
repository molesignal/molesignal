# Incidents Grouping Capability

## Purpose

alert → incident 聚合算法：按 label / 时间窗 / 指纹去重，降低告警噪音；与 `alerting` 的 incident 模型对接，并产出 root-cause 提示元数据。

## Requirements

### Requirement: Incident group aggregation

When alerts fire, the system SHALL group related incidents into `incident_groups` based on `{ alert_rule_id, fingerprint, time_window }`. Default time_window is 15 minutes; two incidents with the same `alert_rule_id` + same `fingerprint` arriving within the window SHALL be merged into the same group.

#### Scenario: Same rule + fingerprint groups

- **WHEN** alert rule R fires for fingerprint F at T0
- **AND** the same rule fires again for fingerprint F at T0 + 5min
- **THEN** both incidents reference the same `group_id` and the group has `incident_count: 2`

### Requirement: Group lifecycle

A group SHALL transition `open → acked → resolved` driven by aggregate ack/resolve operations. Ack on the group SHALL ack all member incidents; resolve on the group SHALL resolve all members.

#### Scenario: Group resolve cascades

- **WHEN** an Admin POSTs `/api/v1/alerts/incident_groups/<id>/resolve`
- **THEN** the group state becomes `resolved`, all member incidents become `resolved`, and the dispatcher stops the escalation chain for each

### Requirement: Group HTTP read API

The system SHALL expose `GET /api/v1/alerts/incident_groups?org_id=&from=&to=&state=` returning paginated groups with `{ id, alert_rule_name, fingerprint, incident_count, first_at, last_at, state, members: [<incident_id>] }`.
 
#### Scenario: List filters by state

- **WHEN** a user GETs `/api/v1/alerts/incident_groups?state=open`
- **THEN** only groups currently `open` are returned
