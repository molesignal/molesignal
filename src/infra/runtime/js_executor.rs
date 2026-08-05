// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! JS function runtime（spec functions-runtime / change `js-runtime-functions`）。
//!
//! 仅在 `cargo build --features js-runtime` 下编译。设计选型见
//! `openspec/changes/js-runtime-functions/design.md`。
//!
//! 关键约束：`deno_core::JsRuntime` 内含 `RefCell<dyn Any>` → `!Send + !Sync`。
//! `FunctionExecutor` trait 又要求 `async fn run` 在 multi-thread tokio scheduler
//! 上可调度，所以 `JsRuntime` 不能直接挂在 `Arc<...>` 跨线程共享。
//!
//! 解决方案：每个 `(function_id, updated_at)` 起一个 OS 线程拥有 `JsRuntime`，
//! 外部通过 `mpsc::Sender<Job>` 投递评估请求 + `tokio::oneshot` 拿结果。
//! `v8::IsolateHandle` 自带 `Send + Sync`，主线程拿一份用于 `terminate_execution`
//! 实现 wall-clock 超时。

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use async_trait::async_trait;
use dashmap::DashMap;
use deno_core::{JsRuntime, RuntimeOptions, v8};
use tokio::sync::oneshot;

use crate::{
    app::ingestion::FunctionExecutor,
    domain::function::{Function, FunctionLanguage},
    shared::{Error, Result},
};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(50);
pub const DEFAULT_HEAP_MAX_BYTES: usize = 32 * 1024 * 1024;

const PRELUDE: &str = include_str!("js_executor_prelude.js");

pub struct JsFunctionExecutor {
    /// per-function worker thread；每个 worker 独占一个 `JsRuntime`。
    workers: DashMap<(String, i64), Arc<IsolateWorker>>,
    timeout: Duration,
    heap_max_bytes: usize,
}

impl JsFunctionExecutor {
    pub fn new(timeout: Duration, heap_max_bytes: usize) -> Self {
        // 不在这里 init V8：deno_core 的 JsRuntime::new 内部走 `init_v8`，
        // 二次 `V8::initialize_platform` 会触发 `Check failed: !IsFrozen()` abort。
        Self {
            workers: DashMap::new(),
            timeout,
            heap_max_bytes,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_TIMEOUT, DEFAULT_HEAP_MAX_BYTES)
    }

    fn get_or_init(&self, f: &Function) -> Result<Arc<IsolateWorker>> {
        let key = (f.id.0.clone(), f.updated_at.0);
        if let Some(slot) = self.workers.get(&key)
            && !slot.is_dirty()
        {
            return Ok(slot.clone());
        }
        let worker = Arc::new(IsolateWorker::spawn(f.source.clone(), self.heap_max_bytes)?);
        self.workers.insert(key, worker.clone());
        // 旧版本同 function_id 清理。
        self.workers
            .retain(|k, _| k.0 != f.id.0 || k.1 == f.updated_at.0);
        Ok(worker)
    }

    /// 同步路径：throwaway isolate 跑 parse-only 校验。
    /// 由 `precheck_compile` 在 HTTP CRUD 路径调用，**不要**在 tokio 任务里跑。
    pub fn parse_only(source: &str) -> Result<()> {
        let mut runtime = JsRuntime::new(RuntimeOptions::default());
        let wrapped = format!("(function(){{\n{source}\n}})");
        runtime
            .execute_script("[molesignal:parse-check]", wrapped)
            .map(|_| ())
            .map_err(|e| Error::invalid(format!("js syntax error: {e}")))
    }
}

