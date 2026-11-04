// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Services an organization knows under an older name: what an alias reported
//! is read as its service's wherever APM reads, merged with what the service
//! reported itself. A query that names the alias reads it as it is.

use std::{collections::HashMap, hash::Hash, sync::Arc};

use async_trait::async_trait;
use futures::future::try_join_all;

use super::settings::OrgSettingsCache;
use crate::{
    domain::apm::{
        ApmQueryRepository, BucketQuery, CatalogQuery, ErrorGroupQuery, ErrorGroupRecord,
        ErrorSample, InstanceObservation, MergedBucket, ProjectionGap, ProjectionState,
        ServiceAlias, ServiceIdentity, ServiceObservation, VersionObservation,
    },
    shared::{Result, ids::Id, time::TimeRange},
};

pub(super) struct AliasedRepository {
    inner: Arc<dyn ApmQueryRepository>,
    settings: Arc<OrgSettingsCache>,
}

impl AliasedRepository {
    pub(super) fn new(inner: Arc<dyn ApmQueryRepository>, settings: Arc<OrgSettingsCache>) -> Self {
        Self { inner, settings }
    }

    /// How `query` reads its organization's aliases, and the queries that
    /// fetch its rows: itself, and one for each alias its filters leave out.
    /// None when no alias touches it.
    async fn plan<Q: Narrowed>(&self, org_id: &Id, query: &Q) -> Option<(Reading, Vec<Q>)> {
        let settings = self.settings.get(org_id).await;
        let (namespace, service) = query.filters();
        let reading = Reading::of(&settings.aliases, namespace, service);

        if reading.renames.is_empty() {
            return None;
        }

        let queries = std::iter::once(query.clone())
            .chain(
                reading
                    .extra
                    .iter()
                    .map(|(namespace, alias)| query.narrowed(namespace, alias)),
            )
            .collect();

        Some((reading, queries))
    }
}

/// The filters on a service a query has, and the query for one service.
trait Narrowed: Clone {
    fn filters(&self) -> (Option<&str>, Option<&str>);
    fn narrowed(&self, namespace: &str, service: &str) -> Self;
}

macro_rules! narrowed {
    ($($query:ty),*) => {$(
        impl Narrowed for $query {
            fn filters(&self) -> (Option<&str>, Option<&str>) {
                (self.namespace.as_deref(), self.service_name.as_deref())
            }

            fn narrowed(&self, namespace: &str, service: &str) -> Self {
                Self {
                    namespace: Some(namespace.to_owned()),
                    service_name: Some(service.to_owned()),
                    ..self.clone()
                }
            }
        }
    )*};
}

narrowed!(BucketQuery, CatalogQuery, ErrorGroupQuery);

trait HasService {
    fn service_mut(&mut self) -> &mut ServiceIdentity;
}

macro_rules! has_service {
    ($($row:ty),*) => {$(
        impl HasService for $row {
            fn service_mut(&mut self) -> &mut ServiceIdentity {
                &mut self.service
            }
        }
    )*};
}

has_service!(
    ServiceObservation,
    VersionObservation,
    InstanceObservation,
    ErrorGroupRecord,
    ErrorSample
);

impl HasService for MergedBucket {
    fn service_mut(&mut self) -> &mut ServiceIdentity {
        self.dimension.service_mut()
    }
}

/// How a query reads the aliases of its organization.
#[derive(Debug, Default)]
struct Reading {
    /// The service each alias is read as, by the alias's namespace and name.
    renames: HashMap<(String, String), Option<(String, String)>>,
    /// Aliases whose service the filters take in and that they leave out
    /// themselves: read by a query of their own.
    extra: Vec<(String, String)>,
}

impl Reading {
    fn of(aliases: &[ServiceAlias], namespace: Option<&str>, service: Option<&str>) -> Self {
        let takes = |in_namespace: &str, name: &str| {
            namespace.is_none_or(|namespace| namespace == in_namespace)
                && service.is_none_or(|service| service == name)
        };
        let mut reading = Self::default();

        for alias in aliases {
            let key = (alias.alias_namespace.clone(), alias.alias.clone());

            if takes(&alias.namespace, &alias.service) {
                if !takes(&alias.alias_namespace, &alias.alias) {
                    reading.extra.push(key.clone());
                }

                reading
                    .renames
                    .insert(key, Some((alias.namespace.clone(), alias.service.clone())));
            } else if takes(&alias.alias_namespace, &alias.alias) && service != Some(&alias.alias) {
                // Read as a service the filters leave out.
                reading.renames.insert(key, None);
            }
        }

        reading
    }

