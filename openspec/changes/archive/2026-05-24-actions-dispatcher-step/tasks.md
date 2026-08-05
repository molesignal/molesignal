## 1. Domain enum

- [x] 1.1 `crates/domain/src/alerting/escalation.rs::EscalationTarget` 加 `Action { action_id: Id }` variant；保留 serde tag = "kind"
- [x] 1.2 加 unit test 验证 `(serde_json::to_string + from_str)` 对 4 个 variant 都往返一致

## 2. App 层 port

- [x] 2.1 `crates/app/src/alerting/mod.rs` 新增 `pub mod executor;`
- [x] 2.2 `crates/app/src/alerting/executor.rs`：`ActionExecutorPort` trait（`async fn execute(action_id, IncidentContext) -> Result<ExecutionResult>`）+ `ExecutionResult` 结构（status / status_code / response_body / error / duration_ms） + `NoopActionExecutor` 实装
- [x] 2.3 `crates/app/src/alerting/dispatcher.rs::EscalationDispatcher`：构造增 `Arc<dyn ActionExecutorPort>`；`tick` 内遇 `Action` target → 调 `execute()` → 写一条 `action_executions`（通过新加的 `Arc<dyn ActionExecutionSink>` port 异步写入，避免 app 依赖 infra）

## 3. Infra adapter

- [x] 3.1 `crates/infra/src/alerting/action_executor_adapter.rs`：cfg=enterprise 实装 `ActionExecutorPort` —— 包装 `molesignal_enterprise_actions::ActionExecutor` + `ActionRepository::get(action_id)` 拿 ActionKind + `record_execution` 写表
- [x] 3.2 `crates/infra/src/alerting/mod.rs` 暴露上述 adapter
- [x] 3.3 `ActionExecutionSink` 实装：直接复用 `ActionRepository::record_execution`，做一个薄 trait → impl 转发

## 4. Wire

- [x] 4.1 `crates/bootstrap/src/wire.rs`：cfg=not(enterprise) 时注入 `NoopActionExecutor`；cfg=enterprise 时构造 `EnterpriseActionExecutorAdapter::new(actions_repo, webhook_client)` 并注入
- [x] 4.2 把 sink port 也注入 dispatcher 构造（OSS / enterprise 都用 PgActionRepository 实装的 sink）

## 5. 测试

- [x] 5.1 unit：`escalation_target_serde_roundtrip` —— 4 variant `to_string` + `from_str` 等价
- [x] 5.2 unit：`dispatcher_skips_action_target_in_oss` —— OSS noop executor 返 Skipped，dispatcher warn 不阻塞 sibling targets
- [x] 5.3 unit：`dispatcher_records_action_execution_on_success` —— mock executor 返 Success → sink 收到一条 record_execution 调用
- [x] 5.4 unit：`dispatcher_records_action_execution_on_failure` —— mock executor 返 Failed → sink 收到 status=failed + error 字符串

## 6. 编译矩阵

- [x] 6.1 `cargo check --workspace` clean
- [x] 6.2 `cargo check -p molesignal-bootstrap --features enterprise` clean
- [x] 6.3 `cargo test --workspace --lib` 全绿（含上述 4 个新单测）
