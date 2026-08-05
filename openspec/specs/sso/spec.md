# SSO Capability

## Purpose

OIDC、SAML 与 LDAP 集中登录、用户自动 provisioning、认证平台字段到本平台身份字段的映射，以及外部用户组到本地 Role 的绑定。付费特性，受 `license.has_feature("sso")` 闸门控制；社区版不公开 provider，登录端点返回 `403 Forbidden`。

## Requirements

### Requirement: OIDC, SAML, and LDAP Login Flow

When `[auth.sso].enabled = true` AND `license.has_feature("sso")` is true, the system SHALL expose `GET /api/v1/auth/sso/login?provider=<id>` that redirects to the configured IdP (OIDC `authorization_endpoint` or SAML `SingleSignOnService` URL), and `GET/POST /api/v1/auth/sso/callback` that consumes the IdP response, validates signature / nonce / audience, resolves the local user via `email` claim, issues a local JWT, and redirects to the configured post-login URL. When the license gate is off the routes are still mounted but return `403 Forbidden` with `{ "error": "feature 'sso' requires  license" }`.

#### Scenario: OIDC roundtrip issues JWT
- **WHEN** a user clicks login and authenticates at an OIDC IdP, returning to `/callback` with a valid `code`
- **THEN** the server exchanges `code → tokens`, validates the ID Token's `iss/aud/exp/nonce`, finds-or-provisions the user, sets `Set-Cookie: token=<jwt>` and `Location: /`, status `302 Found`

#### Scenario: SAML POST binding accepted
- **WHEN** the IdP POSTs a signed `SAMLResponse` to `/callback`
- **THEN** the signature is verified against `[auth.sso].saml.idp_metadata_url`'s certificate, the user is provisioned/found by `NameID` (email), a JWT is issued

#### Scenario: Signature mismatch rejected
- **WHEN** the SAML signature fails verification
- **THEN** the response is `401 Unauthorized` with `{ "error": "saml signature invalid" }`, no user is provisioned, an audit row records the failure

#### Scenario: LDAP credentials issue a local JWT
- **WHEN** an enabled LDAP provider resolves exactly one DN for the escaped `{username}` value and a simple bind with that DN and the submitted password succeeds
- **THEN** the configured subject, email, display-name, and group attributes are mapped to a local user and provider-org membership, and a local JWT is returned

#### Scenario: LDAP transport and filter are hardened
- **WHEN** an administrator saves an LDAP provider
- **THEN** the URL MUST use `ldaps://`, or `ldap://` with StartTLS enabled; the user filter MUST contain `{username}`, runtime substitution MUST use RFC 4515 escaping, empty user passwords MUST be rejected before bind, and a search returning zero or multiple DNs MUST fail authentication

#### Scenario: Public provider discovery does not expose secrets
- **WHEN** the sign-in page requests `GET /api/v1/auth/sso/providers`
- **THEN** only enabled provider `id`, `name`, and `kind` values are returned; OIDC client secrets, LDAP bind credentials, endpoints, certificates, and role mappings are never included

### Requirement: Protocol-specific identity field mapping

Every OIDC, SAML, and LDAP provider SHALL configure a `field_mapping` for the MoleSignal fields `subject`, `email`, `display_name`, and `groups`. Required mapped fields MUST be resolved only after the external identity has passed protocol validation.

#### Scenario: OIDC uses custom and nested claim names
- **WHEN** an OIDC provider maps `email = "mail"` and `groups = "realm_access.roles"`
- **THEN** the verified ID Token or UserInfo JSON is read using those claim paths, including the nested roles array

#### Scenario: SAML uses custom Attribute names
- **WHEN** a SAML provider maps `email = "mailAddress"` and `groups = "Roles"`
- **THEN** the signed Assertion is read from the exact configured Attribute Names; `NameID` MAY be selected as a special subject or email source

#### Scenario: LDAP uses custom attributes
- **WHEN** an LDAP provider maps `email = "userPrincipalName"` and `groups = "isMemberOf"`
- **THEN** those attributes are requested in the bounded directory search and used for provisioning; `dn` MAY be selected as the stable subject source

#### Scenario: Required mapped identity field is absent
- **WHEN** the configured subject or email source is absent or empty in a verified external identity
- **THEN** authentication fails with `401 Unauthorized` and no local user or membership is created

### Requirement: SSO User Auto-Provisioning and Role Mapping

On first successful external login, the system SHALL auto-create a `User` row with `email`, mark `disabled = false`, hash a random throwaway password, and create a `Membership` in the provider's organization with roles from the provider's group mapping (defaulting to the configured default role, then the organization self-service role). Subsequent logins reuse the existing user and synchronize any roles selected by matching external groups. JWTs MUST always target the selected provider's organization rather than another membership owned by the same user. Every referenced role id MUST be validated as an organization role in the provider's organization when the provider is saved.

#### Scenario: First login provisions Viewer
- **WHEN** a new email logs in via SSO and no `role_mapping` entry matches
- **THEN** a `User` + default-org `Membership { role: Viewer }` is created in one transaction

#### Scenario: Group claim promotes to Editor
- **WHEN** the IdP claim `groups` includes `molesignal-editors` and `role_mapping = { "molesignal-editors": "editor" }`
- **THEN** the existing membership's role is upgraded to `Editor` (and recorded in audit); never downgraded by a missing claim

#### Scenario: Default role handles an unmatched external group
- **WHEN** no external group mapping matches and the provider configures `default_role_id`
- **THEN** the provider-organization membership is synchronized to that default role

#### Scenario: Role mapping cannot cross organizations
- **WHEN** an administrator saves a group or default role mapping that references a role from another organization
- **THEN** the provider update is rejected and the previous mapping remains unchanged
