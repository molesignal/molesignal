## ADDED Requirements

### Requirement: IAM Derived Views Prefer Existing APIs
IAM pages SHALL use existing identity, token, and license APIs for derived views before marking a page backend-pending.

#### Scenario: Service accounts derived from users
- **WHEN** `/iam/service-accounts` can derive service accounts from `GET /api/v1/users`
- **THEN** it renders matching non-human accounts as a live list
- **AND** only the create-token workflow remains gated if no dedicated endpoint exists

#### Scenario: Quota derived from license snapshot
- **WHEN** `/iam/quota` can read plan limits from `GET /api/v1/license`
- **THEN** it renders a live quota summary using known limits and unknown usage as unavailable values
