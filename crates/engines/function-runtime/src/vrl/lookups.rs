// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! VRL 里查组织的扩展表（`lookup`）和 IP 的地理位置（`geoip_lookup`）。
//!
//! - 数据在二进制里（扩展表、DB-IP 库），启动时由 [`install`] 接进来一次；没接的进程里两个函数都
//!   返回 `null`。
//! - 组织在编译程序时绑进函数（[`super::VrlRuntime::compile_for`]），程序改不了：`%` 元数据可以被
//!   程序改写，所以组织不从那里读。只做检查的编译（[`super::VrlRuntime::compile`]）没有组织，
//!   `lookup` 返回 `null`。
//! - `lookup` 的表名必须写成字面量：编译时就知道程序读哪些表，执行前由
//!   [`super::VrlRuntime::prepare`] 先把它们读进来，执行时 VRL 同步地查内存。
//! - 两个函数都不会失败：查不到、键不是文本、IP 不合法，都返回 `null`。

use std::{
    net::IpAddr,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use parking_lot::Mutex;
use vrl::compiler::prelude::*;

use crate::shared::{Result, ids::Id};

/// VRL 函数读的数据。
#[async_trait]
pub trait VrlLookups: Send + Sync {
    /// 把组织的这几张扩展表读进来（或在过期时重读），之后 [`Self::row`] 才查得到。
    async fn prepare(&self, org: &Id, tables: &[String]) -> Result<()>;

    /// 组织的扩展表 `table` 里键为 `key` 的那一行；没有这张表、这一行，或还没读进来时为 `None`。
    fn row(&self, org: &Id, table: &str, key: &str) -> Option<serde_json::Value>;

    /// IP 地址所在的地方（`country`、`city`……）；库里没有时为 `None`。
    fn geoip(&self, ip: IpAddr) -> Option<serde_json::Value>;
}

static INSTALLED: OnceLock<Arc<dyn VrlLookups>> = OnceLock::new();

/// 接上本进程的扩展表和地理位置库。只认第一次。
pub fn install(lookups: Arc<dyn VrlLookups>) {
    let _ = INSTALLED.set(lookups);
}

pub(super) fn installed() -> Option<Arc<dyn VrlLookups>> {
    INSTALLED.get().cloned()
}

/// 编译一个程序时额外可用的函数。`tables` 收集程序里 `lookup` 写的表名。
pub(super) fn functions(
    org: Option<&Id>,
    lookups: Option<Arc<dyn VrlLookups>>,
    tables: Arc<Mutex<Vec<String>>>,
) -> Vec<Box<dyn Function>> {
    vec![
        Box::new(Lookup {
            org: org.cloned(),
            lookups: lookups.clone(),
            tables,
        }),
        Box::new(GeoipLookup { lookups }),
    ]
}

#[derive(Clone)]
struct Lookup {
    org: Option<Id>,
    lookups: Option<Arc<dyn VrlLookups>>,
    tables: Arc<Mutex<Vec<String>>>,
}

impl fmt::Debug for Lookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lookup").field("org", &self.org).finish()
    }
}

impl Function for Lookup {
    fn identifier(&self) -> &'static str {
        "lookup"
    }

    fn usage(&self) -> &'static str {
        "Returns the row of the organization's extend table `table` whose key is `key`, or null \
         when there is none. The table name must be written as a literal string."
    }

    fn category(&self) -> &'static str {
        Category::Object.as_ref()
    }

    fn return_kind(&self) -> u16 {
        kind::ANY
    }

    fn parameters(&self) -> &'static [Parameter] {
        const PARAMETERS: &[Parameter] = &[
            Parameter::required("table", kind::BYTES, "The name of the extend table."),
            Parameter::required("key", kind::ANY, "The key of the row."),
        ];
        PARAMETERS
    }

    fn examples(&self) -> &'static [Example] {
        &[]
    }

    fn compile(
        &self,
        state: &state::TypeState,
        _ctx: &mut FunctionCompileContext,
        arguments: ArgumentList,
    ) -> Compiled {
        let table = match arguments.required_literal("table", state)? {
            Value::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            other => other.to_string(),
        };

        self.tables.lock().push(table.clone());

        Ok(LookupFn {
            org: self.org.clone(),
            lookups: self.lookups.clone(),
            table,
            key: arguments.required("key"),
        }
        .as_expr())
    }
}

#[derive(Clone)]
struct LookupFn {
    org: Option<Id>,
    lookups: Option<Arc<dyn VrlLookups>>,
    table: String,
    key: Box<dyn Expression>,
}

impl fmt::Debug for LookupFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupFn")
            .field("table", &self.table)
            .finish()
    }
}

impl FunctionExpression for LookupFn {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        let key = self.key.resolve(ctx)?;

        let (Some(org), Some(lookups), Some(key)) = (&self.org, &self.lookups, key_text(&key))
        else {
            return Ok(Value::Null);
        };

        Ok(lookups
            .row(org, &self.table, &key)
            .map(Value::from)
            .unwrap_or(Value::Null))
    }

    fn type_def(&self, _: &state::TypeState) -> TypeDef {
        TypeDef::any()
    }
}

#[derive(Clone)]
struct GeoipLookup {
    lookups: Option<Arc<dyn VrlLookups>>,
}

impl fmt::Debug for GeoipLookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeoipLookup").finish()
    }
}

impl Function for GeoipLookup {
    fn identifier(&self) -> &'static str {
        "geoip_lookup"
    }

    fn usage(&self) -> &'static str {
        "Returns where an IP address is, as an object with `country`, `country_name`, `continent`, \
         `region`, `city`, `latitude` and `longitude`, or null when the database does not know it."
    }

    fn category(&self) -> &'static str {
        Category::Ip.as_ref()
    }

    fn return_kind(&self) -> u16 {
        kind::OBJECT | kind::NULL
    }

    fn parameters(&self) -> &'static [Parameter] {
        const PARAMETERS: &[Parameter] = &[Parameter::required(
            "ip",
            kind::ANY,
            "The IP address, IPv4 or IPv6.",
        )];
        PARAMETERS
    }

    fn examples(&self) -> &'static [Example] {
        &[]
    }

    fn compile(
        &self,
        _state: &state::TypeState,
        _ctx: &mut FunctionCompileContext,
        arguments: ArgumentList,
    ) -> Compiled {
        Ok(GeoipLookupFn {
            lookups: self.lookups.clone(),
            ip: arguments.required("ip"),
        }
        .as_expr())
    }
}

#[derive(Clone)]
struct GeoipLookupFn {
    lookups: Option<Arc<dyn VrlLookups>>,
    ip: Box<dyn Expression>,
}

impl fmt::Debug for GeoipLookupFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeoipLookupFn").finish()
    }
}

impl FunctionExpression for GeoipLookupFn {
    fn resolve(&self, ctx: &mut Context) -> Resolved {
        let ip = self.ip.resolve(ctx)?;

        let place = key_text(&ip)
            .and_then(|text| text.trim().parse::<IpAddr>().ok())
            .zip(self.lookups.as_ref())
            .and_then(|(ip, lookups)| lookups.geoip(ip));

        Ok(place.map(Value::from).unwrap_or(Value::Null))
    }

    fn type_def(&self, _: &state::TypeState) -> TypeDef {
        TypeDef::object(Collection::any()).or_null()
    }
}

/// A key as the text it is looked up by: text as it is, numbers and booleans as written.
fn key_text(value: &Value) -> Option<String> {
    match value {
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        Value::Integer(_) | Value::Float(_) | Value::Boolean(_) => Some(value.to_string()),
        _ => None,
    }
}
