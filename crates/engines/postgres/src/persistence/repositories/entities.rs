// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The `entities` and `entity_links` tables: what services run on, and what runs on or is part of
//! what.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domain::entity::{
    Entity, EntityKind, EntityLink, EntityQuery, EntityRef, EntityRepository, MAX_LISTED_ENTITIES,
    Relation, SERVICE,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;

pub struct PgEntityRepository {
    pool: PgPool,
}

impl PgEntityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "kind, key, name, attributes, first_seen_at, last_seen_at";
const LINK_COLS: &str = "from_kind, from_key, relation, to_kind, to_key, last_seen_at";

fn entity_of(row: &PgRow) -> Result<Entity> {
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;

    Ok(Entity {
        kind: EntityKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown entity kind {kind}")))?,
        key: row.try_get("key").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        attributes: row
            .try_get::<Json<BTreeMap<String, String>>, _>("attributes")
            .map(|attributes| attributes.0)
            .unwrap_or_default(),
        first_seen_at: row.try_get("first_seen_at").map_err(sqlx_err)?,
        last_seen_at: row.try_get("last_seen_at").map_err(sqlx_err)?,
    })
}

fn link_of(row: &PgRow) -> Result<EntityLink> {
    let relation: String = row.try_get("relation").map_err(sqlx_err)?;

    Ok(EntityLink {
        from: EntityRef {
            kind: row.try_get("from_kind").map_err(sqlx_err)?,
            key: row.try_get("from_key").map_err(sqlx_err)?,
        },
        relation: Relation::parse(&relation)
            .ok_or_else(|| Error::internal(format!("unknown entity relation {relation}")))?,
        to: EntityRef {
            kind: row.try_get("to_kind").map_err(sqlx_err)?,
            key: row.try_get("to_key").map_err(sqlx_err)?,
        },
        last_seen_at: row.try_get("last_seen_at").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl EntityRepository for PgEntityRepository {
    async fn record(&self, org_id: &Id, entities: &[Entity], links: &[EntityLink]) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for entity in entities {
            sqlx::query(
                "INSERT INTO entities (org_id, kind, key, name, attributes, first_seen_at, last_seen_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (org_id, kind, key) DO UPDATE SET
                    name = EXCLUDED.name,
                    attributes = EXCLUDED.attributes,
                    first_seen_at = LEAST(entities.first_seen_at, EXCLUDED.first_seen_at),
                    last_seen_at = GREATEST(entities.last_seen_at, EXCLUDED.last_seen_at)",
            )
            .bind(org_id.as_str())
            .bind(entity.kind.as_str())
            .bind(&entity.key)
            .bind(&entity.name)
            .bind(Json(&entity.attributes))
            .bind(entity.first_seen_at)
            .bind(entity.last_seen_at)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        for link in links {
            sqlx::query(
                "INSERT INTO entity_links
                    (org_id, from_kind, from_key, relation, to_kind, to_key, last_seen_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (org_id, from_kind, from_key, relation, to_kind, to_key) DO UPDATE SET
                    last_seen_at = GREATEST(entity_links.last_seen_at, EXCLUDED.last_seen_at)",
            )
            .bind(org_id.as_str())
            .bind(&link.from.kind)
            .bind(&link.from.key)
            .bind(link.relation.as_str())
            .bind(&link.to.kind)
            .bind(&link.to.key)
            .bind(link.last_seen_at)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)
    }

    async fn list(&self, org_id: &Id, query: &EntityQuery) -> Result<Vec<Entity>> {
        let limit = query.limit.clamp(1, MAX_LISTED_ENTITIES);
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM entities
             WHERE org_id = $1
               AND ($2::TEXT IS NULL OR kind = $2)
               AND ($3::TEXT IS NULL OR strpos(lower(name), lower($3)) > 0
                    OR strpos(lower(key), lower($3)) > 0)
               AND ($4::TIMESTAMPTZ IS NULL OR last_seen_at >= $4)
             ORDER BY last_seen_at DESC, key
             LIMIT $5"
        ))
        .bind(org_id.as_str())
        .bind(query.kind.map(EntityKind::as_str))
        .bind(
            query
                .search
                .as_deref()
                .map(str::trim)
                .filter(|search| !search.is_empty()),
        )
        .bind(query.seen_since)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(entity_of).collect()
    }

    async fn get(&self, org_id: &Id, kind: EntityKind, key: &str) -> Result<Option<Entity>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM entities WHERE org_id = $1 AND kind = $2 AND key = $3"
        ))
        .bind(org_id.as_str())
        .bind(kind.as_str())
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(entity_of)
        .transpose()
    }

    async fn links(&self, org_id: &Id, end: &EntityRef) -> Result<Vec<EntityLink>> {
        let rows = sqlx::query(&format!(
            "SELECT {LINK_COLS} FROM entity_links
             WHERE org_id = $1
               AND ((from_kind = $2 AND from_key = $3) OR (to_kind = $2 AND to_key = $3))
             ORDER BY last_seen_at DESC
             LIMIT 1000"
        ))
        .bind(org_id.as_str())
        .bind(&end.kind)
        .bind(&end.key)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(link_of).collect()
    }

    async fn service_counts(
        &self,
        org_id: &Id,
        ends: &[EntityRef],
    ) -> Result<BTreeMap<EntityRef, u64>> {
        if ends.is_empty() {
            return Ok(BTreeMap::new());
        }

        let kinds = ends.iter().map(|end| end.kind.clone()).collect::<Vec<_>>();
        let keys = ends.iter().map(|end| end.key.clone()).collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT to_kind, to_key, count(DISTINCT from_key) AS services
             FROM entity_links
             WHERE org_id = $1 AND from_kind = $4
               AND (to_kind, to_key) IN (SELECT * FROM UNNEST($2::TEXT[], $3::TEXT[]))
             GROUP BY to_kind, to_key",
        )
        .bind(org_id.as_str())
        .bind(&kinds)
        .bind(&keys)
        .bind(SERVICE)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| {
                let services: i64 = row.try_get("services").map_err(sqlx_err)?;
                let end = EntityRef {
                    kind: row.try_get("to_kind").map_err(sqlx_err)?,
                    key: row.try_get("to_key").map_err(sqlx_err)?,
                };

                Ok((end, u64::try_from(services).unwrap_or_default()))
            })
            .collect()
    }

    async fn forget_before(&self, cutoff: TimestampMicros) -> Result<u64> {
        let links = sqlx::query("DELETE FROM entity_links WHERE last_seen_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        let entities = sqlx::query("DELETE FROM entities WHERE last_seen_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        Ok(links + entities)
    }
}
