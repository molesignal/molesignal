# MoleSignal Tool Runtime

MoleSignal 的平台工具由一份协议中立的 catalog 和一个应用层 runtime 提供。内置
Mole Agent 使用它；后续入站 MCP Server 也必须复用它，不能另写一套查询、权限或
风险判断。

## 分层

```text
Mole Agent adapter ─┐
                    ├─ surface catalog ─ builtin coordinator ─ ToolRuntime ─ domain/application ports
Inbound MCP adapter ┘                         └─ approval/execution coordinator
```

- `crates/modules/tool-runtime`：稳定工具名、JSON Schema、输出契约、风险、权限提示、
  exposure、可信调用上下文和协议中立 dispatcher trait。
- `bin/molesignal/src/app/tools`：权限和 license 复核、参数校验、查询上限、租户隔离、
  surface exposure 硬校验、审批请求创建及各领域 handler。即使客户端绕过 `tools/list`
  按名字调用未暴露 Tool，runtime 也会拒绝。
- `api/http/routes/agent/builtin_execution.rs`：两种入口共用的执行策略、审批创建、Automatic
  执行与显式执行闭环；adapter 不得自行拼接这些步骤。
- `api/http/routes/agent/tool_dispatcher.rs`：仅保留 Mole Agent 的 Profile/Toolset、
  `tool_search` / `tools_call`、出站 MCP 和 Agent 调用审计。
- 未来 MCP Server：把认证结果转换成 `ToolInvocationContext`，用
  `tools_for_surface(ToolSurface::InboundMcp)` 实现 `tools/list`，用共享 builtin
  coordinator 实现 `tools/call`。不得直接调用领域 repository，也不得接受 arguments
  中的 `org_id`、`user_id`、执行模式或审批人数。

`ToolInvocationContext` 的 `user_id` / `org_id` 是私有字段，只能通过
`ToolInvocationContext::from_iam(&IamContext)` 从服务端认证完成的 IAM 上下文
派生。Inbound MCP adapter 必须先完成 bearer/session credential 验证和 IAM context
解析，再创建 invocation；JSON-RPC params、tool arguments 及未经认证的 HTTP Header
都不能成为身份来源。Chat、investigation、request ID 与执行策略只能通过 builder
补充，不能改变已经绑定的身份。`ToolExecutionMode` 默认 `Disabled`；入口没有从组织策略
解析并显式注入模式时，runtime 必须 fail closed。

Service Account 是不可交互登录的 IAM principal，不具备密码、Session 或 JWT 登录链路；
API Token 是独立的凭证资源。创建 Service Account 时，服务端必须在同一数据库事务内
创建一个绑定该 principal 的初始 API Token，明文只在创建响应中返回一次。之后 Token 的
列出、追加签发和撤销仍归 `api_tokens.*` 权限与 `/auth/tokens` API 管理；用户个人 API
Token 只通过 API Token 管理入口手动创建。

`create_service_account` 由执行策略决定是立即执行、等待确认还是等待审批。执行成功时，初始 Token 明文通过响应的
`one_time_result` 返回，并在持久化执行记录前剥离；执行历史、审计事件、`list_api_tokens`
以及 MCP 的后续读取都只包含 Token 元数据。

## 执行与审批闭环

所有命名写 Tool 先创建可审计的 ApprovalRequest，再按服务端解析的模式推进：

- `automatic`：同一次 Tool 调用立即执行并返回 Execution；
- `confirmation`：返回已批准但未执行的请求，由用户界面或 inbound MCP 的
  `execute_agent_approval` 显式执行；
- `single_approval` / `dual_approval`：最后一票审批自动触发幂等执行，并在审批响应中返回
  Execution；
- `disabled`：创建审批前拒绝。

`get_agent_approval`、`get_agent_execution` 提供状态闭环。`execute_agent_approval` 只暴露给
inbound MCP；Mole Agent 不能调用它自行绕过 confirmation，内置界面通过受认证的审批 API
完成显式确认。一次性凭据只存在于触发执行的响应中，持久化前剥离，Tool 审计摘要递归脱敏。
幂等键一旦绑定某个 Approval 就不能用于另一个 Approval；重试只返回同一执行记录，不重复
推进领域操作。

## Tool metadata 与国际化

`ToolSpec` 只包含稳定的机器契约和 canonical en-US 兜底文案：`display_name` 从稳定
工具名生成，`description` 同时供 Mole Agent、`tool_search` 和未来 MCP Server 使用。
Catalog 不包含 `description_zh` 等 locale 专用字段，也不根据请求语言改变 MCP schema。

管理 UI 使用工具名查找 `agent:tools.<tool_name>.title` 与
`agent:tools.<tool_name>.description`，翻译资源位于
`web/src/i18n/en-us/agent.json` 和 `web/src/i18n/zh-cn/agent.json`。不存在翻译时回退到
服务端的 canonical metadata；外部 MCP 工具继续使用 MCP Server 返回的 metadata。
新增或删除内置工具时必须同步两种 locale，并通过 `pnpm -C web i18n:check` 检查 key
集合一致性。

