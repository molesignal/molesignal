# Inbound MCP Server

MoleSignal exposes a tenant-aware MCP server at:

```text
https://<molesignal-host>/api/v1/mcp
```

The server uses Streamable HTTP. It supports protocol versions `2026-07-28`,
`2025-11-25`, `2025-06-18`, and `2025-03-26`. The old HTTP+SSE transport and
stdio are intentionally not exposed.

## Enable the server

Inbound MCP requires the licensed Agent feature and is enabled by default for
eligible organizations. An organization administrator with `agent.manage` can
change the setting at **Mole Agent → Settings → Inbound MCP**.

The page also controls exact browser Origin allowlisting and the following
per-credential limits:

- request body: 1 MiB by default, 8 MiB hard maximum;
- response body: 1 MiB by default, 8 MiB hard maximum;
- concurrent HTTP calls: 8 by default;
- HTTP calls per minute: 60 by default;
- read-tool timeout: 30 seconds by default.

Changes are read when the next HTTP request starts. An already-running request
keeps its existing settings snapshot.

## Authentication and tenant identity

Send one of these credentials as `Authorization: Bearer <credential>`:

- a personal API token;
- an API token bound to a Service Account;
- an OAuth 2.1 access token issued by MoleSignal.

Do not put a user ID or organization ID in the MCP URL, MCP arguments, or a
custom header. MoleSignal derives both identities from the authenticated
credential, rechecks the user, Service Account, and organization state, and
loads the current IAM snapshot on every request. An MCP session is bound to the
credential that initialized it and cannot be reused with another credential.

The effective tool set is the intersection of:

1. tools exposed on the Inbound MCP surface;
2. the credential's current IAM permissions;
3. the organization's current Tool Policy.

Agent Profiles do not restrict Inbound MCP. Configured outbound MCP servers and
their remote tools are not proxied through this endpoint.

## OAuth 2.1

Protected-resource metadata:

```text
/.well-known/oauth-protected-resource/api/v1/mcp
```

Authorization-server metadata:

```text
/.well-known/oauth-authorization-server
```

MoleSignal supports Authorization Code with PKCE `S256`, refresh-token
rotation and family revocation, RFC 8707 resource binding, dynamic client
registration, and HTTPS client ID metadata documents. Public clients use
`token_endpoint_auth_method=none`; confidential clients can use the exact
registered `client_secret_basic` or `client_secret_post` method.

The interactive authorization endpoint is `/oauth/authorize`. It requires a
logged-in human user; API tokens and Service Accounts cannot approve an OAuth
grant. Access tokens live for one hour. A refresh token is issued only when
`offline_access` is granted and the client registered the `refresh_token`
grant. Refresh tokens live for 30 days and rotate on every use. Reuse revokes
the whole token family.

OAuth and credential responses use `Cache-Control: no-store`. Active OAuth
connections can be reviewed and revoked from the Inbound MCP settings page.

## Tools and managed changes

`tools/list` keeps the initial context small. It exposes seven pinned read
tools, the approval lifecycle, and three adapters:

- `tool_search` discovers the complete authorized catalog;
- `call_read_tool` runs an authorized read or preflight tool;
- `call_managed_tool` proposes or automatically executes an authorized change.

Every `call_managed_tool` request requires an `idempotency_key`. MoleSignal
reserves it for the authenticated principal before creating an approval. Using
the same key with different arguments is rejected; repeating the same request
returns the persisted result or its in-progress state.

Tool Policy determines the real confirmation behavior:

- automatic mode creates an auditable approval record and executes it;
- confirmation mode creates an approval that the MCP host confirms through a
  multi-round form before `execute_agent_approval` runs;
- single- and dual-approval modes wait for MoleSignal approvers before
  `execute_agent_approval` can run. Once the required reviews are complete, the
  original MCP requester can finish the operation without receiving approver
  permission; another principal still needs `agent.approve`.

Executing an approval also requires an `idempotency_key`. API-token plaintext,
OAuth secrets, and one-time credential results are redacted from MCP results
and tool-call audit records. `create_api_token` and `create_service_account`
stay off the Inbound MCP surface because both must deliver a new plaintext API
token; create them in the Web UI. Inbound MCP also refuses to execute an
existing credential-producing approval created on another surface, preventing
the one-time secret from being consumed without a Web delivery channel.
Existing Service Accounts and API tokens can still be listed, updated, enabled,
disabled, revoked, and deleted through their authorized non-secret tools.

## Resources, prompts, tasks, and notifications

The server exposes IAM-filtered resources and resource templates for platform
capabilities, the tool catalog, approvals, executions, search jobs, and stream
schemas. Organization identity is never accepted as a resource parameter.

Enabled built-in, organization, and current-user Mole Agent prompts are
available through `prompts/list` and `prompts/get`.

Clients advertising the Tasks extension can set `as_task=true` on
`call_read_tool`, then poll, update, or cancel the durable task. Long tool calls
emit progress notifications when the request supplies a progress token.

Protocol `2026-07-28` clients can use `subscriptions/listen` and multi-round
input requests. Legacy protocol clients can use `resources/subscribe` and
`resources/unsubscribe`. Catalog changes and subscribed resource refreshes are
delivered over the session's SSE stream.

## Browser Origin and Host checks

A request without `Origin` is allowed for non-browser MCP clients. A browser
request must be same-origin or match an exact HTTP/HTTPS Origin configured by
the organization; otherwise MoleSignal returns `403`.

For non-loopback deployment, configure MoleSignal's external URL. The request
`Host` must match it. This prevents DNS-rebinding attacks against local MCP
servers.
