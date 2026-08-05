# Notify Capability

## Purpose

Provide one organization-scoped Notify control plane and delivery engine for
connectors, recipients, policies, templates, fallback routing, and delivery
audit. Alerting emits events into this engine and does not own a separate
channel or template system.

## Requirements

### Requirement: Notify Settings Information Architecture

The web app SHALL expose Notify management only under System Settings at:

- `/settings/notify/connectors`
- `/settings/notify/users`
- `/settings/notify/policies`
- `/settings/notify/templates`
- `/settings/notify/defaults`
- `/settings/notify/deliveries`

The Settings sidebar SHALL render these pages as children of one Notify group.
The application SHALL NOT register `/alerts/notify/*`, `/alerts/channels`,
`/alerts/templates`, `/settings/alert_destinations`, or
`/settings/alert_templates`.

#### Scenario: Old unshipped URL is absent

- **WHEN** a user opens `/alerts/notify/connectors`
- **THEN** the application renders its normal not-found page
- **AND** it does not redirect to a Settings page or the home page

### Requirement: Connector Management

The system SHALL expose organization-scoped connector type discovery and CRUD
under `/api/v1/notify/connector-types` and `/api/v1/notify/connectors`.
Connector secrets SHALL be encrypted at rest and redacted from API responses.

#### Scenario: Connector secret is protected

- **WHEN** an organization admin creates an SMTP connector with a password
- **THEN** the response returns a redacted password
- **AND** the plaintext password is not present in `notify_connectors`

### Requirement: User Endpoints And Preferences

The system SHALL manage per-user delivery endpoints at
`/api/v1/users/:user_id/notify-endpoints` and category preferences at
`/api/v1/users/:user_id/notify-preferences`. A preference SHALL define an
ordered endpoint chain, quiet hours, and critical-bypass behavior.

#### Scenario: Ordered user route

- **WHEN** a user preference lists two enabled and verified endpoints
- **THEN** delivery planning preserves their configured order
- **AND** can continue from the primary endpoint to the fallback endpoint

### Requirement: Policy Matching And Recipient Resolution

The system SHALL expose Notify policy CRUD and preview under
`/api/v1/notify/policies`. A policy SHALL match `event_type`, category, and
attribute matchers, then resolve recipients using a registered resolver.
Supported resolvers SHALL include fixed users, team members, and on-call
schedules.

#### Scenario: Policy preview is side-effect free

- **WHEN** an admin previews a policy with a representative event
- **THEN** the response reports whether it matched, the resolved recipients,
  and the ordered delivery plan
- **AND** no event or delivery row is created

### Requirement: User Team And Organization Fallback Routing

Delivery planning SHALL support an ordered chain of user primary endpoints,
user fallback endpoints, team defaults, and organization defaults. A policy's
fallback configuration SHALL explicitly enable or disable each fallback
scope.

#### Scenario: Full fallback chain

- **WHEN** a matching policy enables all fallback scopes for a team member
- **THEN** the plan orders user primary, user fallback, team default, and
  organization default routes
- **AND** duplicate connector-target pairs are removed without reordering the
  remaining routes

### Requirement: Notify Templates

The system SHALL expose organization-scoped template CRUD at
`/api/v1/notify/templates`, backed by `notify_templates`. Every template SHALL
declare a Notify category and SHALL support common Notify fields such as
`{{event.id}}`, `{{event.type}}`, `{{event.occurred_at}}`,
`{{event.attributes.<key>}}`, `{{message.title}}`, and `{{message.text}}`.

Alert templates SHALL support the alert placeholder contract:
`{{rule.id}}`, `{{rule.name}}`, `{{rule.description}}`,
`{{incident.id}}`, `{{incident.fingerprint}}`, `{{incident.status}}`,
`{{incident.summary}}`, `{{severity}}`, `{{value}}`, `{{threshold}}`,
`{{evaluated_at}}`, `{{labels.<key>}}`, and `{{annotations.<key>}}`.
Alert escalation templates SHALL use the same alert placeholder contract.

On-call templates SHALL support schedule, shift-transition, and override
placeholders under `schedule.*`, `oncall.*`, and `override.*`. Missing dynamic
values SHALL remain visible as their original placeholder rather than being
silently removed.

The system SHALL expose the template field catalog and category-specific
presets at `/api/v1/notify/template-fields`. The response SHALL also include
label and annotation keys discovered from the current organization so the
editor can list concrete dynamic placeholders.
Every Notify category SHALL provide built-in `text`, `markdown`, and `html`
presets.

#### Scenario: Alert placeholder is rendered

- **WHEN** an alert policy selects a template containing `{{rule.name}}`,
  `{{incident.status}}`, and `{{labels.service}}`
- **THEN** the delivered message contains the corresponding alert values

#### Scenario: On-call template is generated and rendered

- **WHEN** an admin selects an on-call preset and a shift transition event is
  delivered
- **THEN** the generated template uses the schedule and shift placeholders
- **AND** the delivered message contains the schedule name, current user, next
  user, and transition time

### Requirement: Event Delivery Engine

The engine SHALL durably enqueue organization-scoped Notify events, match all
enabled policies in priority order, resolve recipients, create idempotent
delivery attempts, render the selected template, and dispatch through the
connector adapter registry. Every attempt SHALL be observable through
`/api/v1/notify/deliveries`.

#### Scenario: Replayed event remains idempotent

- **WHEN** the same event and delivery route are processed more than once
- **THEN** the unique idempotency key prevents duplicate delivery attempts

### Requirement: Alert Integration

Alerting SHALL enqueue `alert.triggered`, `alert.acknowledged`,
`alert.resolved`, and `alert.escalated` events into the Notify engine.
Escalation targets SHALL identify users, teams, or schedules only; connector
selection, templates, and fallbacks SHALL be owned by Notify policies.

#### Scenario: Alert trigger uses only Notify

- **WHEN** an alert rule opens an incident
- **THEN** an `alert.triggered` event is enqueued
- **AND** no alert-owned channel dispatcher or alert-owned delivery row is
  invoked

### Requirement: Legacy Alert Notify Removal

The runtime schema SHALL NOT contain `notify_channels`, the old `deliveries`
table, or `alert_subscriptions`. Alert rules and incidents SHALL NOT contain
`template_id` or `body_template`, escalation steps SHALL NOT contain
`channel_ids`, and Notify connectors SHALL NOT contain `legacy_channel_id`.
The legacy alert channel, subscription, template-fields, and alert-template
HTTP routes SHALL not be registered.

#### Scenario: Upgrade removes unshipped schema

- **WHEN** migrations run against a database containing the unshipped alert
  notify schema
- **THEN** the obsolete tables and columns are dropped
- **AND** `alert_templates` is renamed to `notify_templates`
