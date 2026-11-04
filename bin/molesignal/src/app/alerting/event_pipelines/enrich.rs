// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 两种走出进程的补充步骤。都只在开事件时跑，有超时；失败只算这一步没做，事件照这一步之前的样子
//! 往下走，原因记进时间线和流水线的计数。
//!
//! - 调用接口：把事件和元数据 POST 给组织的 HTTP 连接器（认证、地址防护都是连接器的），回答里只取
//!   步骤能改的字段，标签和注解按键合并。
//! - 查询补充：事件的值填进模板化的语句（`query_language::fill_template`，值只能待在语句的字符串里），
//!   结果按行数上限写进注解。

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::AlertPipelines;
use crate::{
    domain::{
        pipeline::alert::{
            AlertFields, AlertPipeline, EnrichQuery,
            fields::merge_answer,
            query::{placeholder_value, rows_text},
        },
        query::QueryRequest,
    },
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// How long a connector has to answer.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a query has to finish.
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of an error kept as the reason.
const MAX_REASON_CHARS: usize = 300;

impl AlertPipelines {
    /// Posts the event to the connector and merges what the answer changes into it. An answer the
    /// event could not take leaves it as it was.
    pub(super) async fn call_connector(
        &self,
        pipeline: &AlertPipeline,
        connector: &Id,
        fields: &AlertFields,
        event: &mut Value,
        metadata: &Map<String, Value>,
    ) -> Result<(), String> {
        let enricher = self
            .enricher
            .as_ref()
            .ok_or("connectors cannot be called from here")?;

        let mut body = event.clone();
        body["metadata"] = Value::Object(metadata.clone());

        let answer = tokio::time::timeout(
            CALL_TIMEOUT,
            enricher.call(&pipeline.org_id, connector, &body),
        )
        .await
        .map_err(|_| format!("no answer within {} seconds", CALL_TIMEOUT.as_secs()))?
        .map_err(|error| reason(&error))?;

        let Some(answer) = answer else {
            return Ok(());
        };

        let mut next = event.clone();
        merge_answer(&mut next, &answer);
        fields.changed(&next)?;
        *event = next;

        Ok(())
    }

    /// Runs the query with the event's values in it and writes its rows into the annotation. No rows
    /// leave the annotation out.
    pub(super) async fn run_query(
        &self,
        pipeline: &AlertPipeline,
        query: &EnrichQuery,
        event: &mut Value,
        metadata: &Map<String, Value>,
        at: TimestampMicros,
    ) -> Result<(), String> {
        let engine = self
            .queries
            .as_ref()
            .ok_or("queries cannot be run from here")?;

        let statement = query_language::fill_template(query.language, &query.statement, |path| {
            placeholder_value(event, metadata, path)
        })
        .map_err(|error| reason(&error))?;

        let from =
            at.0.saturating_sub(i64::from(query.window_secs).saturating_mul(1_000_000));

        let request = QueryRequest {
            org_id: pipeline.org_id.clone(),
            language: query.language,
            statement,
            time_range: TimeRange::new(TimestampMicros(from), at),
            stream: Some(query.stream_hint()),
            limit: Some(query.max_rows as usize),
            step: None,
            federation_clusters: Vec::new(),
        };

        let result = tokio::time::timeout(QUERY_TIMEOUT, engine.execute(request))
            .await
            .map_err(|_| format!("no result within {} seconds", QUERY_TIMEOUT.as_secs()))?
            .map_err(|error| reason(&error))?;

        if let Some(text) = rows_text(&result, query.max_rows) {
            match event.get_mut("annotations").and_then(Value::as_object_mut) {
                Some(annotations) => {
                    annotations.insert(query.annotation.clone(), Value::String(text));
                }
                None => event["annotations"] = json!({ query.annotation.clone(): text }),
            }
        }

        Ok(())
    }
}

fn reason(error: &crate::shared::Error) -> String {
    error.to_string().chars().take(MAX_REASON_CHARS).collect()
}
