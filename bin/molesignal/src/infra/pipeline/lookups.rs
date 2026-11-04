// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 本节点上 VRL 的 `lookup` 与 `geoip_lookup` 读的数据：组织的扩展表，和 DB-IP 地理位置库。

use std::{
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::ExtendTable;
use crate::{
    infra::{
        enrichment::{GeoIp, GeoLocation, GeoLookup},
        runtime::vrl::VrlLookups,
    },
    shared::{Result, ids::Id},
};

/// DB-IP 库：本节点启动时就加载了的那份，或者第一次有程序查地理位置时才读的。
pub enum GeoSource {
    Loaded(Arc<dyn GeoLookup>),
    /// 不接收 RUM 的节点启动时不加载这份约 120 MB 的库；第一次 `geoip_lookup` 时读，读不到时一直
    /// 查不到。
    OnFirstUse(PathBuf),
}

pub struct PlatformLookups {
    tables: Arc<ExtendTable>,
    geo: GeoSource,
    opened: OnceLock<Arc<dyn GeoLookup>>,
}

impl PlatformLookups {
    pub fn new(tables: Arc<ExtendTable>, geo: GeoSource) -> Self {
        Self {
            tables,
            geo,
            opened: OnceLock::new(),
        }
    }

    fn geo(&self) -> &dyn GeoLookup {
        match &self.geo {
            GeoSource::Loaded(geo) => geo.as_ref(),
            GeoSource::OnFirstUse(path) => self
                .opened
                .get_or_init(|| {
                    tracing::info!(path = %path.display(), "loading the GeoIP database for VRL");
                    GeoIp::open_or_noop(path) as Arc<dyn GeoLookup>
                })
                .as_ref(),
        }
    }
}

#[async_trait]
impl VrlLookups for PlatformLookups {
    async fn prepare(&self, org: &Id, tables: &[String]) -> Result<()> {
        self.tables.prepare(org, tables).await
    }

    fn row(&self, org: &Id, table: &str, key: &str) -> Option<Value> {
        self.tables.lookup(org, table, key)
    }

    fn geoip(&self, ip: IpAddr) -> Option<Value> {
        self.geo().lookup(ip).map(place)
    }
}

/// 地理位置里有的字段，按 `geoip_lookup` 的说明命名。
fn place(location: GeoLocation) -> Value {
    let mut fields = Map::new();

    let texts = [
        ("country", location.country),
        ("country_name", location.country_name),
        ("continent_code", location.continent_code),
        ("continent", location.continent),
        ("region", location.region),
        ("city", location.city),
    ];

    for (name, value) in texts {
        if let Some(value) = value {
            fields.insert(name.into(), Value::String(value));
        }
    }

    for (name, value) in [
        ("latitude", location.latitude),
        ("longitude", location.longitude),
    ] {
        if let Some(number) = value.and_then(serde_json::Number::from_f64) {
            fields.insert(name.into(), Value::Number(number));
        }
    }

    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::{
        infra::{
            pipeline::{
                ExtendKvRepository, ExtendRow, ExtendTableDefinition, ExtendTableSummary,
                ExtendValueField,
            },
            runtime::VrlRuntime,
        },
        shared::time::TimestampMicros,
    };

    struct Beijing;

    impl GeoLookup for Beijing {
        fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
            (ip.to_string() == "1.2.3.4").then(|| GeoLocation {
                country: Some("CN".into()),
                country_name: Some("China".into()),
                continent_code: Some("AS".into()),
                continent: Some("Asia".into()),
                region: None,
                city: Some("Beijing".into()),
                latitude: Some(39.9),
                longitude: None,
            })
        }
    }

    /// One table, `owners`, of organization `o1`.
    struct Owners(Mutex<usize>);

    #[async_trait]
    impl ExtendKvRepository for Owners {
        async fn create_table(
            &self,
            table: ExtendTableDefinition,
        ) -> Result<ExtendTableDefinition> {
            Ok(table)
        }

        async fn update_table_fields(
            &self,
            _: &Id,
            _: &str,
            _: &[ExtendValueField],
            _: TimestampMicros,
            _: bool,
        ) -> Result<bool> {
            Ok(true)
        }

        async fn delete_table(&self, _: &Id, _: &str) -> Result<()> {
            Ok(())
        }

        async fn upsert(&self, _: ExtendRow) -> Result<()> {
            Ok(())
        }

        async fn delete(&self, _: &Id, _: &str, _: &str) -> Result<()> {
            Ok(())
        }

        async fn list_table(&self, org: &Id, table: &str) -> Result<Vec<ExtendRow>> {
            *self.0.lock().unwrap() += 1;

            if org.as_str() != "o1" || table != "owners" {
                return Ok(Vec::new());
            }

            Ok(vec![ExtendRow {
                id: Id("r1".into()),
                org_id: org.clone(),
                table_name: table.into(),
                key: "web-01".into(),
                value_json: json!({ "owner": "alice", "team": "payments" }),
                updated_at: TimestampMicros(0),
            }])
        }

        async fn list_tables(&self, _: &Id) -> Result<Vec<ExtendTableSummary>> {
            Ok(Vec::new())
        }
    }

    fn runtime() -> (VrlRuntime, Arc<Owners>) {
        let store = Arc::new(Owners(Mutex::new(0)));
        let lookups: Arc<dyn VrlLookups> = Arc::new(PlatformLookups::new(
            Arc::new(ExtendTable::new(store.clone())),
            GeoSource::Loaded(Arc::new(Beijing)),
        ));

        (VrlRuntime::new().with_lookups(lookups), store)
    }

    async fn run(runtime: &VrlRuntime, org: &str, source: &str, mut event: Value) -> Value {
        let org = Id(org.into());
        let program = runtime.compile_for(&org, source).unwrap();

        runtime.prepare(&org, &program).await.unwrap();
        runtime.run(&program, &mut event).unwrap();
        event
    }

    #[tokio::test]
    async fn lookup_reads_the_extend_tables_of_the_program_s_organization() {
        let (runtime, store) = runtime();
        let source = r#".owner = lookup("owners", .host).owner"#;

        assert_eq!(
            run(&runtime, "o1", source, json!({ "host": "web-01" })).await,
            json!({ "host": "web-01", "owner": "alice" })
        );
        // Another organization has no such table, and a missing row is null.
        assert_eq!(
            run(&runtime, "o2", source, json!({ "host": "web-01" })).await,
            json!({ "host": "web-01", "owner": null })
        );
        assert_eq!(
            run(&runtime, "o1", source, json!({ "host": "web-02" })).await,
            json!({ "host": "web-02", "owner": null })
        );
        // The table was read once per organization, then from memory.
        assert_eq!(*store.0.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn the_table_name_has_to_be_written_out() {
        let (runtime, _) = runtime();

        assert!(
            runtime
                .compile_for(&Id("o1".into()), r#".x = lookup(.table, "web-01")"#)
                .is_err()
        );
        // A program compiled only to be checked has no organization: lookup gives null.
        let program = runtime
            .compile(r#".owner = lookup("owners", "web-01")"#)
            .unwrap();
        let mut event = json!({});

        runtime.run(&program, &mut event).unwrap();
        assert_eq!(event, json!({ "owner": null }));
    }

    #[tokio::test]
    async fn geoip_lookup_says_where_an_address_is() {
        let (runtime, _) = runtime();

        assert_eq!(
            run(
                &runtime,
                "o1",
                ".place = geoip_lookup(.ip)",
                json!({ "ip": "1.2.3.4" })
            )
            .await["place"],
            json!({
                "country": "CN",
                "country_name": "China",
                "continent_code": "AS",
                "continent": "Asia",
                "city": "Beijing",
                "latitude": 39.9
            })
        );

        for ip in [
            json!("10.0.0.1"),
            json!("not an ip"),
            json!(42),
            json!(null),
        ] {
            assert_eq!(
                run(
                    &runtime,
                    "o1",
                    ".place = geoip_lookup(.ip)",
                    json!({ "ip": ip })
                )
                .await["place"],
                json!(null)
            );
        }
    }

    /// The program the pipeline editor writes for an extend table lookup
    /// (`web/src/routes/pipelines/processors/catalog.ts`), verbatim: change both together.
    #[tokio::test]
    async fn the_editors_lookup_program_copies_the_values_of_the_row() {
        let (runtime, _) = runtime();
        let program = r#"value = null
if exists(."labels.host") {
  value = ."labels.host"
} else if exists(.labels.host) {
  value = .labels.host
}
row = lookup("owners", value)
if is_object(row) {
  .labels.owner = row.owner
}"#;

        assert_eq!(
            run(
                &runtime,
                "o1",
                program,
                json!({ "labels": { "host": "web-01" } })
            )
            .await,
            json!({ "labels": { "host": "web-01", "owner": "alice" } })
        );
        // No row for the key, or no key at all: the event is left as it was.
        for event in [
            json!({ "labels": { "host": "web-09" } }),
            json!({ "labels": {} }),
        ] {
            assert_eq!(run(&runtime, "o1", program, event.clone()).await, event);
        }
    }
}
