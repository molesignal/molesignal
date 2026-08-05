## Why

Mole Agent 已经能够发现并调用受控的内置/MCP 工具，但 Dashboard 的完整结构和校验主要维护在前端，后端只做顶层检查，模型无法可靠地产生可执行、可升级的 Dashboard。需要一条服务端拥有契约、校验、预览和确认执行权的 Dashboard authoring 链路，让运行时没有源码上下文的 AI 也能安全创建 Dashboard。

## What Changes

- 新增版本化 `DashboardAuthoringSpec`，作为面向 AI 的小型语义契约；服务端负责补齐 ID、布局、默认 visualization options 和持久化元数据，并编译为当前 `DashboardDefinition`。
- 将 Dashboard JSON Schema 收敛为单一、语言无关的事实来源，后端和前端消费同一契约；后端在 create/update/authoring 边界执行完整结构与领域语义校验。
- 新增只读 `get_dashboard_capabilities` 与 `prepare_dashboard` 工具：发现当前契约/visualization/query 能力，编译草稿，校验 panel/query，执行受限 dry-run，并返回可修复的结构化问题、稳定 hash 和预览信息。
- 扩展受控操作注册表以支持 `create_dashboard`：AI 只能基于未过期且 hash 匹配的草稿提出创建操作，最终由现有确认/审批执行链路完成持久化，并提供幂等保护和审计。
- 新增 `dashboard_authoring` prompt/skill capability 与意图激活规则；显式 Dashboard 入口确定性激活，自由文本经 capability routing 激活，并只向模型暴露所需工具。
- 为 provider-neutral completion 增加 tool choice，允许 Dashboard authoring 首步强制调用 `prepare_dashboard`，避免模型仅输出不可执行的 JSON 或文字建议。
- 继续使用 Agent Profile、Toolset、IAM、风险策略和可信 `ToolAuthContext` 收窄能力；模型参数不得提供或覆盖 `org_id`、`user_id`、审批人等身份信息。

## Capabilities

### New Capabilities

- `ai-dashboard-authoring`: 面向 Mole Agent 的 Dashboard authoring 契约、草稿编译/校验/预览、skill 激活、工具选择以及确认后创建闭环。

### Modified Capabilities

- `dashboard`: Dashboard create/update SHALL 使用统一的版本化契约执行完整结构和领域语义校验，并接受 authoring compiler 生成的模型。
- `copilot-chat`: Chat SHALL 支持 Dashboard authoring purpose/capability routing、provider-neutral tool choice，以及基于有效 Toolset/Profile 自动暴露和调用 Dashboard 工具。
- `copilot-mcp`: 已同步 MCP tool 的输入 SHALL 使用完整 JSON Schema 校验，并把 schema 版本/hash 绑定到实际执行，避免只检查顶层字段。

## Impact

- 后端：Dashboard domain/application validation、Mole Agent tool registry/dispatcher、prompt resolution、provider adapters、approval execution、bootstrap wiring 和 persistence repositories。
- 前端：Dashboard schema/types 生成或消费方式、AI Dashboard starter、草稿预览/确认入口和结构化错误展示。
- 数据：新增组织隔离、带 TTL 的 Dashboard authoring drafts 持久化；开发期 migration 继续折叠至现有 initial migration。
- 契约：新增 Dashboard model/authoring schema 及版本/hash；现有合法 `schemaVersion = 2` Dashboard 保持兼容，不引入 API breaking change。
- 安全：创建操作要求 `dashboards.create`，folder 和 draft 必须属于认证上下文中的组织；所有模型输出在 dry-run 和最终执行时重复校验。
