## 1. 依赖 + feature

- [x] 1.1 `Cargo.toml` 工作区加 `deno_core = "0.330"` (optional)
- [x] 1.2 `crates/infra/Cargo.toml` 加 `deno_core = { workspace = true, optional = true }`；新 feature `js-runtime = ["dep:deno_core"]`
- [x] 1.3 `crates/bootstrap/Cargo.toml` 加同 feature 转发到 infra：`js-runtime = ["molesignal-infra/js-runtime"]`
- [x] 1.4 `crates/config/src/settings.rs::FunctionsSettings { js_runtime_enabled: bool }`

## 2. JsFunctionExecutor

- [x] 2.1 `crates/infra/src/runtime/js_executor.rs`（cfg `js-runtime`）：`JsFunctionExecutor { isolates: DashMap<(Id, i64), JsIsolate>, timeout: Duration, heap_max_bytes: usize }`
- [x] 2.2 `JsIsolate` 包装 `deno_core::JsRuntime`，初始化时定义 `molesignal` global + 注入 op (`set_field` / `del_field` / `now` / `log` / `sha256` / `parse_json` / `encode_json`)
- [x] 2.3 `impl FunctionExecutor`：language=Js 时取 / 起 isolate → 把 event JSON 写入 `molesignal.fields` → 评估 source → 拉回 fields 写回 event
- [x] 2.4 超时：`tokio::select!` 包 evaluate + `tokio::time::sleep(timeout)` → 超时调 `isolate.handle.terminate_execution()` + 标记 isolate dirty 下次重建
- [x] 2.5 heap：`Isolate::set_oom_error_handler` 抛 `IngestError`
- [x] 2.6 unit test 4 个：set 字段 / del 字段 / 超时 / syntax error

## 3. ChainedExecutor

- [x] 3.1 `crates/infra/src/runtime/chained_executor.rs`：`ChainedFunctionExecutor { vrl: Arc<VrlExecutor>, js: Option<Arc<JsExecutor>> }`
- [x] 3.2 `impl FunctionExecutor`：language=Vrl → vrl；language=Js + js.is_some() → js；language=Js + js.is_none() → `Err(IngestError { reason: "javascript runtime disabled" })`

## 4. precheck_compile JS path

- [x] 4.1 `crates/infra/src/persistence/repositories/functions.rs::precheck_compile`：language=Js + feature on + `js_runtime_enabled=true` → 试在 throwaway isolate 上 evaluate parse-only；语法错 → return `Err`
- [x] 4.2 unit test：好语法过 / 坏语法返 invalid

## 5. wire

- [x] 5.1 `crates/bootstrap/src/wire.rs`：feature `js-runtime` + `settings.functions.js_runtime_enabled` 构造 `JsFunctionExecutor`，否则 None
- [x] 5.2 把 vrl + js 包成 `ChainedFunctionExecutor` 注入 PipelineEngine

## 6. 文档 + 编译矩阵

- [x] 6.1 README 加 "JS Function Runtime" 段：何时启用、如何启用、有何限制
- [x] 6.2 `cargo check --workspace` clean（feature off 默认）
- [x] 6.3 `cargo check --workspace --features molesignal-infra/js-runtime` clean
- [x] 6.4 `cargo test --workspace --lib --features molesignal-infra/js-runtime` 全绿
