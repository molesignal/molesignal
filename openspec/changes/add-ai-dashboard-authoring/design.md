## Context

Dashboard Engine 当前以 TypeScript interfaces、手写 validator、`DASHBOARD_JSON_SCHEMA` 和一份独立 `dashboard.schema.json` 描述持久化模型；后端领域对象把 `model` 保存为 `serde_json::Value`，application service 只检查 engine、schemaVersion 和少量顶层容器。这使交互式前端可以工作，但相同模型从 AI/tool 边界进入时缺少完整、统一、可审计的契约。

Mole Agent 已具备 provider adapters、流式 Agent loop、编译期内置工具、远端 MCP tool 同步、Agent Profile/Toolset 白名单、风险执行模式、可信 `ToolAuthContext`、审批/执行记录和 tool-call 审计。缺口是 Dashboard 专用的语义输入、服务端 compiler、草稿生命周期、确认执行和 capability 激活；同时现有 MCP argument validator 只检查 required 与顶层 primitive type，不能把同步的 JSON Schema 当作安全边界。

本 change 横跨 Dashboard domain/app/infra、Intelligence tools/chat/control、provider adapters、Postgres 和 Web。实现必须保持组织隔离，不能信任模型提供的身份字段；生产源码按职责拆入专属目录且单文件不超过 500 行；开发期 schema 继续折叠进现有 initial migration。

## Goals / Non-Goals

**Goals:**

- 让 Mole Agent 在没有仓库源码上下文时，仅依赖 versioned skill instructions、tool descriptions 和 JSON Schema 发现并创建原生 Dashboard。
- 每份 Dashboard 模型只存在一个规范化结构契约，并在后端写边界和前端导入/编辑边界使用同一 schema revision。
- 用稳定、较小的 `DashboardAuthoringSpec` 隔离模型与 renderer 内部字段，让 compiler 负责默认值、ID、布局和 visualization 版本。
- 在最终持久化前提供 query-aware prepare、结构化修复问题、只读预览、hash/TTL 和显式确认/审批。
- 复用 Agent Profile、Toolset、IAM、风险策略、审批执行、幂等和审计，不新增绕过这些控制面的写入路径。
- 让 provider abstraction 能表达强制工具选择，保证已识别的 Dashboard authoring 不退化为纯文本 JSON。
- 把远端 MCP tool 输入校验升级为完整、有限制、与 advertised schema revision 一致的 JSON Schema 校验。

**Non-Goals:**

- 不让 AI 直接编辑任意已有 Dashboard；update/patch authoring 留作后续能力。
- 不实现通用的第三方 skill marketplace、任意脚本、Shell、Browser 或开放 HTTP 执行。
- 不在本 change 中新增对外 MCP server endpoint；内置 AI 使用本地 builtin dispatcher，未来外部 MCP adapter 复用同一 application service。
- 不让模型生成 Grafana JSON，也不改变 Grafana import 保留未知 vendor 字段的兼容要求。
- 不保证 dry-run 返回数据；空结果是 warning，契约/权限/查询有效性才是阻断条件。
- 不在 prompt 中复制完整 Dashboard schema、visualization defaults 或租户数据结构。

## Decisions

### 1. 每个契约只有一个规范源，生成物不得手改

新增语言无关的契约目录：

```text
contracts/dashboard/
  model/v2.schema.json
  authoring/v1.schema.json
  visualizations/v1.json
  fixtures/{valid,invalid}/...
```

`model/v2.schema.json` 是持久化/渲染模型规范源；`authoring/v1.schema.json` 是模型 tool input 规范源；`visualizations/v1.json` 保存 compiler 和前端 registry 共同需要的 type、option schema version、default options、允许的 query/data shape 与默认尺寸。schema 使用稳定 `$id`，Dashboard model version 使用 `const: 2`，而不是宽松的 `minimum: 1`。

后端以 `include_str!` 嵌入这些资源并在 bootstrap 时编译 validator；前端通过生成脚本复制为只读 generated module，并使用 Ajv 2020 校验。TypeScript 领域类型继续提供易读的 API，但 CI 使用 schema-derived type/共享 fixture 双向校验，禁止 schema、类型和手写语义 validator 静默漂移。authoring Rust structs 使用 `serde(deny_unknown_fields)`，其序列化结构由 contract fixture 锁定。

替代方案是把完整 Dashboard model 改为 Rust code-first 并生成 TypeScript。未采用，因为 renderer/visualization option 的演进归属前端，强制把全部插件配置建模到 Rust 会制造第二个实现中心。另一个替代方案是继续以 TypeScript 常量为真相；未采用，因为后端和 MCP runtime 无法可靠消费 TypeScript 源码。

### 2. JSON Schema 只验证结构，领域语义由服务端 validator 负责

