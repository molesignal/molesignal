## MODIFIED Requirements

### Requirement: Sidebar Extended Nav

The Sidebar SHALL surface APM and RUM as separate top-level entries under the analysis/observe group. Its primary investigation sequence SHALL be Dashboards, Metrics, Logs, Traces, APM and RUM, followed by Profiles and Alerts. APM SHALL link to `/apm/overview`; RUM SHALL link to `/rum/overview`. Each entry SHALL use a distinct Lucide icon. Functions and IAM SHALL remain top-level entries in their respective groups.

#### Scenario: Extended entries appear in collapsed and expanded states

- **WHEN** the user opens the app
- **THEN** the analysis/observe group contains Dashboards, Metrics, Logs, Traces, APM, RUM, Profiles and Alerts in that order
- **AND** APM and RUM remain independently addressable products
- **AND** in collapsed state every entry's icon is keyboard-reachable via `Tab`
- **AND** the active route's left rail tick is rendered
