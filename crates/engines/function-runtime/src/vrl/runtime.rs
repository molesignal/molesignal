// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! VRL runtime。
//!
//! 设计：
//! - 编译期 `compile(source)` 返回 `CompiledProgram`（缓存在 PipelineEngine 里，按
//!   `function_id+source_hash` 做 key）；
//! - 执行期 `run(&program, event)` 拿到 `serde_json::Value`、转 `vrl::value::Value`
//!   做 target，跑解释器，再转回 `serde_json::Value`；
//! - VRL `del()`/`.field = ...` 直接突变 target；返回值是程序末尾表达式的值（这里忽略）。
//!
//! 时间预算：VRL stdlib 是纯函数 + 解释器，每条 event 评估 < 100µs；不引入 `runtime::Runtime`
//! 共享状态以避免锁竞争，每次 `run` 新建一个 `Runtime` 实例（成本可忽略）。

use std::sync::Arc;

use parking_lot::Mutex;
use vrl::{
    compiler::{
        ExpressionError, Program, TargetValue,
        runtime::{Runtime, Terminate},
    },
    value::{Secrets, Value as VrlValue},
};

use crate::{
    domain::function::FunctionOutcome,
    shared::{Error, Result},
};

/// 编译好的 VRL 程序（克隆便宜：内部 `Program` 是 Arc 化的 AST 表示）。
#[derive(Clone)]
pub struct CompiledProgram {
    program: Arc<Program>,
}

pub struct VrlRuntime {
    /// VRL stdlib 函数集合（all 包含 `parse_json`/`del`/`match`/`to_int` 等）。
    fns: Vec<Box<dyn vrl::compiler::Function>>,
    /// `Runtime` 复用：`vrl::compiler::runtime::Runtime` 是 cheap；我们仍按 thread-local 兜底。
    _phantom: std::marker::PhantomData<()>,
    /// 多线程共享缓存的占位 mutex（暂不缓存，简化）
    _lock: Mutex<()>,
}

impl Default for VrlRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl VrlRuntime {
    pub fn new() -> Self {
        Self {
            fns: vrl::stdlib::all(),
            _phantom: std::marker::PhantomData,
            _lock: Mutex::new(()),
        }
    }

    /// 编译 source；失败 → Error::invalid（HTTP CRUD 校验路径可直接 400）。
    pub fn compile(&self, source: &str) -> Result<CompiledProgram> {
        let result = vrl::compiler::compile(source, &self.fns)
            .map_err(|diags| Error::invalid(format!("vrl compile: {diags:?}")))?;

        Ok(CompiledProgram {
            program: Arc::new(result.program),
        })
    }

    /// 在 `event` 上执行 program；event 被原地突变（VRL `.field = ...` 语义）。程序末尾表达式的值
    /// 忽略，pipeline 关心的是突变后的 event。程序里写的 `abort` 是有意丢弃这条 event，返回
    /// [`FunctionOutcome::Dropped`]，不算失败；运行出错（含未处理的可失败表达式）才返回 `Err`。
    /// 无论结果如何，`event` 都是程序停下时的样子。
    pub fn run(
        &self,
        program: &CompiledProgram,
        event: &mut serde_json::Value,
    ) -> Result<FunctionOutcome> {
        self.run_with_metadata(program, event, serde_json::Map::new())
    }

    /// [`Self::run`]，另带程序以 `%name` 读取的元数据。元数据不属于 event，也不会写进 event。
    pub fn run_with_metadata(
        &self,
        program: &CompiledProgram,
        event: &mut serde_json::Value,
        metadata: serde_json::Map<String, serde_json::Value>,
    ) -> Result<FunctionOutcome> {
        let mut target = TargetValue {
            value: VrlValue::from(std::mem::take(event)),
            metadata: VrlValue::from(serde_json::Value::Object(metadata)),
            secrets: Secrets::default(),
        };

        let timezone = vrl::compiler::TimeZone::default();
        let mut rt = Runtime::default();
        let resolved = rt.resolve(&mut target, &program.program, &timezone);
        *event = vrl_to_json(target.value);

        match resolved {
            Ok(_) => Ok(FunctionOutcome::Kept),
            Err(Terminate::Abort(ExpressionError::Abort { .. })) => Ok(FunctionOutcome::Dropped),
            Err(e) => Err(Error::internal(format!("vrl run: {e}"))),
        }
    }
}

