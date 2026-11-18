// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Removing a deleted organization's rows.
//!
//! Deleting an organization only marks it and takes access away. The purge then removes its rows in
//! batches, table by table, and the organization row last. No foreign key points at
//! `organizations`, so nothing cascades from that row: every table with an `org_id` or
//! `organization_id` column is purged by that column, read from the catalog at run time, and the
//! few tables that name the organization another way have statements of their own ([`STEPS`]).
//!
//! Foreign keys between the purged tables decide the order. A row whose key does not cascade must
//! go before the row it refers to. Otherwise a table goes after the tables whose deletes cascade
//! into it, so that a parent's batch takes its children along.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{PgPool, Row};

use crate::persistence::sqlx_err;

/// Tables that name the organization without an organization column of their own. They run first
/// in every pass, while the rows they reach the organization through are still there.
const STEPS: &[&str] = &[
    "DELETE FROM alert_rule_eval_state
      WHERE rule_id IN (SELECT id FROM alert_rules WHERE org_id = $1)",
    "DELETE FROM realtime_count_snapshots
      WHERE rule_id IN (SELECT id FROM alert_rules WHERE org_id = $1)",
    "DELETE FROM cluster_org_link WHERE local_org_id = $1",
    "DELETE FROM iam_cross_org_grants
      WHERE source_organization_id = $1 OR target_organization_id = $1",
];

/// An organization waiting for its purge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedOrganization {
    pub id: Id,
    pub deleted_at: TimestampMicros,
}

/// What one purge pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeProgress {
    pub rows_deleted: u64,
    /// The pass went through every table and found no row of the organization left.
    pub finished: bool,
}

#[async_trait]
pub trait OrganizationPurgeRepository: Send + Sync {
    /// Deleted organizations whose rows are still being removed, the longest deleted first.
    async fn deleted(&self) -> Result<Vec<DeletedOrganization>>;
    /// Delete the organization's rows, `batch` rows a statement, until none is left or `deadline`
    /// passes.
    async fn purge_rows(
        &self,
        organization_id: &Id,
        batch: i64,
        deadline: Instant,
    ) -> Result<PurgeProgress>;
    /// Delete the row of a deleted organization, once a pass finished.
    async fn remove(&self, organization_id: &Id) -> Result<()>;
}

pub struct PgOrganizationPurge {
    pool: PgPool,
}

