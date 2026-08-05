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
    compiler::{Program, TargetValue, runtime::Runtime},
    value::{Secrets, Value as VrlValue},
};

use crate::shared::{Error, Result};

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

    /// 在 `event` 上执行 program；event 被原地突变（VRL `.field = ...` 语义）。
    /// 返回 `Ok(())` 即可（程序末尾表达式值忽略，pipeline 关心的是 mutated event）。
    pub fn run(&self, program: &CompiledProgram, event: &mut serde_json::Value) -> Result<()> {
        let target_value = std::mem::take(event);
        let mut target = TargetValue {
            value: VrlValue::from(target_value),
            metadata: VrlValue::Object(Default::default()),
            secrets: Secrets::default(),
        };
        let timezone = vrl::compiler::TimeZone::default();
        let mut rt = Runtime::default();
        let _ret = rt
            .resolve(&mut target, &program.program, &timezone)
            .map_err(|e| Error::internal(format!("vrl run: {e}")))?;
        // 转回 serde_json::Value
        *event = vrl_to_json(target.value);
        Ok(())
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
}
