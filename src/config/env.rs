// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 将单下划线环境变量名无歧义地映射回嵌套配置路径。

use std::{cmp::Reverse, collections::HashMap};

use figment::{
    Error, Metadata, Profile, Provider,
    providers::Env,
    value::{Dict, Map},
};

use super::Settings;

/// 构造只接受 `MS_` 前缀、单下划线路径的配置环境变量 provider。
pub(super) fn provider(defaults: &Settings) -> anyhow::Result<UnderscoreEnv> {
    let paths = EnvKeyPaths::from_defaults(defaults)?;
    let env = Env::prefixed("MS_")
        .filter_map(move |key| paths.resolve(key.as_str()).map(|resolved| resolved.into()));
    Ok(UnderscoreEnv { env })
}

/// 给 Figment 提供环境变量数据，并让错误来源也显示真实的下划线变量名。
pub(super) struct UnderscoreEnv {
    env: Env,
}

impl Provider for UnderscoreEnv {
    fn metadata(&self) -> Metadata {
        Metadata::named("`MS_` environment variable(s)").interpolater(
            |_: &Profile, keys: &[&str]| format!("MS_{}", keys.join("_").to_ascii_uppercase()),
        )
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error> {
        self.env.data()
    }
}

/// 配置路径的扁平索引。
///
/// 单下划线同时承担层级分隔和字段名单词分隔，不能用 `split('_')`。这里从默认
/// `Settings` 收集所有已知路径；普通字段和可选字段精确匹配，只有明确登记的动态
/// map 才允许在其路径后追加用户自定义 key。
#[derive(Clone, Debug)]
struct EnvKeyPaths {
    exact: HashMap<String, String>,
    dynamic_tables: Vec<(String, String)>,
}

impl EnvKeyPaths {
    fn from_defaults(defaults: &Settings) -> anyhow::Result<Self> {
        let value = toml::Value::try_from(environment_schema(defaults))?;
        let mut paths = Self {
            exact: HashMap::new(),
            dynamic_tables: Vec::new(),
        };
        paths.collect(&value, &mut Vec::new())?;
        paths
            .dynamic_tables
            .sort_by_key(|(flattened, _)| Reverse(flattened.len()));
        Ok(paths)
    }

    fn collect(&mut self, value: &toml::Value, path: &mut Vec<String>) -> anyhow::Result<()> {
        if !path.is_empty() {
            let flattened = path.join("_");
            let dotted = path.join(".");
            if let Some(existing) = self.exact.insert(flattened.clone(), dotted.clone())
                && existing != dotted
            {
                anyhow::bail!(
                    "config paths `{existing}` and `{dotted}` both map to environment key `MS_{}`",
                    flattened.to_ascii_uppercase()
                );
            }
        }

        if let Some(table) = value.as_table() {
            for (key, child) in table {
                if key == DYNAMIC_KEY_SENTINEL {
                    self.dynamic_tables.push((path.join("_"), path.join(".")));
                    continue;
                }
                path.push(key.clone());
                self.collect(child, path)?;
                path.pop();
            }
        }
        Ok(())
    }