## 工具颗粒度

工具按“一个明确调查意图、一个有界结果”划分，而不是把底层 HTTP API 一比一暴露：

- 发现与 schema 分开：`list_streams` 返回紧凑列表，`get_stream_schema` 返回精确字段。
- 搜索与详情分开：例如 `list_traces` / `get_trace` / `get_trace_dag`、
  `list_incidents` / `get_incident` / `get_incident_rca`。
- 原始查询与高层语义分开：保留 `query_logs` / `query_metrics`，同时提供 APM、拓扑、
  RUM、Profile 和跨信号关联工具。
- 写操作使用领域命名的原子 Tool；创建、更新、删除、生命周期变化和动作触发分别建模，
  实际推进方式由执行策略决定。`propose_*` 只保留为 Mole Agent 兼容/创作入口，不暴露给
  inbound MCP。
- 所有列表、Span、事件、Profile 合并和返回体均设置硬上限；schema 不声明未实现参数。

## 覆盖边界

HTTP route 不是 Tool 的同义词。项目当前有近 400 个 route，其中认证、公开订阅确认、
OTLP/Prometheus intake、Webhook、头像/文件二进制传输、节点 drain、运行时 pprof 等
入口不应交给模型调用。Catalog 覆盖的是 Mole Agent 可安全使用的产品能力：

- 读能力必须具备明确调查意图、可信租户上下文和有界输出；
- 测试/预检能力必须无业务副作用；
- 改变业务状态的能力统一走已注册操作、权限复核、策略、审批、幂等执行、验证和审计；
- 读取与列表工具只返回 Secret 元数据；新建凭据只允许通过明确的一次性响应交付，且不得
  写入审批、执行历史或审计。Connector/Search Job/Audit 中的凭据形态字段会省略或递归脱敏；
- `org_id`、`user_id`、审批人数和调用来源永远由入口 adapter 注入，不能出现在参数中。

明确不直接暴露：登录、注册、密码重置、Token/Secret 明文、原始 intake、外部
Webhook、公开免登录 route、二进制上传下载、节点 drain、pprof、底层任意 HTTP、
shell 和浏览器执行。

## Catalog（190 个）

- Meta / 能力发现：`tool_search`、`tools_call`、`get_platform_capabilities`
- Logs / Metrics / Streams：`query_logs`、`query_metrics`、`list_streams`、
  `get_stream_schema`、`get_stream_settings`、`list_metric_names`、
  `list_metric_label_values`、`list_metric_labels`、`list_metric_series`、
  `search_around`、`search_field_values`
- Traces / AI Sessions：`list_traces`、`get_trace`、`get_trace_dag`、
  `list_trace_sessions`、`get_trace_session`、`list_trace_users`
- Incidents / On-call：`list_recent_alerts`、`list_incidents`、`get_incident`、
  `get_incident_rca`、`get_incident_insights`、`acknowledge_incident`、
  `resolve_incident`、`list_on_call_schedules`、
  `get_on_call_schedule`、`get_current_on_call`
- Alert 配置：`list_alert_rules`、`get_alert_rule`、`create_alert_rule`、
  `update_alert_rule`、`delete_alert_rule`、`test_alert_rule`、
  `trigger_alert_rule`、`list_incident_groups`、
  `get_incident_group`、`list_mute_rules`、`get_mute_rule`、
  `list_escalation_policies`、`get_escalation_policy`
- Notifications：`list_notification_connectors`、`get_notification_connector`、
  `list_notification_policies`、`get_notification_policy`、
  `list_notification_templates`、`get_notification_template`、
  `list_notification_deliveries`、`get_notification_delivery`、
  `retry_notification_delivery`、`acknowledge_notification_delivery`
- APM / Correlation：`apm_overview`、`list_apm_services`、`get_apm_service`、
  `list_apm_transactions`、`get_apm_transaction`、`list_apm_dependencies`、
  `list_apm_errors`、`get_apm_error`、`compare_apm_versions`、`get_apm_health`、
  `correlate_signals`、`get_service_topology`
- RUM：`list_rum_sessions`、`get_rum_session`、`list_rum_actions`、
  `list_rum_errors`、`get_rum_related_traces`
- Profiles：`list_continuous_profiles`、`get_profile_flamegraph`、`compare_profiles`
- Reports / Dashboard 内容：`list_report_templates`、`get_report_template`、
  `list_scheduled_reports`、`get_scheduled_report`、`list_report_deliveries`、
  `list_dashboards`、`get_dashboard`、`create_dashboard`、`update_dashboard`、
  `delete_dashboard`、`list_folders`、`get_folder`、`create_folder`、`update_folder`、
  `delete_folder`、
  `list_annotations`、`get_annotation`、`create_annotation`、`update_annotation`、
  `delete_annotation`、`add_dashboard_panel`、`update_dashboard_panel`、
  `move_dashboard_panel`、`delete_dashboard_panel`
- Dashboard 创作：`get_dashboard_capabilities`、`prepare_dashboard`、
  `propose_dashboard_creation`