共享 `ContractValidator` 使用标准 JSON Schema evaluator 执行结构校验，并把错误归一化为：

```json
{
  "code": "INVALID_GRID_POSITION",
  "path": "/panels/2/layout",
  "message": "panel exceeds the 24-column grid",
  "retryable": true
}
```

Dashboard semantic validator 继续检查 schema 无法或不宜表达的约束：递归 element ID 唯一、grid bounds、refId 唯一、变量引用、visualization/query 兼容、refresh 组合规则、query length/panel count budgets 和安全 renderer invariants。`DashboardService::create/update` 与 authoring compiler 共享该 validator；API route、tool dispatcher 和 frontend 不复制这些规则。

Grafana import 在转换/规范化后检查安全结构不变量，但保留未知 vendor extension。原生 model 与 authoring contract 默认拒绝未知输入字段；持久化 model 对明确 extension points 保持兼容。

### 3. Authoring DSL 表达意图，compiler 拥有机械字段

`DashboardAuthoringSpec v1` 包含：title/description/tags、relative time range、refresh preference、可选 folder、variables、text sections 和 panels。Panel query 是 discriminated union：PromQL、stream-scoped read-only SQL、trace/profile query；visualization 是 catalog enum 加少量语义配置（unit、reducer、thresholds、legend、size hint）。

compiler 负责：

- 生成 dashboard/element/tab/ref IDs；
- 把顺序和 `small|medium|wide|full` size hint 确定性排入 24-column grid；
- 从 visualization manifest 合并 option schema version/defaults；
- 把 typed query 转成当前 `PanelQuery` record；
- 补齐 time/refresh/layout/empty collections/editable/default flags；
- 规范化 JSON key order 后计算 model hash。

第一版只广告 compiler 真正实现的 visualization/query 组合。新增 renderer 类型时先更新 manifest、compiler test 和 capability catalog，不需要改 Dashboard skill 正文。

替代方案是把完整 `DashboardDefinition` 直接作为 tool input。未采用，因为字段过多、默认值随前端演进、模型容易生成重复 ID/越界布局，且会把 renderer 私有结构永久暴露为 AI API。

### 4. Prepare 是只读风险的有状态草稿流程

新增 `DashboardAuthoringService::prepare`，按固定流水线执行：

1. authoring JSON Schema；
2. typed deserialize 与预算限制；
3. compiler；
4. Dashboard model schema + semantic validation；
5. 根据可信 org 查询 stream/schema；
6. parse/plan 每个 query，并在有限 time range、row/byte/timeout budget 下 dry-run；
7. 将 valid draft 持久化并返回摘要、warnings、hash、expiry 和 preview route。

invalid/error 不持久化 executable draft。query 语法、权限、未知 stream/field 是 blocking issue；合法但空结果是 warning。为避免 Agent 无限自修复，skill 指令限制同一 draft 最多两次 prepare 修正，Agent loop 的总 tool budget 仍是硬上限。

`get_dashboard_capabilities` 和 `prepare_dashboard` 是 L0 builtin tools。prepare 虽写入临时 draft metadata，但不会创建或改变用户业务资源，其写入仅用于完整性/TTL/审计，因此保持 read-only execution policy 可用；它仍要求 `intelligence.use`、`dashboards.create`，dry-run 还要求对应 query 权限。

### 5. Draft 使用 Postgres JSONB、canonical hash 和一次性消费

新增 `intelligence_dashboard_drafts`：

- `id`, `org_id`, `created_by`
- `authoring_version`, `model_schema_version`, `compiler_version`
- `authoring_spec`, `compiled_model`, `model_hash`
- `folder_id`, `status = ready|consumed|expired`
- `dashboard_id`, `created_at_micros`, `expires_at_micros`, `consumed_at_micros`

默认 TTL 为 30 分钟，并受合理上下限配置约束。hash 对 recursively key-sorted canonical JSON 做 SHA-256；proposal 和 execution 都必须提交/比较 expected hash。执行时还比较 schema/compiler compatibility，部署后无法安全重放的旧 draft 返回 `DRAFT_STALE` 并要求重新 prepare。

Dashboard 创建与 draft `ready -> consumed` 使用同一 Postgres transaction/原子 repository operation，并对 draft 建唯一消费约束。这样即使不同 idempotency key 并发执行，也只会产生一个 Dashboard。现有 execution idempotency key 仍负责同一请求的结果重放。

替代方案是把完整 model 放进聊天消息或 approval parameters。未采用，因为 payload 大、重复传输、容易被模型改写，且无法可靠绑定用户看到的 preview。

### 6. 模型只能 propose，最终创建复用受控 operation

新增 builtin tools：

