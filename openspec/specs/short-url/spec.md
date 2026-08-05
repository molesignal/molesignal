# Short URL Capability

## Purpose

`/api/v1/short` 短链生成与 `/s/:code` 反解重定向，dashboard / saved view / scheduled report 分享必需。8 位 base62 短码、org 范围、记录 click 计数与失效策略。

## Requirements

### Requirement: Short URL creation and lookup

The system SHALL expose `POST /api/v1/short` to create a short URL from a long URL, and `GET /s/:code` to redirect. The short code SHALL be 8 base62 characters generated from `rand::random`. Each short URL is org-scoped and carries `{ org_id, long_url, created_by, created_at, click_count, expires_at }`.

#### Scenario: Create then redirect

- **WHEN** a user POSTs `{ "long_url": "https://molesignal.example/dashboards/abc" }`
- **THEN** the response carries `{ "short_code": "<8chars>", "short_url": "https://.../s/<8chars>" }`
- **AND** a GET to `/s/<8chars>` returns 302 to the long URL with `click_count` incremented

### Requirement: Expiry and revocation

A short URL with `expires_at <= now` SHALL return 410 Gone on redirect. Users SHALL be able to DELETE a short URL they created (or Admin+ for any in the org).

#### Scenario: Expired short URL

- **WHEN** a GET hits an expired short code
- **THEN** the system returns 410 with body `{ "error": "short url expired" }`