    fn resolve(&self, key: &str) -> Option<String> {
        let flattened = key.to_ascii_lowercase();
        if flattened.is_empty()
            || flattened.contains('.')
            || flattened.contains("__")
            || flattened.starts_with('_')
            || flattened.ends_with('_')
        {
            return None;
        }

        if let Some(path) = self.exact.get(&flattened) {
            return Some(path.clone());
        }

        self.dynamic_tables
            .iter()
            .find_map(|(table_key, table_path)| {
                let field = flattened.strip_prefix(table_key)?.strip_prefix('_')?;
                (!field.is_empty()).then(|| format!("{table_path}.{field}"))
            })
    }
}

const DYNAMIC_KEY_SENTINEL: &str = "__molesignal_environment_key__";

/// `Option::None` 不会进入 TOML value，空 map 也没有可供识别的子项；用仅用于建索引
/// 的副本补齐这些路径。字段访问由编译器校验，新增可选字段或动态 map 时也应在这里登记。
fn environment_schema(defaults: &Settings) -> Settings {
    let mut schema = defaults.clone();

    schema.auth.deprecated_jwt_secret = Some(String::new());
    schema.store.object.credentials_file = Some(std::path::PathBuf::new());
    schema.telemetry.log_directory = Some(String::new());
    schema.telemetry.log_file_prefix = Some(String::new());
    schema.telemetry.log_rotation = Some(String::new());
    schema.telemetry.log_max_files = Some(0);
    schema.telemetry.trace.external.custom_ca_file = Some(String::new());
    schema.telemetry.trace.external.client_certificate_file = Some(String::new());
    schema.telemetry.trace.external.client_key_file = Some(String::new());

    schema
        .telemetry
        .trace
        .external
        .headers
        .insert(DYNAMIC_KEY_SENTINEL.into(), String::new());
    schema
        .search
        .admission
        .groups
        .insert(DYNAMIC_KEY_SENTINEL.into(), 0);
    schema
        .search
        .admission
        .role_map
        .insert(DYNAMIC_KEY_SENTINEL.into(), String::new());
    schema
        .search
        .admission
        .cluster_groups
        .insert(DYNAMIC_KEY_SENTINEL.into(), 0);

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> EnvKeyPaths {
        EnvKeyPaths::from_defaults(&Settings::default()).expect("default settings paths")
    }

    #[test]
    fn nested_paths_keep_field_name_underscores() {
        let paths = paths();
        assert_eq!(
            paths.resolve("STORE_META_DSN").as_deref(),
            Some("store.meta.dsn")
        );
        assert_eq!(
            paths.resolve("CLUSTER_ADVERTISE_ADDR").as_deref(),
            Some("cluster.advertise_addr")
        );
        assert_eq!(
            paths
                .resolve("TELEMETRY_TRACE_SLOW_THRESHOLDS_OBJECT_STORE_MS")
                .as_deref(),
            Some("telemetry.trace.slow_thresholds.object_store_ms")
        );
    }

    #[test]
    fn absent_optional_fields_are_included_in_the_schema() {
        let paths = paths();
        assert_eq!(
            paths.resolve("TELEMETRY_LOG_DIRECTORY").as_deref(),
            Some("telemetry.log_directory")
        );
        assert_eq!(
            paths
                .resolve("TELEMETRY_TRACE_EXTERNAL_CUSTOM_CA_FILE")
                .as_deref(),
            Some("telemetry.trace.external.custom_ca_file")
        );
    }

    #[test]
    fn dynamic_map_keys_are_appended_without_splitting_their_underscores() {
        let paths = paths();
        assert_eq!(
            paths
                .resolve("TELEMETRY_TRACE_EXTERNAL_HEADERS_X_API_KEY")
                .as_deref(),
            Some("telemetry.trace.external.headers.x_api_key")
        );
        assert_eq!(
            paths
                .resolve("SEARCH_ADMISSION_GROUPS_BATCH_JOBS")
                .as_deref(),
            Some("search.admission.groups.batch_jobs")
        );
    }

    #[test]
    fn separately_consumed_ms_variables_are_not_treated_as_settings() {
        let paths = paths();
        assert_eq!(paths.resolve("INTELLIGENCE_OPENAI_API_KEY"), None);
        assert_eq!(paths.resolve("AUTH_JWT_SECRET_OVERRIDE"), None);
        assert_eq!(paths.resolve("LICENSE_FILE"), None);
        assert_eq!(paths.resolve("OBJECT_STORE_SECRET_KEY"), None);
    }

    #[test]
    fn dotted_and_double_underscore_names_are_not_configuration_keys() {
        let paths = paths();
        assert_eq!(paths.resolve("HTTP.PORT"), None);
        assert_eq!(paths.resolve("STORE__META__DSN"), None);
        assert_eq!(paths.resolve("RUN_IT"), None);
    }
}