#[async_trait]
impl FunctionExecutor for JsFunctionExecutor {
    async fn run(&self, function: &Function, event: &mut serde_json::Value) -> Result<()> {
        if !matches!(function.language, FunctionLanguage::Js) {
            return Err(Error::invalid(
                "JsFunctionExecutor only accepts language=Js; wrap with ChainedFunctionExecutor",
            ));
        }
        let worker = self.get_or_init(function)?;
        let timeout = self.timeout;

        let event_json =
            serde_json::to_string(event).map_err(|e| Error::internal(format!("encode: {e}")))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        worker
            .submit(Job::Run {
                event_json,
                reply: reply_tx,
            })
            .map_err(|_| Error::internal("js worker thread dead"))?;

        // wall-clock timer：超时把 isolate terminate；worker 那条 evaluate 会带 error 返。
        let handle = worker.handle();
        let terminator = tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            handle.terminate_execution();
        });

        let outcome = reply_rx.await.map_err(|_| {
            worker.mark_dirty();
            Error::internal("js worker dropped reply channel")
        })?;
        terminator.abort();

        match outcome {
            Ok(dump) => {
                let new_fields: serde_json::Value = serde_json::from_str(&dump)
                    .map_err(|e| Error::internal(format!("decode js result: {e}")))?;
                *event = new_fields;
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("terminated") {
                    worker.mark_dirty();
                    return Err(Error::invalid(format!(
                        "js timeout {}ms",
                        timeout.as_millis()
                    )));
                }
                if msg.contains("heap") || msg.contains("Maximum") {
                    worker.mark_dirty();
                    return Err(Error::invalid("js heap exhausted"));
                }
                Err(e)
            }
        }
    }
}

enum Job {
    Run {
        event_json: String,
        reply: oneshot::Sender<Result<String>>,
    },
    Shutdown,
}

/// 持有 worker 线程 + sender + handle；JsRuntime 永远在 worker 线程内。
struct IsolateWorker {
    tx: SyncSender<Job>,
    handle: v8::IsolateHandle,
    dirty: Mutex<bool>,
    _thread: Mutex<Option<JoinHandle<()>>>,
}

impl IsolateWorker {
    fn spawn(source: String, heap_max_bytes: usize) -> Result<Self> {
        // bound=1：单 producer 单 consumer，反压自然。
        let (tx, rx) = mpsc::sync_channel::<Job>(1);
        let (handle_tx, handle_rx) = mpsc::sync_channel::<Result<v8::IsolateHandle>>(1);

        let thread = thread::Builder::new()
            .name("molesignal-js-isolate".into())
            .spawn(move || {
                let mut runtime = match build_runtime(&source, heap_max_bytes) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = handle_tx.send(Err(e));
                        return;
                    }
                };
                let v8_handle = runtime.v8_isolate().thread_safe_handle();
                if handle_tx.send(Ok(v8_handle)).is_err() {
                    return;
                }
                drop(handle_tx);

                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Run { event_json, reply } => {
                            let r = run_once(&mut runtime, &event_json);
                            let _ = reply.send(r);
                        }
                        Job::Shutdown => break,
                    }
                }
            })
            .map_err(|e| Error::internal(format!("spawn js worker thread: {e}")))?;

        let handle = handle_rx
            .recv()
            .map_err(|_| Error::internal("js worker died during init"))??;
        Ok(Self {
            tx,
            handle,
            dirty: Mutex::new(false),
            _thread: Mutex::new(Some(thread)),
        })
    }

    fn submit(&self, job: Job) -> std::result::Result<(), mpsc::SendError<Job>> {
        self.tx.send(job)
    }

    fn handle(&self) -> v8::IsolateHandle {
        self.handle.clone()
    }

    fn is_dirty(&self) -> bool {
        self.dirty.lock().map(|g| *g).unwrap_or(true)
    }

    fn mark_dirty(&self) {
        if let Ok(mut g) = self.dirty.lock() {
            *g = true;
        }
    }
}

impl Drop for IsolateWorker {
    fn drop(&mut self) {
        // 优雅退出：先 terminate 醒来阻塞的 evaluate；再投 Shutdown。
        self.handle.terminate_execution();
        let _ = self.tx.send(Job::Shutdown);
        if let Ok(mut slot) = self._thread.lock()
            && let Some(t) = slot.take()
        {
            let _ = t.join();
        }
    }
}

