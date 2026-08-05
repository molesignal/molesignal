## ADDED Requirements

### Requirement: AcmeClient Implementation

The system SHALL ship an `AcmeClient` implementation using `instant-acme` that performs http-01 challenge flow against a configurable ACME directory (`production` Let's Encrypt by default; `staging` for tests; arbitrary URL for Pebble / local CA).

#### Scenario: Successful issuance writes cert to domain row

- **WHEN** `AcmeClient::issue("obs.acme.com")` is invoked against a directory that returns valid challenge + certificate
- **AND** the http-01 challenge token is reachable at `http://obs.acme.com/.well-known/acme-challenge/<token>`
- **THEN** `domains` row updates with `state="active"`, `cert_pem` populated, `cert_not_after_micros` set to cert's notAfter, `last_error = NULL`

#### Scenario: Challenge fails → state=failed + last_error

- **WHEN** issuance fails because http-01 challenge is unreachable
- **THEN** `domains` row has `state="failed"`, `last_error` contains the ACME error detail

### Requirement: Auto Renewal Loop

The system SHALL run a background `acme_runner` that:
- Every `acme.issue_poll_secs` (default 60s): scans `state="pending"` domains and issues certs.
- Every `acme.renewal_retry_secs` (default 6h): scans `state="active"` domains whose `cert_not_after_micros < now + 30d` and re-issues.
- On any failure: writes `last_error` and either keeps `state="active"` (renewal failure with existing valid cert) or sets `state="failed"` (initial issue failure).

#### Scenario: Cert expiring within 30 days triggers renewal

- **WHEN** a domain row has `state="active"` and `cert_not_after_micros = now + 20d`
- **THEN** the next renewal tick re-issues the cert; the row's `cert_pem` is replaced and `cert_not_after_micros` is updated to the new value

#### Scenario: Failed renewal keeps current cert

- **WHEN** renewal fails for a domain whose existing cert is still valid for 20 more days
- **THEN** the row state stays `active`, `last_error` is populated, the old `cert_pem` is NOT cleared

### Requirement: SNI Cert Resolver

The HTTPS server SHALL select certificates per request via `rustls::server::ResolvesServerCert` backed by `domains` table lookups:
- Given SNI hostname → query `find_by_hostname` → if `state="active"` and `cert_pem` non-NULL, parse + return `CertifiedKey`
- Otherwise → return `None` (rustls aborts handshake)
- A small in-memory cache (TTL 60s) reduces DB load.

#### Scenario: Known hostname returns CertifiedKey

- **WHEN** a TLS handshake reaches `serverName = "obs.acme.com"` and that domain is active
- **THEN** the resolver returns `Some(CertifiedKey)` constructed from `cert_pem` + private key file

#### Scenario: Unknown hostname yields None

- **WHEN** SNI is `random.example.org` not in `domains`
- **THEN** resolver returns `None`, handshake aborts with `BadCertificate`

#### Scenario: Cache invalidation on cert update

- **WHEN** `AcmeClient::issue` updates a domain's `cert_pem`
- **THEN** the resolver's internal cache for that hostname is invalidated (next handshake re-reads from DB)

### Requirement: TLS Server Bind

When `[http.tls].enabled = true`, the bootstrap process SHALL bind both:
- Port 80 (configurable): plain HTTP, mounts `/healthz` + `/.well-known/acme-challenge/{token}` only (everything else 301 → https)
- Port 443 (configurable): rustls server with the SNI resolver, serves the full `/api/v1/*` router

When `enabled = false`, behavior is unchanged from current (single plain HTTP on `[http.port]`).

#### Scenario: tls.enabled=false keeps current behavior

- **WHEN** config has `[http.tls].enabled = false`
- **THEN** the server binds only `[http.bind]:[http.port]` plain, no rustls dep activation, no 80/443 split

#### Scenario: tls.enabled=true binds both 80 and 443

- **WHEN** config has `[http.tls].enabled = true` with `bind_addr = "0.0.0.0"`, `port = 443`, `plain_port = 80`
- **THEN** bootstrap log shows both listeners ready
- **AND** GET http://host/healthz returns 200
- **AND** GET https://host/api/v1/healthz with valid SNI returns 200
