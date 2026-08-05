## ADDED Requirements

### Requirement: Dashboard Authoring Capability Activation

Copilot Chat SHALL support a `dashboard_authoring` purpose/skill. An explicit Dashboard starter or `analysis_mode = dashboard` SHALL activate it deterministically; free-form Dashboard creation intent SHALL be classified through the registered capability catalog. Activation SHALL inject the version-compatible authoring instructions and expose only tools allowed by the active Agent Profile, Toolset, network policy, and execution policy.

#### Scenario: Explicit Dashboard starter activates authoring
- **WHEN** a user starts chat from the Build Dashboard capability
- **THEN** the request selects `dashboard_authoring`, loads its current instructions, and advertises the enabled Dashboard discovery/preparation/proposal tools

#### Scenario: Ordinary investigation does not load authoring instructions
- **WHEN** a user asks a logs-only investigation question with no Dashboard creation intent
- **THEN** Dashboard authoring instructions are not injected and the normal investigation tool workflow remains unchanged

### Requirement: Provider-Neutral Tool Choice

The provider-neutral completion request SHALL represent `auto`, `none`, `required`, and a specific tool choice. OpenAI, Anthropic, and OpenAI-compatible adapters SHALL map those values to their native request formats without changing the advertised tool schemas. After a forced first tool succeeds, subsequent Agent loop calls SHALL return to the capability's configured automatic selection mode.

#### Scenario: Dashboard preparation is forced after routing
- **WHEN** Dashboard authoring routing determines that the request has enough information to prepare a draft
- **THEN** the first authoring completion forces `prepare_dashboard` instead of allowing a text-only answer

#### Scenario: Provider receives native tool-choice shape
- **WHEN** the same specific tool choice is sent through OpenAI and Anthropic adapters
- **THEN** each outbound request contains the provider's native equivalent and both adapters surface the resulting tool call through the existing Agent loop

### Requirement: Dashboard Skill and Tool Contract Compatibility

The active Dashboard skill version SHALL declare the authoring contract versions and tool names it requires. The chat backend SHALL activate it only when all required tools are enabled and at least one compatible contract version is available; otherwise it SHALL return a user-facing capability limitation without asking the model to fabricate Dashboard JSON.

#### Scenario: Required proposal tool is disabled
- **WHEN** `prepare_dashboard` is enabled but `propose_dashboard_creation` is disabled by the Agent Profile
- **THEN** chat may prepare and preview a draft but clearly reports that it cannot submit the creation operation

#### Scenario: Skill and compiler versions do not overlap
- **WHEN** the selected skill supports authoring contract v1 but the deployed compiler supports only v2
- **THEN** authoring activation fails closed with a version compatibility error before a model completion is sent

