## ADDED Requirements

### Requirement: Custom domain registration

The system SHALL expose `POST /api/v1/domains` accepting `{ hostname, org_id }`. Hostnames SHALL be validated as valid DNS names and unique across orgs. Domain operations require `license.has_feature("domain_management")`.

#### Scenario: Duplicate hostname rejected

- **WHEN** an Admin POSTs `{ "hostname": "obs.acme.com" }` and that hostname already maps to another org
- **THEN** the system returns 409 with body `{ "error": "hostname already registered" }`

### Requirement: ACME certificate issuance

For each registered domain, the system SHALL request a Let's Encrypt certificate via HTTP-01 challenge. The router SHALL serve the challenge response at `/.well-known/acme-challenge/<token>` and present the issued cert via SNI.

#### Scenario: Cert issued and served

- **WHEN** a domain is registered and ACME flow completes
- **THEN** `GET https://<hostname>/api/v1/healthz` succeeds with a Let's Encrypt cert valid for that hostname

### Requirement: Renewal scheduling

A background task SHALL renew certs at 30 days before expiry. Failures SHALL alert on the `domain.renewal_failed` event and retry every 6 hours.

#### Scenario: Renewal trigger

- **WHEN** a cert's `not_after` is within 30 days
- **THEN** the renewal task attempts re-issuance on the next tick
