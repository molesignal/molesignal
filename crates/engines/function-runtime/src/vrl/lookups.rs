// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Looks up an organization's extend tables (`lookup`) and an IP's location (`geoip_lookup`)
//! from VRL.
//!
//! - The data lives in the binary (extend tables, the DB-IP database) and is connected
//!   once at startup by [`install`]; in a process where it is not connected, both functions
//!   return `null`.
//! - The organization is bound into the functions when the program is compiled
//!   ([`super::VrlRuntime::compile_for`]) and the program cannot change it: `%` metadata
//!   can be rewritten by the program, so the organization is not read from there. A compile
//!   used only for checking ([`super::VrlRuntime::compile`]) has no organization, so
//!   `lookup` returns `null`.
//! - The table name passed to `lookup` must be a literal: the tables a program reads are
//!   known at compile time, [`super::VrlRuntime::prepare`] reads them in before execution,
//!   and VRL then looks them up in memory synchronously at execution time.
//! - Neither function can fail: nothing found, a non-text key, or an invalid IP all return
//!   `null`.

use std::{
    net::IpAddr,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use kernel::{Result, ids::Id};
use parking_lot::Mutex;
use vrl::compiler::prelude::*;

/// The data that the VRL functions read.
#[async_trait]
pub trait VrlLookups: Send + Sync {
    /// Loads the given extend tables of the organization (or reloads them when stale); only
    /// afterward can [`Self::row`] find them.
    async fn prepare(&self, org: &Id, tables: &[String]) -> Result<()>;

    /// The row with key `key` in the organization's extend table `table`; `None` if there is
    /// no such table, no such row, or it has not been loaded yet.
    fn row(&self, org: &Id, table: &str, key: &str) -> Option<serde_json::Value>;

    /// The location of an IP address (`country`, `city`, ...);
    /// `None` if the database has no entry.
    fn geoip(&self, ip: IpAddr) -> Option<serde_json::Value>;
}

static INSTALLED: OnceLock<Arc<dyn VrlLookups>> = OnceLock::new();

/// Connects this process's extend tables and geolocation database.
/// Only the first call takes effect.
pub fn install(lookups: Arc<dyn VrlLookups>) {
    let _ = INSTALLED.set(lookups);
}

pub(super) fn installed() -> Option<Arc<dyn VrlLookups>> {
    INSTALLED.get().cloned()
}

/// Extra functions available when compiling a program.
/// `tables` collects the table names written in the program's `lookup` calls.
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