- Saved Views / Search Jobs：`list_saved_views`、`get_saved_view`、`create_saved_view`、
  `update_saved_view`、`delete_saved_view`、
  `list_search_jobs`、`get_search_job`、`get_search_job_results`、
  `submit_search_job`、`cancel_search_job`、`retry_search_job`、`delete_search_job`
- Pipelines / Functions / Enrichment：`list_scheduled_pipelines`、
  `get_scheduled_pipeline`、`list_pipeline_runs`、`enable_scheduled_pipeline`、
  `disable_scheduled_pipeline`、`delete_scheduled_pipeline`、`list_functions`、
  `get_function`、`test_function`、`create_function`、`update_function`、
  `delete_function`、`list_enrichment_tables`、`list_enrichment_rows`、
  `get_enrichment_value`
- Patterns / Masking / Data Connectors：`list_log_patterns`、`get_log_pattern`、
  `list_regex_patterns`、`get_regex_pattern`、`list_field_masking_rules`、
  `get_effective_field_masking`、`list_data_connectors`、`get_data_connector`
- Synthetics：`list_synthetic_monitors`、`get_synthetic_monitor`、
  `list_synthetic_revisions`、`list_synthetic_results`、`list_synthetic_locations`、
  `list_synthetic_agents`、`list_synthetic_secrets`、`run_synthetic_monitor`、
  `pause_synthetic_monitor`、`resume_synthetic_monitor`、`archive_synthetic_monitor`
- Status Pages：`list_status_pages`、`get_status_page`、
  `list_status_page_incidents`、`get_status_page_incident`、
  `list_status_page_subscribers`、`list_status_page_deliveries`、
  `list_status_page_automation_rules`、`list_status_page_automation_candidates`、
  `get_status_page_automation_settings`、`archive_status_page`、`restore_status_page`、
  `pause_status_page_automation`、`resume_status_page_automation`
- IAM / Audit：`get_user_profile`、`get_user_preferences`、
  `list_organization_members`、`list_teams`、`list_roles`、`list_audit_events`、
  `get_iam_capabilities`、`list_service_accounts`、`get_service_account`、
  `list_api_tokens`、`create_api_token`、`revoke_api_token`、`create_service_account`、
  `update_service_account`、`enable_service_account`、`disable_service_account`、
  `delete_service_account`
- Agent Control：`list_agent_investigations`、`get_agent_investigation`、
  `list_agent_approvals`、`get_agent_approval`、`execute_agent_approval`、
  `list_agent_executions`、`get_agent_execution`、`list_agent_automations`
- 审批型写操作：`propose_alert_action`、`propose_synthetic_monitor_action`、
  `propose_status_page_action`、`propose_notification_action`、
  `propose_scheduled_pipeline_action`、`propose_operation`

`get_user_profile` 与 `get_user_preferences` 的 `target_user_id` 可省略；省略时读取认证
用户。指定其他用户时，runtime 要求调用者具备 `org.members.read`，并在读取用户或偏好
前确认目标属于认证上下文中的组织。该参数只选择目标资源，不改变调用者身份。

审批型操作当前注册了告警确认/解决/规则手动触发、Annotation CRUD、Dashboard Panel
新增/更新/移动/删除、Search Job 提交/取消/重试/删除、Service Account
创建/更新/启用/禁用/删除、Synthetic 立即运行/暂停/恢复/归档、
Status Page 归档/恢复/自动化暂停与恢复、通知投递重试/确认、定时 Pipeline
启用/禁用/删除。通用 `propose_operation` 是兼容入口；模型应优先使用领域专用工具。

## 暴露策略

Catalog 的 exposure 既用于生成列表，也由 `ToolRuntime::authorize` 在执行前再次强制：

- Mole Agent：可使用 meta、查询、原子写 Tool 和兼容的创作/`propose_*` 入口；不能调用
  `execute_agent_approval`。
- Inbound MCP：可使用查询、预检、原子写 Tool 和审批/执行闭环 Tool；不暴露
  `tool_search`、`tools_call`、Dashboard 创作入口及 `propose_*` 兼容入口。
- Automation：默认不暴露内置 Tool，直到具体 Tool 显式声明 automation surface；未来
  adapter 不能沿用 Mole Agent 的列表。

`get_platform_capabilities` 只返回当前调用 surface 可用的 catalog，不泄漏另一入口的私有
工作流 Tool。

每轮模型请求只固定暴露 meta、常用查询和 Dashboard 工作流工具。其余内置工具与健康、
已授权的出站 MCP 工具通过 `tool_search` 发现，再由 `tools_call` 调用。目标工具仍执行
自己的 Toolset、风险、超时、响应大小、权限、license 与审计策略；meta 工具不能递归
调用自身。

每个 ToolSpec 同时声明 `required_permissions` 与 `permission_mode`（`all` / `any`），
因此 `dashboards.read OR sys.dashboards.read`、`streams.query OR sys.telemetry.read`
不会被错误解释为同时需要两项权限。
