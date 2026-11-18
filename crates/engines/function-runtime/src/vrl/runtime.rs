// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! VRL runtime.
//!
//! Design:
//! - At compile time, `compile(source)` returns a `CompiledProgram` (cached in
//!   `VrlFunctionExecutor`, keyed by `function_id+updated_at`);
//! - At run time, `run(&program, event)` takes the `serde_json::Value`, converts it to a
//!   `vrl::value::Value` as the target, runs the interpreter, then converts it back to
//!   `serde_json::Value`;
//! - VRL `del()`/`.field = ...` mutates the target directly; the return value is the value
//!   of the program's final expression (ignored here).
//!
//! Time budget: the VRL stdlib is pure functions plus an interpreter, and evaluating
//! each event takes < 100µs. No shared `runtime::Runtime` state is introduced, to avoid
//! lock contention, and each `run` creates a new `Runtime` instance (the cost is negligible).

use std::sync::Arc;

use domain::function::FunctionOutcome;
use kernel::{Error, Result, ids::Id};
use parking_lot::Mutex;
use vrl::{
    compiler::{
        ExpressionError, Program, TargetValue,
        runtime::{Runtime, Terminate},
    },
    value::{Secrets, Value as VrlValue},
};

use super::lookups::{self, VrlLookups};

/// A compiled VRL program (cheap to clone: the inner `Program` is an Arc-wrapped AST
/// representation).
#[derive(Clone)]
pub struct CompiledProgram {
    program: Arc<Program>,
    /// The extend tables that the program reads with `lookup`; [`VrlRuntime::prepare`] loads
    /// them before execution.
    tables: Arc<[String]>,
}

impl CompiledProgram {
    /// Whether the program can drop the event: it can if it contains an `abort`.
    pub fn can_abort(&self) -> bool {
        self.program.info().abortable
    }
}

pub struct VrlRuntime {
    /// The data read by `lookup` and `geoip_lookup`; when none is given, the one connected
    /// at startup by [`lookups::install`] is used (fetched only when needed, so the runtime
    /// can be built before it is installed).
    lookups: Option<Arc<dyn VrlLookups>>,
    /// `Runtime` reuse: `vrl::compiler::runtime::Runtime` is cheap; we still use
    /// thread-local as a fallback.
    _phantom: std::marker::PhantomData<()>,
    /// Placeholder mutex for a cache shared across threads (not cached for now, to simplify).
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
            lookups: None,
            _phantom: std::marker::PhantomData,
            _lock: Mutex::new(()),
        }
    }

    /// Uses this data instead of the one connected at startup (for tests).
    pub fn with_lookups(mut self, lookups: Arc<dyn VrlLookups>) -> Self {
        self.lookups = Some(lookups);
        self
    }

    /// Compiles source only to check it; failure -> Error::invalid (the HTTP CRUD validation
    /// path can respond 400 directly). There is no organization, so `lookup` returns `null`
    /// in programs compiled this way; use [`Self::compile_for`] for programs to be executed.
    pub fn compile(&self, source: &str) -> Result<CompiledProgram> {
        self.compile_with(None, source)
    }

    /// Compiles source to be executed for organization `org`: `lookup` queries that
    /// organization's extend tables.
    pub fn compile_for(&self, org: &Id, source: &str) -> Result<CompiledProgram> {
        self.compile_with(Some(org), source)
    }

    fn compile_with(&self, org: Option<&Id>, source: &str) -> Result<CompiledProgram> {
        let tables = Arc::new(Mutex::new(Vec::new()));

        // The stdlib (`parse_json`/`del`/`match`/`to_int`, etc.), plus table lookup and
        // geolocation lookup.
        let mut fns = vrl::stdlib::all();
        fns.extend(lookups::functions(org, self.lookups(), tables.clone()));

        let result = vrl::compiler::compile(source, &fns)
            .map_err(|diags| Error::invalid(format!("vrl compile: {diags:?}")))?;

        let mut tables = std::mem::take(&mut *tables.lock());
        tables.sort();
        tables.dedup();

        Ok(CompiledProgram {
            program: Arc::new(result.program),
            tables: tables.into(),
        })
    }

    /// Loads the extend tables that the program reads with `lookup`; call it before execution.
    /// Does nothing for a program that reads no tables.
    pub async fn prepare(&self, org: &Id, program: &CompiledProgram) -> Result<()> {
        match self.lookups() {
            Some(lookups) if !program.tables.is_empty() => {
                lookups.prepare(org, &program.tables).await
            }
            _ => Ok(()),
        }
    }

    fn lookups(&self) -> Option<Arc<dyn VrlLookups>> {
        self.lookups.clone().or_else(lookups::installed)
    }

    /// Runs the program on `event`; the event is mutated in place (VRL `.field = ...`
    /// semantics). The value of the program's final expression is ignored; the pipeline cares
    /// about the mutated event. An `abort` in the program deliberately drops this event and
    /// returns [`FunctionOutcome::Dropped`], which is not a failure; only a runtime error
    /// (including an unhandled fallible expression) returns `Err`. Whatever the outcome,
    /// `event` is left as it was when the program stopped.
    pub fn run(
        &self,
        program: &CompiledProgram,
        event: &mut serde_json::Value,
    ) -> Result<FunctionOutcome> {
        self.run_with_metadata(program, event, serde_json::Map::new())
    }

    /// [`Self::run`], plus metadata that the program reads as `%name`. The metadata is not
    /// part of the event and is never written into it.
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