    /// The rows of every query, each service read as its alias says.
    fn apply<T: HasService>(&self, rows: Vec<Vec<T>>) -> Vec<T> {
        rows.into_iter()
            .flatten()
            .filter_map(|mut row| self.read(row.service_mut()).then_some(row))
            .collect()
    }

    /// Renames `service` when it is an alias; false when it is to be left out.
    fn read(&self, service: &mut ServiceIdentity) -> bool {
        match self
            .renames
            .get(&(service.namespace.clone(), service.name.clone()))
        {
            None => true,
            Some(None) => false,
            Some(Some((namespace, name))) => {
                service.namespace.clone_from(namespace);
                service.name.clone_from(name);
                true
            }
        }
    }
}

/// `rows` with those of one key made one; a row `absorb` cannot take in
/// (it hands it back) stays a row of its own.
fn merged<T, K: Hash + Eq>(
    rows: Vec<T>,
    key: impl Fn(&T) -> K,
    absorb: impl Fn(&mut T, T) -> Option<T>,
) -> Vec<T> {
    let mut merged = Vec::<T>::with_capacity(rows.len());
    let mut at = HashMap::<K, usize>::new();

    for row in rows {
        let key = key(&row);

        match at.get(&key) {
            Some(index) => {
                if let Some(row) = absorb(&mut merged[*index], row) {
                    merged.push(row);
                }
            }
            None => {
                at.insert(key, merged.len());
                merged.push(row);
            }
        }
    }

    merged
}

#[async_trait]
impl ApmQueryRepository for AliasedRepository {
    async fn query_buckets(&self, query: &BucketQuery) -> Result<Vec<MergedBucket>> {
        let Some((reading, queries)) = self.plan(&query.org_id, query).await else {
            return self.inner.query_buckets(query).await;
        };
        let rows =
            try_join_all(queries.iter().map(|query| self.inner.query_buckets(query))).await?;

        Ok(merged(
            reading.apply(rows),
            |bucket| (bucket.bucket_at, bucket.dimension.clone()),
            |into, bucket| {
                into.measurements
                    .merge(&bucket.measurements)
                    .err()
                    .map(|_| bucket)
            },
        ))
    }

    async fn list_services(&self, query: &CatalogQuery) -> Result<Vec<ServiceObservation>> {
        let Some((reading, queries)) = self.plan(&query.org_id, query).await else {
            return self.inner.list_services(query).await;
        };
        let rows =
            try_join_all(queries.iter().map(|query| self.inner.list_services(query))).await?;

        Ok(merged(
            reading.apply(rows),
            |row| row.service.clone(),
            |into, row| {
                into.first_seen_at = into.first_seen_at.min(row.first_seen_at);
                into.recent_instance_count = into
                    .recent_instance_count
                    .saturating_add(row.recent_instance_count);
                into.entry_kinds.extend(row.entry_kinds);
                into.entry_kinds.sort();
                into.entry_kinds.dedup();

                if row.last_seen_at > into.last_seen_at {
                    into.last_seen_at = row.last_seen_at;
                    into.runtime_language = row.runtime_language.or(into.runtime_language.take());
                    into.telemetry_sdk_name =
                        row.telemetry_sdk_name.or(into.telemetry_sdk_name.take());
                    into.telemetry_sdk_version = row
                        .telemetry_sdk_version
                        .or(into.telemetry_sdk_version.take());
                }

                None
            },
        ))
    }

    async fn list_versions(&self, query: &CatalogQuery) -> Result<Vec<VersionObservation>> {
        let Some((reading, queries)) = self.plan(&query.org_id, query).await else {
            return self.inner.list_versions(query).await;
        };
        let rows =
            try_join_all(queries.iter().map(|query| self.inner.list_versions(query))).await?;

        Ok(merged(
            reading.apply(rows),
            |row| (row.service.clone(), row.version.clone()),
            |into, row| {
                into.first_seen_at = into.first_seen_at.min(row.first_seen_at);
                into.last_seen_at = into.last_seen_at.max(row.last_seen_at);
                into.observation_count =
                    into.observation_count.saturating_add(row.observation_count);

                None
            },
        ))
    }