/// VRL `Value` → `serde_json::Value`。`Timestamp` 转 ISO8601；`Bytes` 转 UTF-8 字符串（无效 UTF-8 退化 base64）。
fn vrl_to_json(v: VrlValue) -> serde_json::Value {
    use serde_json::Value as J;
    use vrl::value::Value as V;

    match v {
        V::Bytes(b) => match std::str::from_utf8(&b) {
            Ok(s) => J::String(s.to_string()),
            Err(_) => J::String(base64_encode(&b)),
        },
        V::Integer(i) => J::Number(i.into()),
        V::Float(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .unwrap_or(J::Null),
        V::Boolean(b) => J::Bool(b),
        V::Timestamp(ts) => J::String(ts.to_rfc3339()),
        V::Regex(r) => J::String(r.to_string()),
        V::Null => J::Null,
        V::Array(arr) => J::Array(arr.into_iter().map(vrl_to_json).collect()),
        V::Object(obj) => {
            let mut map = serde_json::Map::with_capacity(obj.len());

            for (k, val) in obj {
                map.insert(k.to_string(), vrl_to_json(val));
            }

            J::Object(map)
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::engine::{Engine, general_purpose::STANDARD};
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn compile_invalid_returns_error() {
        let rt = VrlRuntime::new();
        assert!(rt.compile("this is not vrl ((").is_err());
    }

    #[test]
    fn assigns_new_field() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#".level = "info""#).unwrap();
        let mut ev = json!({ "msg": "hi" });
        rt.run(&prog, &mut ev).unwrap();
        assert_eq!(ev["level"], "info");
        assert_eq!(ev["msg"], "hi");
    }

    #[test]
    fn deletes_field_with_del() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#"del(.password)"#).unwrap();
        let mut ev = json!({ "user": "alice", "password": "secret" });
        rt.run(&prog, &mut ev).unwrap();
        assert!(ev.get("password").is_none());
        assert_eq!(ev["user"], "alice");
    }

    #[test]
    fn supports_string_uppercase() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#".env = upcase(string!(.env))"#).unwrap();
        let mut ev = json!({ "env": "prod" });
        rt.run(&prog, &mut ev).unwrap();
        assert_eq!(ev["env"], "PROD");
    }

    #[test]
    fn a_written_abort_drops_the_event_and_is_not_an_error() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#"if .level == "debug" { abort }"#).unwrap();

        let mut debug = json!({ "level": "debug" });
        assert_eq!(rt.run(&prog, &mut debug).unwrap(), FunctionOutcome::Dropped);

        let mut info = json!({ "level": "info" });
        assert_eq!(rt.run(&prog, &mut info).unwrap(), FunctionOutcome::Kept);
        assert_eq!(info, json!({ "level": "info" }));
    }

    #[test]
    fn a_failing_program_is_an_error_not_a_drop() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#".n = to_int!(.text)"#).unwrap();
        let mut ev = json!({ "text": "not a number" });

        let err = rt.run(&prog, &mut ev).unwrap_err();

        assert!(err.to_string().contains("vrl run"), "{err}");
        // The event is handed back as the program left it, not emptied.
        assert_eq!(ev["text"], "not a number");
    }

    /// The programs the pipeline editor's filter and sample steps generate
    /// (`web/src/routes/pipelines/processors/catalog.ts`), as they come out of it.
    #[test]
    fn the_editors_filter_and_sample_programs_drop_what_they_say() {
        let rt = VrlRuntime::new();
        let outcome = |source: &str, event: serde_json::Value| {
            let prog = rt
                .compile(source)
                .unwrap_or_else(|e| panic!("{source}\n{e}"));
            let mut event = event;
            rt.run(&prog, &mut event).unwrap()
        };
        let (kept, dropped) = (FunctionOutcome::Kept, FunctionOutcome::Dropped);

        let keep_errors = "value = .level\nmatched = value == \"error\"\nif !matched {\n  abort\n}";
        assert_eq!(outcome(keep_errors, json!({ "level": "error" })), kept);
        assert_eq!(outcome(keep_errors, json!({ "level": "info" })), dropped);
        assert_eq!(outcome(keep_errors, json!({})), dropped);

        let drop_5xx = "value = .status\nmatched = false\nif value != null {\n  number, err = to_float(value)\n  matched = err == null && number >= 500.0\n}\nif matched {\n  abort\n}";
        assert_eq!(outcome(drop_5xx, json!({ "status": 503 })), dropped);
        assert_eq!(outcome(drop_5xx, json!({ "status": "502" })), dropped);
        assert_eq!(outcome(drop_5xx, json!({ "status": 200 })), kept);
        assert_eq!(outcome(drop_5xx, json!({ "status": "n/a" })), kept);
        assert_eq!(outcome(drop_5xx, json!({})), kept);

        let drop_tests = "value = null\nif exists(.\"service.name\") {\n  value = .\"service.name\"\n} else if exists(.service.name) {\n  value = .service.name\n}\nmatched = is_string(value) && match(string!(value), r'^test-')\nif matched {\n  abort\n}";
        assert_eq!(
            outcome(drop_tests, json!({ "service.name": "test-api" })),
            dropped
        );
        assert_eq!(
            outcome(drop_tests, json!({ "service": { "name": "test-api" } })),
            dropped
        );
        assert_eq!(
            outcome(drop_tests, json!({ "service": { "name": "api" } })),
            kept
        );
        assert_eq!(
            outcome(drop_tests, json!({ "service": { "name": 7 } })),
            kept
        );

        let keep_users =
            "matched = exists(.\"user.id\") || exists(.user.id)\nif !matched {\n  abort\n}";
        assert_eq!(outcome(keep_users, json!({ "user": { "id": 1 } })), kept);
        assert_eq!(outcome(keep_users, json!({ "user": {} })), dropped);

        let drop_not_200 = "value = .code\nmatched = !(value == 200 || value == \"200\")\nif matched {\n  abort\n}";
        assert_eq!(outcome(drop_not_200, json!({ "code": 200 })), kept);
        assert_eq!(outcome(drop_not_200, json!({ "code": "200" })), kept);
        assert_eq!(outcome(drop_not_200, json!({ "code": 404 })), dropped);

        let keep_timeouts = "value = .message\nmatched = is_string(value) && contains(string!(value), \"timeout\")\nif !matched {\n  abort\n}";
        assert_eq!(
            outcome(keep_timeouts, json!({ "message": "read timeout" })),
            kept
        );
        assert_eq!(outcome(keep_timeouts, json!({ "message": 3 })), dropped);

        let sample_all = "if !(random_float(0.0, 100.0) < 100.0) {\n  abort\n}";
        assert_eq!(outcome(sample_all, json!({})), kept);

        // By a field, one value is always kept or always dropped.
        let by_trace = "value = .trace_id\nkey, err = to_string(value)\nsampled = false\nif value == null || err != null {\n  sampled = random_float(0.0, 100.0) < 25.0\n} else {\n  sampled = mod(abs(seahash(key)), 10000) < 2500\n}\nif !sampled {\n  abort\n}";
        let mut outcomes = Vec::new();

        for trace in 0..200 {
            let event = json!({ "trace_id": format!("trace-{trace}") });
            let first = outcome(by_trace, event.clone());
            assert_eq!(outcome(by_trace, event), first);
            outcomes.push(first);
        }

        let share = outcomes.iter().filter(|o| **o == kept).count();
        assert!((20..=90).contains(&share), "{share} of 200 kept");
    }

    #[test]
    fn metadata_is_readable_and_stays_out_of_the_event() {
        let rt = VrlRuntime::new();
        let prog = rt.compile(r#".from = %source_stream"#).unwrap();
        let mut ev = json!({ "msg": "a" });
        let mut metadata = serde_json::Map::new();
        metadata.insert("source_stream".into(), json!("nginx"));

        rt.run_with_metadata(&prog, &mut ev, metadata).unwrap();

        assert_eq!(ev, json!({ "msg": "a", "from": "nginx" }));
    }
}