fn build_runtime(source: &str, heap_max_bytes: usize) -> Result<JsRuntime> {
    let create_params = v8::CreateParams::default().heap_limits(0, heap_max_bytes);
    let mut runtime = JsRuntime::new(RuntimeOptions {
        create_params: Some(create_params),
        ..Default::default()
    });
    // 接近 heap 上限 → terminate 当前 evaluate；返回 current 表示「不再扩张」。
    let handle = runtime.v8_isolate().thread_safe_handle();
    runtime.add_near_heap_limit_callback(move |current, _initial| {
        handle.terminate_execution();
        current
    });
    runtime
        .execute_script("[molesignal:prelude]", PRELUDE)
        .map_err(|e| Error::invalid(format!("js prelude: {e}")))?;
    // user source 包成函数，编译只发生一次。
    let wrapped = format!("globalThis.__obsv_user = function() {{\n{source}\n}};");
    runtime
        .execute_script("[molesignal:user-compile]", wrapped)
        .map_err(|e| Error::invalid(format!("js compile: {e}")))?;
    Ok(runtime)
}

fn run_once(runtime: &mut JsRuntime, event_json: &str) -> Result<String> {
    // 双层 JSON：内层是 fields object 序列化；外层包成 JS string literal。
    let arg_literal = serde_json::to_string(event_json)
        .map_err(|e| Error::internal(format!("encode event literal: {e}")))?;
    let src = format!(
        "globalThis.molesignal.__reset({arg_literal}); globalThis.__obsv_user(); globalThis.molesignal.__dump()"
    );
    let global = runtime
        .execute_script("[molesignal:run]", src)
        .map_err(|e| Error::invalid(format!("js: {e}")))?;
    // deno_core 0.403 dropped JsRuntime::handle_scope(); use the v8 pinned-scope
    // macros against the runtime's isolate + main context instead.
    let context = runtime.main_context();
    let isolate = runtime.v8_isolate();
    v8::scope_with_context!(let scope, isolate, &context);
    let local = v8::Local::new(scope, global);
    let s = local
        .to_string(scope)
        .ok_or_else(|| Error::internal("js: __dump did not return string"))?;
    Ok(s.to_rust_string_lossy(scope))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn mk_fn(source: &str, updated_micros: i64) -> Function {
        Function {
            id: Id(format!("fn-{updated_micros}")),
            org_id: Id("orgA".into()),
            name: "js_fn".into(),
            language: FunctionLanguage::Js,
            source: source.into(),
            params_schema: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(updated_micros),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn js_set_field_writes_back_to_event() {
        let ex = JsFunctionExecutor::with_defaults();
        let f = mk_fn(
            r#"molesignal.set("level", molesignal.fields.severity.toLowerCase());"#,
            1,
        );
        let mut ev = json!({ "severity": "INFO" });
        ex.run(&f, &mut ev).await.unwrap();
        assert_eq!(ev["level"], "info");
        assert_eq!(ev["severity"], "INFO");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn js_del_field_removes_key() {
        let ex = JsFunctionExecutor::with_defaults();
        let f = mk_fn(r#"molesignal.del("pw");"#, 2);
        let mut ev = json!({ "user": "alice", "pw": "secret" });
        ex.run(&f, &mut ev).await.unwrap();
        assert!(ev.get("pw").is_none());
        assert_eq!(ev["user"], "alice");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn js_infinite_loop_times_out() {
        let ex = JsFunctionExecutor::new(Duration::from_millis(40), DEFAULT_HEAP_MAX_BYTES);
        let f = mk_fn(r#"while (true) {}"#, 3);
        let mut ev = json!({});
        let err = ex.run(&f, &mut ev).await.unwrap_err();
        assert!(err.to_string().contains("timeout"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn js_syntax_error_caught_at_compile() {
        let ex = JsFunctionExecutor::with_defaults();
        let f = mk_fn(r#"function( bad"#, 4);
        let mut ev = json!({});
        let err = ex.run(&f, &mut ev).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("compile") || msg.contains("syntax") || msg.contains("Unexpected"),
            "{msg}"
        );
    }
}
