// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tenant-scoped realtime projection of pipeline configurations.
//!
//! 一个组织启用着的实时流水线一次读出、按（数据种类，来源流）缓存，写入路径上每批只查内存。同名的
//! 日志流和指标流是两个流，各挂各的。缓存留 [`TTL`]：别的节点改的流水线最多晚这么久生效；本节点
//! 改了（[`super::note_change`]）下一批就重读。配额与限流步骤的计数存在 [`limit_usage`]。
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use crate::{
    domain::{
        pipeline::realtime::{RealtimePipeline, RealtimePipelineRepository, validate_config},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod limit_usage;

/// 一条实时流水线挂在哪：数据种类和来源流。
type SourceKey = (StreamType, String);

/// 一个组织的实时流水线在缓存里留多久。
const TTL: Duration = Duration::from_secs(10);

/// 一个来源流上启用着的实时流水线。
#[derive(Clone)]
enum Slot {
    Ready(Box<RealtimePipeline>),
    /// 同一个来源流上启用了不止一条：写入这个来源时报冲突。
    Conflict,
    /// 存着的配置读不通：写入这个来源时报出来。
    Unreadable(String),
}

struct Loaded {
    at: Instant,
    changes: u64,
    slots: Arc<HashMap<SourceKey, Slot>>,
}

pub struct PgRealtimePipelineRepository {
    pool: PgPool,
    loaded: Mutex<HashMap<Id, Loaded>>,
}

impl PgRealtimePipelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            loaded: Mutex::new(HashMap::new()),
        }
    }

    async fn slots(&self, org: &Id) -> Result<Arc<HashMap<SourceKey, Slot>>> {
        let changes = super::changes();

        let cached = self.loaded.lock().ok().and_then(|loaded| {
            loaded
                .get(org)
                .filter(|entry| entry.changes == changes && entry.at.elapsed() < TTL)
                .map(|entry| entry.slots.clone())
        });

        if let Some(slots) = cached {
            return Ok(slots);
        }

        let slots = Arc::new(self.load(org).await?);

        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.insert(
                org.clone(),
                Loaded {
                    at: Instant::now(),
                    changes,
                    slots: slots.clone(),
                },
            );
        }

        Ok(slots)
    }

    async fn load(&self, org: &Id) -> Result<HashMap<SourceKey, Slot>> {
        let rows = sqlx::query("SELECT id, source_stream, target_stream, function_steps, updated_at FROM scheduled_pipelines WHERE org_id = $1 AND enabled = TRUE AND function_steps->>'mode' = 'realtime'")
            .bind(org.as_str()).fetch_all(&self.pool).await.map_err(crate::infra::persistence::sqlx_err)?;

        let mut slots = HashMap::new();

        for row in rows {
            let source: String = row.get("source_stream");
            let config: Json<Value> = row.get("function_steps");
            let key = (signal_of(&config.0), source);

            let slot = if slots.contains_key(&key) {
                Slot::Conflict
            } else {
                match projection(org, &key.1, &config.0, &row) {
                    Ok(pipeline) => Slot::Ready(Box::new(pipeline)),
                    Err(error) => Slot::Unreadable(error.to_string()),
                }
            };

            slots.insert(key, slot);
        }

        Ok(slots)
    }
}

/// 存着的配置说的数据种类；没写或写得不成样子时照日志算，配置的毛病在写入这个日志流时报出来。
fn signal_of(config: &Value) -> StreamType {
    config
        .get("signalType")
        .and_then(Value::as_str)
        .and_then(|name| StreamType::from_external_name(name).ok())
        .unwrap_or(StreamType::LOGS)
}

fn projection(org: &Id, source: &str, config: &Value, row: &PgRow) -> Result<RealtimePipeline> {
    let target: String = row.get("target_stream");
    let setup = validate_config(config, source, &target)?
        .ok_or_else(|| Error::internal("invalid realtime projection"))?;
    let steps = crate::domain::pipeline::processing::parse_processing_steps(config)?;

    Ok(RealtimePipeline {
        id: Id(row.get("id")),
        org_id: org.clone(),
        signal: setup.signal,
        target,
        routing: setup.routing,
        steps,
        updated_at: row.get::<TimestampMicros, _>("updated_at"),
    })
}

#[async_trait]
impl RealtimePipelineRepository for PgRealtimePipelineRepository {
    async fn for_source(
        &self,
        org: &Id,
        signal: StreamType,
        source: &str,
    ) -> Result<Option<RealtimePipeline>> {
        match self.slots(org).await?.get(&(signal, source.to_owned())) {
            None => Ok(None),
            Some(Slot::Ready(pipeline)) => Ok(Some(pipeline.as_ref().clone())),
            Some(Slot::Conflict) => Err(Error::conflict(
                "only one realtime pipeline may be enabled per source",
            )),
            Some(Slot::Unreadable(reason)) => Err(Error::invalid(format!(
                "the realtime pipeline of `{source}` cannot be read: {reason}"
            ))),
        }
    }
}
