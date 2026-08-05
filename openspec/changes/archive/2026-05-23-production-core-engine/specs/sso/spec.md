## ADDED Requirements

### Requirement: OIDC and SAML Login Flow

When `[auth.sso].enabled = true`, the system SHALL expose `GET /api/v1/auth/sso/login?provider=<id>` that redirects to the configured IdP (OIDC `authorization_endpoint` or SAML `SingleSignOnService` URL), and `GET/POST /api/v1/auth/sso/callback` that consumes the IdP response, validates signature / nonce / audience, resolves the local user via `email` claim, issues a local JWT, and redirects to the configured post-login URL.

#### Scenario: OIDC roundtrip issues JWT
- **WHEN** a user clicks login and authenticates at an OIDC IdP, returning to `/callback` with a valid `code`
- **THEN** the server exchanges `code → tokens`, validates the ID Token's `iss/aud/exp/nonce`, finds-or-provisions the user, sets `Set-Cookie: token=<jwt>` and `Location: /`, status `302 Found`

#### Scenario: SAML POST binding accepted
- **WHEN** the IdP POSTs a signed `SAMLResponse` to `/callback`
- **THEN** the signature is verified against `[auth.sso].saml.idp_metadata_url`'s certificate, the user is provisioned/found by `NameID` (email), a JWT is issued

#### Scenario: Signature mismatch rejected
- **WHEN** the SAML signature fails verification
- **THEN** the response is `401 Unauthorized` with `{ "error": "saml signature invalid" }`, no user is provisioned, an audit row records the failure

### Requirement: SSO User Auto-Provisioning and Role Mapping

On first successful SSO login, the system SHALL auto-create a `User` row with `email`, mark `disabled = false`, hash a random throwaway password, and create a `Membership` in the default org with role from `[auth.sso].role_mapping` (matched against IdP `groups` claim; default `Viewer`). Subsequent logins reuse the existing user; role MAY be promoted on each login per the mapping (never demoted).

#### Scenario: First login provisions Viewer
- **WHEN** a new email logs in via SSO and no `role_mapping` entry matches
- **THEN** a `User` + default-org `Membership { role: Viewer }` is created in one transaction

#### Scenario: Group claim promotes to Editor
- **WHEN** the IdP claim `groups` includes `molesignal-editors` and `role_mapping = { "molesignal-editors": "editor" }`
- **THEN** the existing membership's role is upgraded to `Editor` (and recorded in audit); never downgraded by a missing claim
