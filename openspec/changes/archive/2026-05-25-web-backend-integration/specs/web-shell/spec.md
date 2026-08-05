## MODIFIED Requirements

### Requirement: Auth Bootstrap

On app boot the shell SHALL read the JWT or `ms_` API token from secure storage, exchange it for `AuthContext { user_id, org_id, role }`, and either render the authenticated shell or redirect to `/login` preserving the original URL in `?next=`. **The Login form SHALL collect email + password only — no `workspace` field — and on success route the user to the `?next=` URL (defaulting to `/home`); the JWT's `org_id` claim is the authoritative current-org source.**

#### Scenario: Unauthenticated deep link

- **WHEN** an unauthenticated user navigates to `/investigate?time=-2h..now`
- **THEN** the app redirects to `/login?next=%2Finvestigate%3Ftime%3D-2h..now`; after successful login the user lands back on the original URL

#### Scenario: Token expiry mid-session

- **WHEN** any API call returns `401 token expired`
- **THEN** the shell clears stored tokens and redirects to `/login?next=<current>`; in-flight queries are cancelled

#### Scenario: Login form has no workspace field

- **WHEN** an unauthenticated user opens `/login`
- **THEN** the form renders exactly two text inputs (`email`, `password`) plus a primary "Sign in" button and an "Continue offline (dev)" link
- **AND** no `workspace` / `org` selector is shown on the form

#### Scenario: 401 also fires on org-switch failure

- **WHEN** `POST /api/v1/orgs/{id}/select` returns 401
- **THEN** the http interceptor logs out the user and navigates to `/login?next=<current pathname + search>`