impl PgOrganizationPurge {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The tables to purge, in order.
    async fn plan(&self) -> Result<Vec<Target>> {
        let tables = sqlx::query(
            "SELECT DISTINCT ON (class.relname)
                    class.relname::TEXT AS table_name,
                    attribute.attname::TEXT AS column_name
               FROM pg_class class
               JOIN pg_namespace namespace ON namespace.oid = class.relnamespace
               JOIN pg_attribute attribute ON attribute.attrelid = class.oid
              WHERE namespace.nspname = current_schema()
                AND class.relkind = 'r'
                AND attribute.attname IN ('org_id', 'organization_id')
                AND NOT attribute.attisdropped
                AND class.relname <> 'organizations'
              ORDER BY class.relname, attribute.attname",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let keys = sqlx::query(
            "SELECT child.relname::TEXT AS child,
                    parent.relname::TEXT AS parent,
                    constraint_row.confdeltype::TEXT AS action
               FROM pg_constraint constraint_row
               JOIN pg_class child ON child.oid = constraint_row.conrelid
               JOIN pg_class parent ON parent.oid = constraint_row.confrelid
               JOIN pg_namespace namespace ON namespace.oid = child.relnamespace
              WHERE constraint_row.contype = 'f'
                AND namespace.nspname = current_schema()",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let keys = keys
            .into_iter()
            .map(|row| {
                let action: String = row.try_get("action").map_err(sqlx_err)?;

                Ok(Key {
                    child: row.try_get("child").map_err(sqlx_err)?,
                    parent: row.try_get("parent").map_err(sqlx_err)?,
                    cascades: action == "c",
                    restricts: action == "a" || action == "r",
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let targets = tables
            .into_iter()
            .map(|row| {
                let table: String = row.try_get("table_name").map_err(sqlx_err)?;
                let whole = keys
                    .iter()
                    .any(|key| key.restricts && key.child == table && key.parent == table);

                Ok(Target {
                    column: row.try_get("column_name").map_err(sqlx_err)?,
                    table,
                    whole,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        order(targets, &keys)
    }
}

#[async_trait]
impl OrganizationPurgeRepository for PgOrganizationPurge {
    async fn deleted(&self) -> Result<Vec<DeletedOrganization>> {
        let rows = sqlx::query(
            "SELECT id, deleted_at
               FROM organizations
              WHERE deleted_at IS NOT NULL
              ORDER BY deleted_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(DeletedOrganization {
                    id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                    deleted_at: row.try_get("deleted_at").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn purge_rows(
        &self,
        organization_id: &Id,
        batch: i64,
        deadline: Instant,
    ) -> Result<PurgeProgress> {
        let plan = self.plan().await?;
        let mut deleted = 0;

        for step in STEPS {
            deleted += sqlx::query(step)
                .bind(&organization_id.0)
                .execute(&self.pool)
                .await
                .map_err(sqlx_err)?
                .rows_affected();
        }

        let unfinished = |deleted| PurgeProgress {
            rows_deleted: deleted,
            finished: false,
        };

        for target in &plan {
            let table = quoted(&target.table);
            let column = quoted(&target.column);

            if target.whole {
                let statement = format!("DELETE FROM {table} WHERE {column} = $1");

                deleted += sqlx::query(&statement)
                    .bind(&organization_id.0)
                    .execute(&self.pool)
                    .await
                    .map_err(sqlx_err)?
                    .rows_affected();
            } else {
                let statement = format!(
                    "DELETE FROM {table}
                      WHERE ctid = ANY(ARRAY(
                          SELECT ctid FROM {table} WHERE {column} = $1 LIMIT $2
                      ))"
                );

                loop {
                    let removed = sqlx::query(&statement)
                        .bind(&organization_id.0)
                        .bind(batch)
                        .execute(&self.pool)
                        .await
                        .map_err(sqlx_err)?
                        .rows_affected();

                    deleted += removed;

                    if removed < batch.max(1) as u64 {
                        break;
                    }

                    if Instant::now() >= deadline {
                        return Ok(unfinished(deleted));
                    }
                }
            }

            if Instant::now() >= deadline {
                return Ok(unfinished(deleted));
            }
        }

        Ok(PurgeProgress {
            rows_deleted: deleted,
            finished: deleted == 0,
        })
    }

    async fn remove(&self, organization_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM organizations WHERE id = $1 AND deleted_at IS NOT NULL")
            .bind(&organization_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}

/// A table the purge empties of the organization's rows, and the column naming the organization.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    table: String,
    column: String,
    /// The table refers to itself without cascading: one statement takes all its rows, so no
    /// batch leaves behind a row that a deleted row is still referred to by.
    whole: bool,
}

/// A foreign key: rows of `child` refer to rows of `parent`.
#[derive(Debug, Clone)]
struct Key {
    child: String,
    parent: String,
    /// Deleting a parent row deletes the child rows.
    cascades: bool,
    /// Deleting a parent row fails while a child row refers to it (`NO ACTION`, `RESTRICT`).
    restricts: bool,
}

/// The order to purge `targets` in.
///
/// A target owns the rows its deletes remove: its own, and those of every table its deletes
/// cascade into. When a table refers to another without cascading, the referring rows have to be
/// gone before any target that owns the referred rows, and does not own the referring ones, runs.
/// Among the targets free to go, one that no remaining target cascades into goes first; then by
/// name.
fn order(targets: Vec<Target>, keys: &[Key]) -> Result<Vec<Target>> {
    let names = targets
        .iter()
        .map(|target| target.table.clone())
        .collect::<BTreeSet<_>>();

    let mut owners = names
        .iter()
        .map(|name| (name.clone(), BTreeSet::from([name.clone()])))
        .collect::<BTreeMap<_, _>>();

    loop {
        let mut grew = false;

        for key in keys
            .iter()
            .filter(|key| key.cascades && key.child != key.parent)
        {
            let parents = owners.get(&key.parent).cloned().unwrap_or_default();
            let children = owners.entry(key.child.clone()).or_default();

            for owner in parents {
                grew |= children.insert(owner);
            }
        }

        if !grew {
            break;
        }
    }

    let mut before = names
        .iter()
        .map(|name| (name.clone(), BTreeSet::new()))
        .collect::<BTreeMap<String, BTreeSet<String>>>();

    for key in keys
        .iter()
        .filter(|key| key.restricts && key.child != key.parent)
    {
        let referring_owners = owners.get(&key.child).cloned().unwrap_or_default();

        // A referring target is purged by its own column; other referring rows go with the
        // targets that own them.
        let referring = if names.contains(&key.child) {
            BTreeSet::from([key.child.clone()])
        } else {
            referring_owners.clone()
        };

        for owner in owners.get(&key.parent).cloned().unwrap_or_default() {
            if referring_owners.contains(&owner) {
                continue;
            }

            if let Some(waits) = before.get_mut(&owner) {
                waits.extend(referring.iter().filter(|name| **name != owner).cloned());
            }
        }
    }

    let mut done = BTreeSet::new();
    let mut sequence = Vec::with_capacity(names.len());

    while sequence.len() < names.len() {
        let ready = names
            .iter()
            .filter(|name| !done.contains(*name) && before[*name].is_subset(&done))
            .collect::<Vec<_>>();

        let Some(first) = ready.first() else {
            let left = names.difference(&done).cloned().collect::<Vec<_>>();

            return Err(Error::internal(format!(
                "foreign keys leave no order to purge these tables in: {}",
                left.join(", ")
            )));
        };

        let next = ready
            .iter()
            .find(|name| {
                owners[**name]
                    .iter()
                    .all(|owner| owner == **name || done.contains(owner))
            })
            .unwrap_or(first);

        done.insert((*next).clone());
        sequence.push((*next).clone());
    }

    let mut by_name = targets
        .into_iter()
        .map(|target| (target.table.clone(), target))
        .collect::<BTreeMap<_, _>>();

    Ok(sequence
        .into_iter()
        .filter_map(|name| by_name.remove(&name))
        .collect())
}

/// A catalog name as an SQL identifier.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINES: [&str; 2] = [
        include_str!("../../../migrations/20260101000001_initial.sql"),
        include_str!("../../../migrations/20260101000003_iam_route_catalog.sql"),
    ];

    /// Tables that hold no organization's data.
    const GLOBAL: &[&str] = &[
        "acme_challenges",
        "agent_capability_contract_bindings",
        "agent_contract_versions",
        "cluster_event_delivery",
        "cluster_nodes",
        "i18n_language",
        "iam_catalog_versions",
        "iam_permissions",
        "iam_platform_administrators",
        "iam_routes",
        "inbound_mcp_oauth_clients",
        "instance_settings",
        "organizations",
        "remote_clusters",
        "run_claims",
        "search_admission_load",
        "seen_events",
        "signin_attempts",
        "signing_secrets",
        "synthetic_probe_certificate_authorities",
        "trace_runtime_policies",
        "user_passkeys",
        "users",
        "users_ext",
    ];

    /// Tables without an organization column whose rows cascade from a purged table.
    const CASCADED: &[&str] = &[
        "resource_share_sessions",
        "sso_identities",
        "sso_link_requests",
        "sso_signin_tests",
        "synthetic_probe_agents",
        "synthetic_probe_register_tokens",
    ];

    fn target(table: &str) -> Target {
        Target {
            table: table.into(),
            column: "org_id".into(),
            whole: false,
        }
    }

    fn key(child: &str, parent: &str, action: char) -> Key {
        Key {
            child: child.into(),
            parent: parent.into(),
            cascades: action == 'c',
            restricts: action == 'a' || action == 'r',
        }
    }

    fn position(order: &[Target], table: &str) -> usize {
        order
            .iter()
            .position(|target| target.table == table)
            .unwrap_or_else(|| panic!("{table} is missing"))
    }

    /// The tables a baseline creates, and whether each has an organization column.
    fn baseline_tables() -> BTreeMap<String, bool> {
        let mut tables = BTreeMap::new();

        for sql in BASELINES {
            let mut lines = sql.lines();

            while let Some(line) = lines.next() {
                let Some(rest) = line.strip_prefix("CREATE TABLE ") else {
                    continue;
                };

                let name = rest
                    .trim_start_matches("IF NOT EXISTS ")
                    .split([' ', '('])
                    .next()
                    .unwrap_or_default()
                    .to_string();

                let mut org_column = false;

                for column in lines.by_ref() {
                    if column.starts_with(')') {
                        break;
                    }

                    let first = column.split_whitespace().next().unwrap_or_default();
                    org_column |= first == "org_id" || first == "organization_id";
                }

                tables.insert(name, org_column);
            }
        }

        tables
    }

    #[test]
    fn every_table_is_purged_or_holds_no_organization_data() {
        let tables = baseline_tables();
        assert!(tables.len() > 200, "{} tables read", tables.len());

        let stepped = STEPS
            .iter()
            .filter_map(|step| step.strip_prefix("DELETE FROM "))
            .filter_map(|rest| rest.split_whitespace().next())
            .collect::<BTreeSet<_>>();

        let without_column = tables
            .iter()
            .filter(|(_, org_column)| !**org_column)
            .map(|(name, _)| name.as_str())
            .collect::<BTreeSet<_>>();

        let classified = GLOBAL
            .iter()
            .chain(CASCADED)
            .copied()
            .chain(stepped.iter().copied())
            .collect::<BTreeSet<_>>();

        let unclassified = without_column.difference(&classified).collect::<Vec<_>>();
        let stale = classified.difference(&without_column).collect::<Vec<_>>();

        assert!(
            unclassified.is_empty(),
            "a table without an org_id or organization_id column must be named here: \
             GLOBAL, CASCADED or a purge step: {unclassified:?}"
        );
        assert!(
            stale.is_empty(),
            "named here but not a table without an organization column: {stale:?}"
        );
    }

    #[test]
    fn rows_that_refer_without_cascading_go_first() {
        let targets = [
            "synthetic_monitors",
            "synthetic_probe_locations",
            "synthetic_probe_tasks",
            "synthetic_results",
        ]
        .map(target)
        .to_vec();

        let keys = [
            key("synthetic_probe_tasks", "synthetic_monitors", 'c'),
            key("synthetic_results", "synthetic_monitors", 'c'),
            key("synthetic_results", "synthetic_probe_tasks", 'a'),
            key("synthetic_probe_tasks", "synthetic_probe_locations", 'a'),
            key("synthetic_results", "synthetic_probe_locations", 'a'),
        ];

        let order = order(targets, &keys).expect("an order");

        assert!(position(&order, "synthetic_results") < position(&order, "synthetic_probe_tasks"));
        assert!(
            position(&order, "synthetic_probe_tasks")
                < position(&order, "synthetic_probe_locations")
        );
        assert!(
            position(&order, "synthetic_results") < position(&order, "synthetic_probe_locations")
        );
        // The monitors take their tasks and results along, so they go before them.
        assert_eq!(order[0].table, "synthetic_monitors");
    }

    #[test]
    fn rows_reached_through_a_cascade_bind_the_owner() {
        // Agents have no organization column: the locations' deletes take them, so tasks that
        // refer to an agent go before the locations.
        let targets = ["synthetic_probe_locations", "synthetic_probe_tasks"]
            .map(target)
            .to_vec();

        let keys = [
            key("synthetic_probe_agents", "synthetic_probe_locations", 'c'),
            key("synthetic_probe_tasks", "synthetic_probe_agents", 'a'),
        ];

        let order = order(targets, &keys).expect("an order");

        assert_eq!(order[0].table, "synthetic_probe_tasks");
        assert_eq!(order[1].table, "synthetic_probe_locations");
    }

    #[test]
    fn a_cycle_of_restricting_keys_is_an_error() {
        let targets = ["a", "b"].map(target).to_vec();
        let keys = [key("a", "b", 'a'), key("b", "a", 'r')];

        let error = order(targets, &keys).expect_err("no order");
        assert!(error.to_string().contains("a, b"), "{error}");
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quoted("catalog_segments"), "\"catalog_segments\"");
        assert_eq!(quoted("odd\"name"), "\"odd\"\"name\"");
    }
}
