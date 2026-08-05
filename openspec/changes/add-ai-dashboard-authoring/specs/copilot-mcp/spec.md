## ADDED Requirements

### Requirement: Complete MCP Tool Input Contract Validation

The system SHALL validate synchronized MCP tool input schemas and tool-call arguments with a standards-compliant JSON Schema evaluator for the supported dialect. Validation SHALL enforce nested constraints including `required`, `additionalProperties`, `enum`, `const`, numeric/string/array bounds, composition keywords, and local `$ref`. Invalid or unsupported schemas SHALL fail closed during synchronization, and invalid arguments SHALL NOT be sent to the remote MCP Server.

#### Scenario: Nested invalid MCP arguments are rejected locally
- **WHEN** a synchronized tool schema requires an array item with an enum value and the model supplies an unsupported value
- **THEN** the dispatcher returns a structured input validation error and does not issue `tools/call` to the remote server

#### Scenario: Unsupported remote schema fails synchronization
- **WHEN** an MCP Server advertises a malformed schema or a schema dialect/remote reference the runtime does not permit
- **THEN** synchronization marks that tool unavailable with a diagnostic and does not advertise it to chat providers

### Requirement: MCP Tool Schema Integrity

Each synchronized MCP tool SHALL persist a canonical schema hash and synchronization metadata. The provider-advertised schema and execution validator SHALL use the same persisted schema revision; a server-side schema change SHALL require a successful resynchronization before new arguments are accepted.

#### Scenario: Advertised and executed schema revisions match
- **WHEN** chat advertises an MCP tool with schema hash H and the model calls it
- **THEN** execution validates the arguments against the persisted schema whose canonical hash is H

#### Scenario: Tool schema exceeds safety limits
- **WHEN** a remote schema exceeds configured byte, nesting, or reference-depth limits
- **THEN** synchronization rejects the tool without compiling or advertising the oversized schema

