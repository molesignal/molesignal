## ADDED Requirements

### Requirement: RUM Query Backed Pages Use Normal Empty States
RUM pages that read telemetry through the generic query endpoint SHALL treat successful empty results as no telemetry for the selected time window, not as missing backend support.

#### Scenario: No RUM sessions in selected range
- **WHEN** `/rum/sessions` queries `rum_sessions` and receives no rows
- **THEN** it renders a normal empty session state for the active time window

#### Scenario: No RUM errors in selected range
- **WHEN** `/rum/errors` queries `rum_errors` and receives no rows
- **THEN** it renders a normal empty errors state for the active time window

#### Scenario: No performance samples in selected range
- **WHEN** a RUM performance route queries its backing stream and receives no rows
- **THEN** it renders normal no-data copy instead of backend-pending copy
