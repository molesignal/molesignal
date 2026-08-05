## ADDED Requirements

### Requirement: License Status Page

The web app SHALL provide `/settings/license` rendering a read-only summary of the current plan, quota usage, billing period, and recent invoices. The page SHALL call `GET /api/v1/license` (current plan + quota), `GET /api/v1/license/invoices?limit=12` (recent invoices). An `Upgrade plan` primary button SHALL navigate to the marketplace upgrade flow.

#### Scenario: Plan card renders core fields

- **WHEN** the user opens `/settings/license`
- **THEN** the top card shows plan name, monthly cost, billing period (start → end), seats used / max
- **AND** quota usage bars render for ingestion (GB/day), query (queries/day), storage (TB)
- **AND** every bar at >= 80% used is colored `--yellow`; >= 95% is `--red`

#### Scenario: Invoice history table

- **WHEN** the API returns 12 invoices
- **THEN** a table shows date, amount, status (paid / pending / failed), invoice id
- **AND** clicking an invoice id opens the PDF in a new tab (target=_blank)

#### Scenario: Upgrade button routes to marketplace

- **WHEN** the user clicks `Upgrade plan`
- **THEN** the router navigates to `/marketplace/upgrade?from=license`
- **AND** the marketplace upgrade flow handles the rest (out of scope for this change)

#### Scenario: Non-admin sees plan but no upgrade

- **WHEN** a user with role `Viewer` navigates to `/settings/license`
- **THEN** the page renders plan summary + quota bars (read-only)
- **AND** the `Upgrade plan` button is hidden (admin-only action)
