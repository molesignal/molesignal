// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 动态字段遮掩：流级覆盖优先，全局规则按顺序首条命中。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;

use crate::{
    domain::{
        masking::{
            EffectiveFieldMasking, EffectiveFieldMaskingEntry, FieldMaskingAlgorithm,
            FieldMaskingProvider, FieldMaskingRule, FieldMaskingRuleRepository, FieldMaskingSource,
        },
        metrics::{
            PROMETHEUS_EXEMPLAR_LABELS_FIELD, PROMETHEUS_EXEMPLAR_VALUE_FIELD,
            PrometheusExemplarQueryResult,
        },
        query::{QueryLanguage, QueryRequest, QueryResult},
        stream::{MOLESIGNAL_SYSTEM_STREAM, StreamDefinition, StreamRepository, StreamType},
    },
    infra::{cipher::CipherRootKey, query::parser::extract_referenced_tables},
    shared::{Result, contracts::canonical_json_bytes, ids::Id},
};

mod aliases;
mod output;

use aliases::{propagate_derived_algorithms, sensitive_columns};
use output::mask_label_set;

pub struct FieldMaskingService {
    rules: Arc<dyn FieldMaskingRuleRepository>,
    streams: Arc<dyn StreamRepository>,
    root_key: CipherRootKey,
}

impl FieldMaskingService {
    pub fn new(
        rules: Arc<dyn FieldMaskingRuleRepository>,
        streams: Arc<dyn StreamRepository>,
        root_key: CipherRootKey,
    ) -> Self {
        Self {
            rules,
            streams,
            root_key,
        }
    }

    async fn effective_for_definition(
        &self,
        definition: &StreamDefinition,
        rules: &[FieldMaskingRule],
    ) -> Result<EffectiveFieldMasking> {
        let settings = self.streams.get_settings(&definition.id).await?;
        Ok(resolve_effective(
            definition,
            &settings.field_masking,
            rules,
        ))
    }