/// VRL `Value` -> `serde_json::Value`. `Timestamp` becomes ISO8601; `Bytes` becomes a UTF-8
/// string (invalid UTF-8 falls back to base64).
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

    /// The programs the editor's "rename with a regex" and "keep only" steps generate
    /// (`web/src/routes/pipelines/processors/catalog.ts`), as they come out of it.
    #[test]
    fn the_editors_bulk_rename_and_keep_programs_do_what_they_say() {
        let rt = VrlRuntime::new();
        let run = |source: &str, event: serde_json::Value| {
            let prog = rt
                .compile(source)
                .unwrap_or_else(|e| panic!("{source}\n{e}"));
            let mut event = event;
            assert_eq!(rt.run(&prog, &mut event).unwrap(), FunctionOutcome::Kept);
            event
        };

        let rename_labels = "if is_object(.labels) {\n  .labels = map_keys(object!(.labels)) -> |key| { replace(key, r'^kubernetes_(.+)$', \"$1\") }\n}";
        assert_eq!(
            run(
                rename_labels,
                json!({ "labels": { "kubernetes_pod": "web-1", "env": "prod" }, "summary": "s" })
            ),
            json!({ "labels": { "pod": "web-1", "env": "prod" }, "summary": "s" })
        );
        // A field that is missing or holds something else is left as it was.
        assert_eq!(run(rename_labels, json!({})), json!({}));
        assert_eq!(
            run(rename_labels, json!({ "labels": "x" })),
            json!({ "labels": "x" })
        );

        let rename_event = ". = map_keys(.) -> |key| { replace(key, r'^attr_', \"\") }";
        assert_eq!(
            run(rename_event, json!({ "attr_a": 1, "b": 2 })),
            json!({ "a": 1, "b": 2 })
        );

        let rename_dotted = "if is_object(.\"a.b\") {\n  .\"a.b\" = map_keys(object!(.\"a.b\")) -> |key| { replace(key, r'x', \"y\") }\n} else if is_object(.a.b) {\n  .a.b = map_keys(object!(.a.b)) -> |key| { replace(key, r'x', \"y\") }\n}";
        assert_eq!(
            run(rename_dotted, json!({ "a": { "b": { "x": 1 } } })),
            json!({ "a": { "b": { "y": 1 } } })
        );

        let keep_labels = "if is_object(.labels) {\n  .labels = filter(object!(.labels)) -> |key, _value| { includes([\"service\", \"env\"], key) }\n}";
        assert_eq!(
            run(
                keep_labels,
                json!({ "labels": { "service": "a", "env": "b", "pod": "c" }, "summary": "s" })
            ),
            json!({ "labels": { "service": "a", "env": "b" }, "summary": "s" })
        );
        assert_eq!(run(keep_labels, json!({})), json!({}));

        let keep_event =
            ". = filter(.) -> |key, _value| { includes([\"message\", \"_timestamp\"], key) }";
        assert_eq!(
            run(
                keep_event,
                json!({ "message": "m", "_timestamp": 1, "pod": "c" })
            ),
            json!({ "message": "m", "_timestamp": 1 })
        );
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
