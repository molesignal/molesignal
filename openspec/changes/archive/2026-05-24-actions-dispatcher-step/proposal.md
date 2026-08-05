## Why

Enterprise `actions` crate + OSS `actions` HTTP CRUD 已经接通（spec M1），但 alert escalation 仍然只能派发到 user / schedule / team 三类 `EscalationTarget`。"告警 → 自动跑 webhook / script" 是 PagerDuty / OO 用户的硬需求，缺这个 dispatcher 入口前端建出来的 action 就是死资产。

## What Changes

- **BREAKING**：`EscalationTarget` 枚举增加 `Action { action_id }` 变体；所有匹配该枚举的代码必须新增 arm。
- `EscalationDispatcher` 在派发到 `Action` 目标时，`cfg=enterprise` + `license.has_feature("actions")` 时走 `ActionExecutor::execute(kind, IncidentContext)`，把 `ExecutionResult` 写入 `action_executions` 表；OSS 编译或无 license → 该目标被 skip + warn 日志，escalation step 不被它阻塞继续推进。
- 新增 dispatcher 层 `ActionDispatchAdapter` 把 OSS 的 `Incident → IncidentContext` 映射 + 调 `ActionRepository::record_execution`。
- `EscalationPolicy` JSON schema 增 `action` kind 的样例文档，便于前端 `escalations` 编辑器加入。

## Capabilities

### New Capabilities
<!-- 无新 capability；本 change 是已有 actions + alerting 的纵向打通 -->

### Modified Capabilities
- `alerting`: `EscalationTarget` 添加 `Action { action_id }` 变体；`EscalationDispatcher::tick` 接 ActionExecutor 派发分支。
- `actions`: 新增 dispatcher 适配层 + `record_execution` 接入路径（HTTP CRUD 已就位，本 change 只补"被触发"路径）。

## Impact

- **Domain enum 重排**：`crates/domain/src/alerting/escalation.rs::EscalationTarget` 新增变体，所有 `match` 必须更新（dispatcher / serde 反序列化测试 / Pg `escalation_policies.steps` JSON 编解码）。
- **app 层**：`crates/app/src/alerting/dispatcher.rs::EscalationDispatcher` 新增 `Arc<dyn ActionExecutorPort>`（OSS 默认 noop，enterprise wire 注入 `VrlActionExecutorAdapter`）。
- **infra**：`crates/infra/src/persistence/repositories/actions.rs` 已有 `record_execution`，本 change 在 dispatcher 调用点接入。
- **DB**：无 schema 改动（`action_executions` 表 0615000003 已建）。
- **测试**：4 个新单测覆盖 action target serde 往返 / dispatcher 命中 ActionExecutor / OSS skip / license-gate skip。