- `get_dashboard_capabilities`：L0 / read-only；
- `prepare_dashboard`：L0 / read-only；
- `propose_dashboard_creation`：L1 / creates approval request。

proposal tool input 只含 `draft_id`, `expected_hash`, `reason`, `impact`，不接受 model、folder、org/user 或审批字段。它注册 `create_dashboard` operation，target 为 draft ID。Dashboard creation 的执行模式硬下限是 `Confirmation`：组织策略可以收紧到 single/dual approval 或 disabled，但不能降为 automatic。Confirmation 生成零 reviewer 的 approved proposal，但仍需要请求者显式执行；single/dual approval 沿用 review 流程。

执行器在 match action 之前加载 action-specific target，避免当前 alert-only executor 先读取 Incident。`create_dashboard` 分支重新加载 draft、比较 org/actor/hash/TTL/status、要求 `dashboards.create`、验证 folder、重跑当前 schema/semantic validation，再通过 Dashboard application service 的 atomic create-from-draft 路径持久化，并发出既有 federation CUD 和 activity audit。

Confirmation 的请求者可在拥有 `intelligence.use` 与 action permission 时执行自己的 proposal；需要 reviewer 的模式仍要求 `intelligence.approve` 并满足人数。Advice-only/Read-only chat 不广告或不允许 proposal tool。

### 7. Skill 是版本化编排说明，不是执行边界

增加内置 capability manifest `dashboard-authoring`：id/version、purpose、trigger summaries、negative examples、required/optional tools、authoring contract range、step budget 和 instruction template key。实际 instruction 作为不可变 builtin prompt `dashboard.authoring.v1` 存入现有 prompt catalog，org override 仍走 prompt version/hash 审计。

激活顺序：

1. API 显式 `capability = dashboard_authoring` 或 `analysis_mode = dashboard`；
2. Dashboard starter 确定性设置该字段；
3. 自由文本由只读 capability router 对 manifest summaries 做高置信匹配；低置信时保持普通 chat，并允许模型在 auto mode 自行调用已授权 discovery tool。

激活前检查 required tools 与 contract version overlap。若 proposal tool 被禁用，可以降级为 prepare/preview-only；若 compiler/skill contract 不兼容，则在调用 provider 前 fail closed。skill 不包含 org 数据，不承担参数校验、IAM 或风险决策。

替代方案是让模型先调用通用 `load_skill`。未采用，因为这会把能否发现关键流程再次交给模型，并额外消耗一次 tool loop。

### 8. CompletionRequest 显式表达 tool choice

增加 provider-neutral `ToolChoice::{Auto,None,Required,Specific(String)}`。OpenAI/OpenAI-compatible 映射到 `tool_choice`，Anthropic 映射到其 native `tool_choice`。Agent loop 接受 initial choice：Dashboard router 判断信息足够时首轮强制 `prepare_dashboard`；该 call 完成后恢复 `Auto`，避免每轮重复强制。同一 request 指定的 tool 必须存在于当次 filtered tool schema，否则在访问 provider 前报错。

如果用户信息不足，skill 先用自然语言询问缺失的主题/数据源/时间范围，不强制 prepare。provider 不支持 specific choice 时 adapter 返回 capability error，而不是静默降级。

### 9. MCP schema 在 sync 时编译，advertise 与 execute 绑定同一 revision

把 `mcp::validate_schema` 替换为共享 JSON Schema validator。MCP sync 对每个 input schema：规范化 dialect、禁止 remote `$ref`、限制 schema bytes/depth/ref depth、编译 validator、计算 canonical hash，并保存 hash/synced_at/schema dialect。失败的 tool 标记 unavailable，不进入 Agent Profile 可选项。

Chat schema 从持久化 revision 生成；execute 读取同一 tool record 并用相同 hash 对应 schema 校验后才发 `tools/call`。支持 nested required、additionalProperties、enum/const、bounds、oneOf/anyOf/allOf 和 local `$ref`。校验错误输出有限数量的 JSON Pointer issues，避免把 remote schema 或敏感 payload 全量写入日志。

替代方案是相信 provider 的 constrained decoding 或远端 MCP server 自行校验。未采用，因为 provider 仍可能生成无效参数，且把无效/注入 payload 发往远端会绕过本地 fail-closed 与审计语义。

### 10. Preview 使用现有 DashboardRenderer，不建立第二套渲染器

新增 org-scoped draft read endpoint和 `/ai/dashboard-drafts/:id` 预览 route。Web 读取 compiled model，通过现有 `DashboardRenderer` 以不可编辑模式展示，并同时显示 prepare warnings、expiry 和 Confirm/Create 动作。确认后创建 approval/execution；成功 route 跳转 `/dashboards/{id}`。