    async fn query_definitions(&self, request: &QueryRequest) -> Result<Vec<StreamDefinition>> {
        let mut definitions = Vec::new();
        if let Some(hint) = &request.stream {
            definitions.push(
                self.streams
                    .get(&request.org_id, &hint.name, hint.stream_type)
                    .await?,
            );
        }
        if request.language != QueryLanguage::Sql {
            if request.language == QueryLanguage::Promql {
                let mut needs_system_fallback = false;
                for name in
                    crate::infra::query::promql::referenced_metric_names(&request.statement)?
                {
                    match self
                        .streams
                        .get(&request.org_id, &name, StreamType::Metrics)
                        .await
                    {
                        Ok(definition) => definitions.push(definition),
                        Err(crate::shared::Error::NotFound(_)) => needs_system_fallback = true,
                        Err(error) => return Err(error),
                    }
                }
                if needs_system_fallback {
                    match self
                        .streams
                        .get(
                            &request.org_id,
                            MOLESIGNAL_SYSTEM_STREAM,
                            StreamType::Metrics,
                        )
                        .await
                    {
                        Ok(definition) => definitions.push(definition),
                        Err(crate::shared::Error::NotFound(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                definitions.sort_by(|left, right| left.id.0.cmp(&right.id.0));
                definitions.dedup_by(|left, right| left.id == right.id);
            }
            return Ok(definitions);
        }

        // The stream hint is an optimizer hint supplied by the client, not a security
        // boundary. Always inspect every SQL base table as well, otherwise a caller
        // could point the hint at an unmasked stream while selecting a protected one.
        let names = extract_referenced_tables(&request.statement)?
            .into_iter()
            .map(|table| table.name)
            .collect::<HashSet<_>>();
        if names.is_empty() {
            return Ok(definitions);
        }
        let primary_name = request.stream.as_ref().map(|hint| hint.name.as_str());
        let catalog = self.streams.list(&request.org_id).await?;
        for name in names {
            if primary_name == Some(name.as_str()) {
                continue;
            }
            // 与 SQL join planner 的类型解析顺序保持一致；primary table 则始终使用
            // 已校验的 stream hint。这样同名 metrics/logs 不会彼此污染遮掩策略。
            if let Some(definition) = catalog
                .iter()
                .filter(|definition| definition.name == name)
                .filter_map(|definition| {
                    sql_join_stream_type_rank(definition.stream_type).map(|rank| (rank, definition))
                })
                .min_by_key(|(rank, _)| *rank)
                .map(|(_, definition)| definition.clone())
            {
                definitions.push(definition);
            }
        }
        Ok(definitions)
    }

    async fn algorithms_for_request(
        &self,
        request: &QueryRequest,
    ) -> Result<HashMap<String, FieldMaskingAlgorithm>> {
        if request.language == QueryLanguage::Promql {
            return Ok(HashMap::new());
        }
        let definitions = self
            .query_definitions(request)
            .await?
            .into_iter()
            .filter(|definition| definition.stream_type != StreamType::Metrics)
            .collect::<Vec<_>>();
        if definitions.is_empty() {
            return Ok(HashMap::new());
        }
        let rules = self.rules.list(&request.org_id).await?;
        let mut algorithms = HashMap::new();
        for definition in &definitions {
            let effective = self.effective_for_definition(definition, &rules).await?;
            for field in effective.fields.into_iter().filter(|field| field.masked) {
                if let Some(algorithm) = field.algorithm {
                    insert_algorithm(
                        &mut algorithms,
                        normalize_identifier(&field.field),
                        algorithm,
                    );
                }
            }
        }
        propagate_derived_algorithms(request, &mut algorithms)?;
        Ok(algorithms)
    }
}

fn sql_join_stream_type_rank(stream_type: StreamType) -> Option<u8> {
    match stream_type {
        StreamType::Logs => Some(0),
        StreamType::Metrics => Some(1),
        StreamType::Traces => Some(2),
        StreamType::Extend => Some(3),
        StreamType::Profiles => None,
    }
}

fn resolve_effective(
    definition: &StreamDefinition,
    stream_overrides: &[crate::domain::masking::FieldMaskingOverride],
    rules: &[FieldMaskingRule],
) -> EffectiveFieldMasking {
    if definition.stream_type == StreamType::Metrics {
        return EffectiveFieldMasking {
            stream_id: definition.id.clone(),
            fields: definition
                .schema
                .fields
                .iter()
                .map(|field| EffectiveFieldMaskingEntry {
                    field: field.name.clone(),
                    masked: false,
                    source: FieldMaskingSource::None,
                    algorithm: None,
                    rule_id: None,
                    rule_name: None,
                    inherited_algorithm: None,
                    inherited_rule_id: None,
                    inherited_rule_name: None,
                })
                .collect(),
        };
    }
    let overrides = stream_overrides
        .iter()
        .map(|item| (item.field.as_str(), item))
        .collect::<HashMap<_, _>>();
    let fields = definition
        .schema
        .fields
        .iter()
        .map(|field| {
            let inherited = rules.iter().find(|rule| {
                rule_matches(rule, &field.name, &definition.name, definition.stream_type)
            });
            if let Some(overridden) = overrides.get(field.name.as_str()) {
                return EffectiveFieldMaskingEntry {
                    field: field.name.clone(),
                    masked: overridden.algorithm.is_some(),
                    source: FieldMaskingSource::Stream,
                    algorithm: overridden.algorithm.clone(),
                    rule_id: None,
                    rule_name: None,
                    inherited_algorithm: inherited.map(|rule| rule.algorithm.clone()),
                    inherited_rule_id: inherited.map(|rule| rule.id.clone()),
                    inherited_rule_name: inherited.map(|rule| rule.name.clone()),
                };
            }
            if let Some(rule) = inherited {
                return EffectiveFieldMaskingEntry {
                    field: field.name.clone(),
                    masked: true,
                    source: FieldMaskingSource::Global,
                    algorithm: Some(rule.algorithm.clone()),
                    rule_id: Some(rule.id.clone()),
                    rule_name: Some(rule.name.clone()),
                    inherited_algorithm: Some(rule.algorithm.clone()),
                    inherited_rule_id: Some(rule.id.clone()),
                    inherited_rule_name: Some(rule.name.clone()),
                };
            }
            EffectiveFieldMaskingEntry {
                field: field.name.clone(),
                masked: false,
                source: FieldMaskingSource::None,
                algorithm: None,
                rule_id: None,
                rule_name: None,
                inherited_algorithm: None,
                inherited_rule_id: None,
                inherited_rule_name: None,
            }
        })
        .collect();
    EffectiveFieldMasking {
        stream_id: definition.id.clone(),
        fields,
    }
}

#[async_trait]
impl FieldMaskingProvider for FieldMaskingService {
    async fn effective_for_stream(
        &self,
        org_id: &Id,
        stream_id: &Id,
    ) -> Result<EffectiveFieldMasking> {
        let definition = self.streams.get_by_id(stream_id).await?;
        if &definition.org_id != org_id {
            return Err(crate::shared::Error::not_found("stream"));
        }
        if definition.stream_type == StreamType::Metrics {
            return Ok(resolve_effective(&definition, &[], &[]));
        }
        let rules = self.rules.list(org_id).await?;
        self.effective_for_definition(&definition, &rules).await
    }

    async fn mask_result(&self, request: &QueryRequest, result: &mut QueryResult) -> Result<()> {
        if result.rows.is_empty() || result.columns.is_empty() {
            return Ok(());
        }
        let algorithms = self.algorithms_for_request(request).await?;
        if algorithms.is_empty() {
            return Ok(());
        }

        let columns = sensitive_columns(&result.columns, &algorithms);
        for row in &mut result.rows {
            for (index, algorithm) in &columns {
                if let Some(value) = row.get_mut(*index) {
                    mask_value(value, algorithm, &self.root_key, &request.org_id);
                }
            }
        }
        Ok(())
    }

    async fn mask_exemplars(
        &self,
        request: &QueryRequest,
        result: &mut PrometheusExemplarQueryResult,
    ) -> Result<()> {
        let algorithms = self.algorithms_for_request(request).await?;
        if [
            "value",
            "_timestamp",
            PROMETHEUS_EXEMPLAR_VALUE_FIELD,
            PROMETHEUS_EXEMPLAR_LABELS_FIELD,
        ]
        .iter()
        .any(|field| algorithms.contains_key(*field))
        {
            return Err(crate::shared::Error::forbidden(
                "masked exemplar value, timestamp, or label container cannot be represented by the Prometheus API",
            ));
        }
        for series in &mut result.series {
            mask_label_set(
                &mut series.series_labels,
                &algorithms,
                &self.root_key,
                &request.org_id,
            );
            for exemplar in &mut series.exemplars {
                mask_label_set(
                    &mut exemplar.labels,
                    &algorithms,
                    &self.root_key,
                    &request.org_id,
                );
            }
        }
        Ok(())
    }
}

fn rule_matches(
    rule: &FieldMaskingRule,
    field: &str,
    stream: &str,
    stream_type: crate::domain::stream::StreamType,
) -> bool {
    rule.enabled
        && wildcard_matches(&rule.field_pattern, field)
        && rule
            .stream_pattern
            .as_deref()
            .is_none_or(|pattern| wildcard_matches(pattern, stream))
        && rule
            .stream_type
            .is_none_or(|expected| expected == stream_type)
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern {
        let mut current = vec![false; value.len() + 1];
        if token == '*' {
            current[0] = previous[0];
        }
        for index in 1..=value.len() {
            current[index] = match token {
                '*' => previous[index] || current[index - 1],
                '?' => previous[index - 1],
                literal => previous[index - 1] && literal == value[index - 1],
            };
        }
        previous = current;
    }
    previous[value.len()]
}

fn normalize_identifier(value: &str) -> String {
    value
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .trim_matches(['"', '`'])
        .to_ascii_lowercase()
}

fn insert_algorithm(
    algorithms: &mut HashMap<String, FieldMaskingAlgorithm>,
    field: String,
    algorithm: FieldMaskingAlgorithm,
) {
    algorithms
        .entry(field)
        .and_modify(|existing| {
            if existing != &algorithm {
                *existing = FieldMaskingAlgorithm::default();
            }
        })
        .or_insert(algorithm);
}

fn mask_value(
    value: &mut serde_json::Value,
    algorithm: &FieldMaskingAlgorithm,
    root_key: &CipherRootKey,
    org_id: &Id,
) {
    if value.is_null() {
        return;
    }
    if matches!(algorithm, FieldMaskingAlgorithm::Hash) {
        let input = match &*value {
            serde_json::Value::String(text) => text.as_bytes().to_vec(),
            other => canonical_json_bytes(other),
        };
        *value = serde_json::Value::String(root_key.org_hmac_sha256(&org_id.0, &input));
        return;
    }
    let input = match &*value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    *value = serde_json::Value::String(mask_text(&input, algorithm));
}

fn mask_text(input: &str, algorithm: &FieldMaskingAlgorithm) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    match algorithm {
        FieldMaskingAlgorithm::Full { replacement } => replacement.clone(),
        FieldMaskingAlgorithm::Range {
            start,
            end,
            replacement,
        } => replace_range(&chars, *start, *end, replacement),
        FieldMaskingAlgorithm::Inner {
            prefix_chars,
            suffix_chars,
            replacement,
        } => {
            if prefix_chars.saturating_add(*suffix_chars) >= chars.len() {
                replacement.clone()
            } else {
                replace_range(
                    &chars,
                    *prefix_chars,
                    chars.len() - *suffix_chars,
                    replacement,
                )
            }
        }
        FieldMaskingAlgorithm::Outer {
            start,
            end,
            replacement,
        } => {
            let start = (*start).min(chars.len());
            let end = (*end).min(chars.len()).max(start);
            let visible = chars[start..end].iter().collect::<String>();
            format!("{replacement}{visible}{replacement}")
        }
        FieldMaskingAlgorithm::Hash => unreachable!("hash is handled before text masking"),
    }
}

fn replace_range(chars: &[char], start: usize, end: usize, replacement: &str) -> String {
    let start = start.min(chars.len());
    let end = end.min(chars.len()).max(start);
    if start == end {
        return replacement.to_string();
    }
    let mut output = chars[..start].iter().collect::<String>();
    output.push_str(replacement);
    output.extend(&chars[end..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        masking::FieldMaskingOverride,
        stream::{FieldDef, FieldType, Schema},
    };

    fn masking_definition() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("stream"),
            org_id: Id::from_string("org"),
            name: "customers".into(),
            stream_type: StreamType::Logs,
            schema: Schema {
                fields: ["email", "token", "untouched"]
                    .into_iter()
                    .map(|name| FieldDef {
                        name: name.into(),
                        data_type: FieldType::Utf8,
                        nullable: true,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    })
                    .collect(),
            },
            retention: None,
            created_at: crate::shared::time::TimestampMicros(1),
            updated_at: crate::shared::time::TimestampMicros(1),
        }
    }

    fn masking_rule(
        id: &str,
        name: &str,
        field: &str,
        algorithm: FieldMaskingAlgorithm,
    ) -> FieldMaskingRule {
        FieldMaskingRule {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            name: name.into(),
            priority: 0,
            enabled: true,
            field_pattern: field.into(),
            stream_pattern: None,
            stream_type: None,
            algorithm,
            created_at: crate::shared::time::TimestampMicros(1),
            updated_at: crate::shared::time::TimestampMicros(1),
        }
    }

    #[test]
    fn stream_override_wins_and_global_rules_use_first_match() {
        let definition = masking_definition();
        let first = masking_rule(
            "first",
            "first email",
            "email",
            FieldMaskingAlgorithm::default(),
        );
        let second = masking_rule(
            "second",
            "second email",
            "email",
            FieldMaskingAlgorithm::Hash,
        );
        let token = masking_rule(
            "token-rule",
            "token fallback",
            "token",
            FieldMaskingAlgorithm::default(),
        );
        let rules = vec![first, second, token];

        let inherited = resolve_effective(&definition, &[], &rules);
        let email = &inherited.fields[0];
        assert_eq!(email.source, FieldMaskingSource::Global);
        assert_eq!(
            email.rule_id.as_ref().map(|id| id.0.as_str()),
            Some("first")
        );

        let overridden = resolve_effective(
            &definition,
            &[
                FieldMaskingOverride {
                    field: "email".into(),
                    algorithm: None,
                },
                FieldMaskingOverride {
                    field: "token".into(),
                    algorithm: Some(FieldMaskingAlgorithm::Hash),
                },
            ],
            &rules,
        );
        assert!(!overridden.fields[0].masked);
        assert_eq!(overridden.fields[0].source, FieldMaskingSource::Stream);
        assert!(overridden.fields[0].inherited_algorithm.is_some());
        assert!(overridden.fields[1].masked);
        assert_eq!(
            overridden.fields[1].algorithm,
            Some(FieldMaskingAlgorithm::Hash)
        );
        assert_eq!(overridden.fields[2].source, FieldMaskingSource::None);
    }

    #[test]
    fn metrics_never_have_effective_field_masking() {
        let mut definition = masking_definition();
        definition.stream_type = StreamType::Metrics;
        let rules = vec![masking_rule(
            "all",
            "all fields",
            "*",
            FieldMaskingAlgorithm::Hash,
        )];
        let effective = resolve_effective(
            &definition,
            &[FieldMaskingOverride {
                field: "email".into(),
                algorithm: Some(FieldMaskingAlgorithm::default()),
            }],
            &rules,
        );
        assert!(effective.fields.iter().all(|field| !field.masked));
        assert!(
            effective
                .fields
                .iter()
                .all(|field| field.source == FieldMaskingSource::None)
        );
    }

    #[test]
    fn glob_supports_exact_star_and_question_mark() {
        assert!(wildcard_matches("email", "email"));
        assert!(wildcard_matches("user.*", "user.email"));
        assert!(wildcard_matches("ip_?", "ip_v"));
        assert!(!wildcard_matches("email", "user.email"));
    }

    #[test]
    fn algorithms_mask_unicode_by_character() {
        assert!(
            FieldMaskingAlgorithm::Range {
                start: 1,
                end: 1,
                replacement: "***".into(),
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            mask_text(
                "用户alice",
                &FieldMaskingAlgorithm::Range {
                    start: 1,
                    end: 3,
                    replacement: "***".into(),
                }
            ),
            "用***lice"
        );
        assert_eq!(
            mask_text(
                "short",
                &FieldMaskingAlgorithm::Range {
                    start: 20,
                    end: 21,
                    replacement: "***".into(),
                }
            ),
            "***"
        );
        assert_eq!(
            mask_text(
                "用户alice",
                &FieldMaskingAlgorithm::Inner {
                    prefix_chars: 1,
                    suffix_chars: 2,
                    replacement: "***".into(),
                }
            ),
            "用***ce"
        );
        assert_eq!(
            mask_text(
                "123456",
                &FieldMaskingAlgorithm::Outer {
                    start: 2,
                    end: 4,
                    replacement: "*".into(),
                }
            ),
            "*34*"
        );
    }

    #[test]
    fn derived_alias_inherits_sensitive_dependency() {
        let request = QueryRequest {
            org_id: Id::from_string("org"),
            language: QueryLanguage::Sql,
            statement:
                "WITH x AS (SELECT LOWER(email) AS normalized FROM users) SELECT normalized FROM x"
                    .into(),
            time_range: crate::shared::time::TimeRange::new(
                crate::shared::time::TimestampMicros(0),
                crate::shared::time::TimestampMicros(1),
            ),
            stream: None,
            limit: None,
            federation_clusters: Vec::new(),
        };
        let algorithms = HashMap::from([("email".into(), FieldMaskingAlgorithm::default())]);
        let mut algorithms = algorithms;
        propagate_derived_algorithms(&request, &mut algorithms).unwrap();
        assert!(sensitive_columns(&["normalized".into()], &algorithms).contains_key(&0));
    }

    #[test]
    fn promql_derived_label_inherits_sensitive_dependency() {
        let request = QueryRequest {
            org_id: Id::from_string("org"),
            language: QueryLanguage::Promql,
            statement: r#"label_join(requests_total, "identity", "/", "email", "region")"#.into(),
            time_range: crate::shared::time::TimeRange::new(
                crate::shared::time::TimestampMicros(0),
                crate::shared::time::TimestampMicros(1),
            ),
            stream: None,
            limit: None,
            federation_clusters: Vec::new(),
        };
        let mut algorithms = HashMap::from([("email".into(), FieldMaskingAlgorithm::default())]);
        propagate_derived_algorithms(&request, &mut algorithms).unwrap();
        assert!(sensitive_columns(&["identity".into()], &algorithms).contains_key(&0));
    }
}
