// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `functions` 表 Pg 实装（spec functions-runtime）。
//!
//! 当前实装范围：CRUD + 编译前置校验（regex 检查 / 非空 / 简单语法）。
//! 真正的 VRL / JS 编译 runtime 接入留作 follow-up（需 pull `vrl` crate ~30 deps；
//! `JsRuntime` 走 `feature = "js"`）。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::function::{Function, FunctionLanguage, FunctionRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgFunctionRepository {
    pool: PgPool,
}

impl PgFunctionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, language, source, params_schema,
                    created_at_micros, updated_at_micros";

fn lang_str(l: FunctionLanguage) -> &'static str {
    match l {
        FunctionLanguage::Vrl => "vrl",
        FunctionLanguage::Js => "js",
        FunctionLanguage::Llm => "llm",
    }
}
fn lang_parse(s: &str) -> FunctionLanguage {
    match s {
        "js" => FunctionLanguage::Js,
        "llm" => FunctionLanguage::Llm,
        _ => FunctionLanguage::Vrl,
    }
}

fn row_to(r: sqlx::postgres::PgRow) -> Function {
    let params: Json<serde_json::Value> = r
        .try_get("params_schema")
        .unwrap_or(Json(serde_json::Value::Object(Default::default())));
    Function {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        language: lang_parse(&r.try_get::<String, _>("language").unwrap_or_default()),
        source: r.try_get::<String, _>("source").unwrap_or_default(),
        params_schema: params.0,
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl FunctionRepository for PgFunctionRepository {
    async fn create(&self, f: Function) -> Result<Function> {
        sqlx::query(
            "INSERT INTO functions
                (id, org_id, name, language, source, params_schema,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&f.id.0)
        .bind(&f.org_id.0)
        .bind(&f.name)
        .bind(lang_str(f.language))
        .bind(&f.source)
        .bind(Json(&f.params_schema))
        .bind(f.created_at.0)
        .bind(f.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(f)
    }

    async fn update(&self, f: Function) -> Result<Function> {
        sqlx::query(
            "UPDATE functions SET
                name = $3, language = $4, source = $5, params_schema = $6,
                updated_at_micros = $7
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&f.id.0)
        .bind(&f.org_id.0)
        .bind(&f.name)
        .bind(lang_str(f.language))
        .bind(&f.source)
        .bind(Json(&f.params_schema))
        .bind(f.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(f)
    }

    async fn get_by_id(&self, id: &Id) -> Result<Function> {
        let sql = format!("SELECT {COLS} FROM functions WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Function> {
        // 内置预设（org `__builtin__`）对任何 org 可见、可读。
        let sql = format!(
            "SELECT {COLS} FROM functions WHERE (org_id = $1 OR org_id = '__builtin__') AND id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Function>> {
        // org 自有函数 + 全局内置预设（org `__builtin__`）；自有在前、内置在后。
        let sql = format!(
            "SELECT {COLS} FROM functions WHERE org_id = $1 OR org_id = '__builtin__' ORDER BY (org_id = '__builtin__'), name"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM functions WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

/// 编译前置校验：spec 要求 POST 时同步编译。
///
/// `js_runtime_enabled`：从 `[functions].js_runtime_enabled` 透传；
/// - false（feature off 默认 / 运行期开关关）→ JS source 一律 400，错误码与历史保持一致。
/// - true 且 `feature = "js-runtime"` 编进 binary →
///   走 `JsFunctionExecutor::parse_only` 在 throwaway V8 isolate 上做 syntax check，
///   语法错抛 invalid。
///
/// VRL 路径：跑 [`crate::infra::runtime::VrlRuntime::compile`]，diagnostics 透传给客户端。
pub fn precheck_compile(
    language: FunctionLanguage,
    source: &str,
    js_runtime_enabled: bool,
) -> Result<()> {
    use crate::shared::Error;
    if source.is_empty() {
        return Err(Error::invalid("function source must not be empty"));
    }
    if source.len() > 64 * 1024 {
        return Err(Error::invalid("function source exceeds 64 KiB"));
    }
    match language {
        FunctionLanguage::Vrl => {
            // 真编译：VRL 解释器 stdlib `del` / `parse_json` / 字段赋值 全在。
            crate::infra::runtime::VrlRuntime::new()
                .compile(source)
                .map(|_| ())
        }
        FunctionLanguage::Js => {
            if !js_runtime_enabled {
                return Err(Error::invalid(
                    "javascript runtime not available (build the binary with --features js-runtime)",
                ));
            }
            #[cfg(feature = "js-runtime")]
            {
                crate::infra::runtime::js_executor::JsFunctionExecutor::parse_only(source)
            }
            #[cfg(not(feature = "js-runtime"))]
            {
                Err(Error::invalid(
                    "javascript runtime not enabled (build with --features js-runtime)",
                ))
            }
        }
        // LLM 节点的 source 是自然语言指令/prompt，无可编译语法；非空即接受（上方已校验）。
        // 运行期是否真正执行由 `[functions].llm_eval_enabled` + 已配 AI provider 决定。
        FunctionLanguage::Llm => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_source() {
        assert!(precheck_compile(FunctionLanguage::Vrl, "", false).is_err());
    }

    #[test]
    fn rejects_oversized_source() {
        let big = "a".repeat(65 * 1024);
        assert!(precheck_compile(FunctionLanguage::Vrl, &big, false).is_err());
    }

    #[test]
    fn rejects_js_when_disabled() {
        // `js_runtime_enabled = false` → 永远返错，与 feature 是否开无关。
        assert!(precheck_compile(FunctionLanguage::Js, ".level = 1", false).is_err());
    }

    #[test]
    fn accepts_basic_vrl() {
        assert!(
            precheck_compile(FunctionLanguage::Vrl, ".level = downcase!(.level)", false).is_ok()
        );
    }

    #[test]
    fn rejects_invalid_vrl_with_diagnostic() {
        let err =
            precheck_compile(FunctionLanguage::Vrl, ".level = unknown_fn(.x)", false).unwrap_err();
        // VrlRuntime::compile 返 InvalidArgument，diagnostics 透传给前端排查。
        assert!(err.to_string().contains("vrl"), "{err}");
    }

    #[cfg(feature = "js-runtime")]
    #[test]
    fn accepts_valid_js_when_enabled() {
        assert!(
            precheck_compile(FunctionLanguage::Js, r#"molesignal.set("k", 1);"#, true,).is_ok()
        );
    }

    #[cfg(feature = "js-runtime")]
    #[test]
    fn rejects_js_syntax_error_when_enabled() {
        let err = precheck_compile(FunctionLanguage::Js, "function( bad", true).unwrap_err();
        assert!(
            err.to_string().contains("syntax") || err.to_string().contains("js"),
            "{err}"
        );
    }
}