preview 不执行超出正常 Dashboard viewer 权限的查询，也不信任 tool result 中内联的大 model。草稿 endpoint 重新执行 org、creator/permission 和 TTL 检查；页面只使用服务端 persisted model/hash。

### 11. 模块按领域职责拆分

建议结构：

```text
src/domain/dashboard/authoring/{mod.rs,contract.rs,draft.rs,repositories.rs}
src/app/dashboard/authoring/{mod.rs,compiler.rs,validation.rs,service.rs}
src/infra/persistence/repositories/dashboard_authoring.rs
src/intelligence/capabilities/dashboard_authoring/{manifest.json,instructions.md}
web/src/dashboard-engine/contracts/generated/
web/src/routes/intelligence/dashboard-authoring/
```

通用 JSON Schema evaluator 归属明确的 contract validation 模块，不放入 `utils`。Tool schema、HTTP API 和 compiler 调用 application service，不能反向依赖 Web 或 protocol DTO。

## Risks / Trade-offs

- **[Risk] 统一 schema 会暴露历史已存模型的不一致。** → 读取路径继续使用现有 tolerant upgrade；发布前用共享 fixtures 和内置 Dashboard 数据跑审计，严格校验只阻断新 create/update/execute。
- **[Risk] AI 生成的查询可能昂贵或扫描过多。** → prepare 使用独立 timeout/row/byte/lookback budgets，查询 planner 保持只读限制，超预算作为 blocking issue，最终 Dashboard 正常运行仍受 query runtime controls。
- **[Risk] Draft 与部署版本竞争导致 preview 和 execute 不一致。** → draft 记录 contract/compiler versions 与 hash；不兼容部署后执行返回 stale 并要求重新 prepare。
- **[Risk] Confirmation/approval 扩展影响既有 alert operation。** → 保留现有 alert action policy和测试；action target loader、required reviewers 和 executor 分支使用 registry 驱动的 focused tests。
- **[Risk] 强制 tool choice 让模型在信息不足时生成低质量参数。** → router 只在 completeness check 通过时指定 `prepare_dashboard`，否则先追问；specific tool 必须在 filtered registry 中。
- **[Risk] 完整 remote JSON Schema 可造成编译/验证资源消耗。** → sync 阶段限制 byte/depth/ref count，禁止 remote refs，缓存 compiled validator，执行限制 issue 数量和时间。
- **[Risk] 新增 Ajv/jsonschema 依赖增加体积和维护面。** → 使用成熟 draft-2020 实现并集中封装；不在每个模块引入独立 validator。
- **[Trade-off] 第一版 DSL 无法表达 Dashboard Engine 所有高级 options。** → capability catalog 只广告稳定语义子集；用户创建后仍可在 editor 中继续编辑，后续按 contract version 增量扩展。

## Migration Plan

1. 建立 canonical contracts、visualization manifest、共享 valid/invalid fixtures 和生成/漂移检查；让现有内置/测试 Dashboard 全部通过 model v2 validator。
2. 引入后端/前端 JSON Schema evaluator，在 native create/update 写边界启用完整校验；读取历史模型仍保持 tolerant upgrade。
3. 在 initial migration 增加 `intelligence_dashboard_drafts`、hash/status/TTL/唯一消费约束和所需 repository；不创建新的增量 migration。
4. 实现 domain authoring types、compiler、semantic validation、query dry-run 和 atomic create-from-draft application path。
5. 注册三个 builtin tools、`create_dashboard` operation、风险/确认执行和 tool-call/activity/federation audit。
6. 增加 capability manifest、builtin prompt、request capability/analysis mode、router 与 provider-neutral tool choice，并更新 Agent Profile/Toolset 管理表面。
7. 增加 draft preview API/route、AI starter、warning/error/confirmation UI 和成功跳转。
8. 将 MCP sync/execute 切换到共享完整 schema validator，并回归现有远端 MCP fixture。
9. 执行 focused Rust/frontend tests、共享 contract corpus、集成 chat tool loop、并发幂等、tenant isolation、OpenSpec validation 和一轮收尾验证。

回滚时先禁用 Dashboard authoring capability/tools，停止产生新 draft；现有 Dashboard CRUD 和普通 Agent 工具继续工作。新增 draft 表与 schema assets 可保留未使用。若严格 write validation 出现兼容问题，可临时退回旧写 validator，但不得执行未通过 authoring hash/tenant 检查的 draft。已经创建的 Dashboard 是正常原生资源，不依赖 skill 或 draft 表读取。

## Open Questions

无。第一版固定 authoring v1、Dashboard model v2、30 分钟 draft TTL、显式确认硬下限以及“创建新 Dashboard、不编辑既有 Dashboard”的范围；高级 option 和外部 MCP server adapter 后续独立演进。