    async fn list_instances(&self, query: &CatalogQuery) -> Result<Vec<InstanceObservation>> {
        let Some((reading, queries)) = self.plan(&query.org_id, query).await else {
            return self.inner.list_instances(query).await;
        };
        let rows =
            try_join_all(queries.iter().map(|query| self.inner.list_instances(query))).await?;

        Ok(merged(
            reading.apply(rows),
            |row| (row.service.clone(), row.instance_id.clone()),
            |into, row| {
                into.first_seen_at = into.first_seen_at.min(row.first_seen_at);

                if row.last_seen_at > into.last_seen_at {
                    into.last_seen_at = row.last_seen_at;
                    into.version = row.version;
                    into.attributes = row.attributes;
                }

                None
            },
        ))
    }

    async fn list_error_groups(&self, query: &ErrorGroupQuery) -> Result<Vec<ErrorGroupRecord>> {
        let Some((reading, queries)) = self.plan(&query.org_id, query).await else {
            return self.inner.list_error_groups(query).await;
        };
        let rows = try_join_all(
            queries
                .iter()
                .map(|query| self.inner.list_error_groups(query)),
        )
        .await?;

        Ok(merged(
            reading.apply(rows),
            |row| (row.service.clone(), row.error.fingerprint.clone()),
            |into, row| {
                into.first_seen_at = into.first_seen_at.min(row.first_seen_at);
                into.occurrence_count = into.occurrence_count.saturating_add(row.occurrence_count);

                if row.last_seen_at > into.last_seen_at {
                    into.last_seen_at = row.last_seen_at;
                    into.representative_message = row.representative_message;
                    into.representative_stack = row.representative_stack;
                }

                None
            },
        ))
    }

    async fn list_error_samples(&self, org_id: &Id, fingerprint: &str) -> Result<Vec<ErrorSample>> {
        let mut samples = self.inner.list_error_samples(org_id, fingerprint).await?;
        let settings = self.settings.get(org_id).await;
        let reading = Reading::of(&settings.aliases, None, None);

        for sample in &mut samples {
            reading.read(&mut sample.service);
        }

        Ok(samples)
    }

    async fn projection_state(&self, org_id: &Id) -> Result<Option<ProjectionState>> {
        self.inner.projection_state(org_id).await
    }

    async fn projection_gaps(&self, org_id: &Id, range: TimeRange) -> Result<Vec<ProjectionGap>> {
        self.inner.projection_gaps(org_id, range).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(alias_namespace: &str, alias: &str, namespace: &str, service: &str) -> ServiceAlias {
        ServiceAlias {
            alias_namespace: alias_namespace.into(),
            alias: alias.into(),
            namespace: namespace.into(),
            service: service.into(),
        }
    }

    fn service(namespace: &str, name: &str) -> ServiceIdentity {
        ServiceIdentity {
            namespace: namespace.into(),
            name: name.into(),
            environment: "prod".into(),
        }
    }

    #[test]
    fn a_query_reads_an_alias_as_its_service_and_fetches_it_when_filtered_out() {
        let aliases = [
            alias("shop", "checkout-v1", "shop", "checkout"),
            alias("legacy", "cart", "shop", "cart"),
        ];

        let everything = Reading::of(&aliases, None, None);
        let mut old = service("shop", "checkout-v1");

        assert!(everything.extra.is_empty());
        assert!(everything.read(&mut old));
        assert_eq!(old.name, "checkout");

        // Asking for checkout fetches checkout-v1 as well.
        let checkout = Reading::of(&aliases, None, Some("checkout"));
        assert_eq!(
            checkout.extra,
            [("shop".to_owned(), "checkout-v1".to_owned())]
        );

        // Asking for the old name reads it as it is.
        let named = Reading::of(&aliases, None, Some("checkout-v1"));
        let mut old = service("shop", "checkout-v1");
        assert!(named.read(&mut old));
        assert_eq!(old.name, "checkout-v1");

        // In its own namespace, an alias read as a service elsewhere is left out.
        let legacy = Reading::of(&aliases, Some("legacy"), None);
        assert!(!legacy.read(&mut service("legacy", "cart")));
        assert!(legacy.read(&mut service("legacy", "orders")));
    }

    #[test]
    fn rows_of_one_key_merge_and_a_row_handed_back_stays_apart() {
        let rows = vec![("a", 1), ("b", 2), ("a", 3), ("a", 100)];

        let merged = merged(
            rows,
            |row| row.0,
            |into, row| {
                if row.1 >= 100 {
                    return Some(row);
                }

                into.1 += row.1;
                None
            },
        );

        assert_eq!(merged, [("a", 4), ("b", 2), ("a", 100)]);
    }
}
