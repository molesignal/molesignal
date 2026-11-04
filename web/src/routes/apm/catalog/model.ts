import type { ServiceDefinition, ServiceSummary, ServiceTier } from '@/api/apm';
import type { MetadataRules } from '@/api/apmSettings';
import type { Incident } from '@/types/alerting';

import { serviceKey } from '../scope';
import { entryKindsOf, stackOf } from '../service/serviceFacts';

/**
 * A row of the service catalog: a service in an environment as APM sees it,
 * with what people keep about the service. A service declared before it
 * first reports has a definition and no environment yet.
 */
export interface CatalogRow {
  key: string;
  namespace: string;
  name: string;
  environment: string | null;
  summary: ServiceSummary | null;
  definition: ServiceDefinition | null;
}

export function catalogRows(services: readonly ServiceSummary[], definitions: readonly ServiceDefinition[]): CatalogRow[] {
  const byKey = new Map(definitions.map((definition) => [serviceKey(definition), definition]));
  const seen = new Set<string>();

  const rows: CatalogRow[] = services.map((summary) => {
    const key = serviceKey(summary.service);
    seen.add(key);

    return {
      key: `${key}/${summary.service.environment}`,
      namespace: summary.service.namespace,
      name: summary.service.name,
      environment: summary.service.environment,
      summary,
      definition: byKey.get(key) ?? null,
    };
  });

  for (const definition of definitions) {
    const key = serviceKey(definition);
    if (seen.has(key)) continue;

    rows.push({
      key,
      namespace: definition.namespace,
      name: definition.name,
      environment: null,
      summary: null,
      definition,
    });
  }

  return rows;
}

export const CATALOG_COLUMNS = [
  'environment',
  'type',
  'team',
  'owner',
  'on_call',
  'tier',
  'alerts',
  'lifecycle',
  'tags',
  'language',
  'throughput',
  'error_rate',
  'p50',
  'p90',
  'p95',
  'p99',
  'average',
  'apdex',
  'instances',
  'region',
  'version',
  'last_deploy',
  'last_seen',
] as const;

export type CatalogColumn = (typeof CATALOG_COLUMNS)[number];

export const DEFAULT_CATALOG_COLUMNS: readonly CatalogColumn[] = ['environment', 'type', 'team', 'tier', 'alerts', 'throughput', 'error_rate', 'p95', 'version', 'last_deploy'];

/** How wide each column is drawn; the service's name takes what is left. */
export const CATALOG_COLUMN_WIDTH: Record<CatalogColumn, string> = {
  environment: '112px',
  type: '112px',
  team: '132px',
  owner: '132px',
  on_call: '132px',
  tier: '84px',
  alerts: '84px',
  lifecycle: '96px',
  tags: '200px',
  language: '136px',
  throughput: '156px',
  error_rate: '156px',
  p50: '84px',
  p90: '84px',
  p95: '84px',
  p99: '84px',
  average: '84px',
  apdex: '64px',
  instances: '72px',
  region: '128px',
  version: '120px',
  last_deploy: '128px',
  last_seen: '112px',
};

/** Columns whose figures read from the right. */
export const NUMERIC_COLUMNS: ReadonlySet<CatalogColumn> = new Set(['throughput', 'error_rate', 'p50', 'p90', 'p95', 'p99', 'average', 'apdex', 'instances']);

export interface CatalogFilter {
  search: string;
  environment: string;
  team: string;
  namespace?: string;
  /** A tag's key, or `key=value`. */
  tag?: string;
  /** Archived services are listed too; they are left out otherwise. */
  archived?: boolean;
  /** Only the services of these keys, when given: the reader's favorites. */
  only?: ReadonlySet<string> | null;
}

export function filterCatalogRows(rows: readonly CatalogRow[], filter: CatalogFilter): CatalogRow[] {
  const search = filter.search.trim().toLocaleLowerCase();

  return rows.filter(
    (row) =>
      (!search || row.name.toLocaleLowerCase().includes(search) || row.namespace.toLocaleLowerCase().includes(search)) &&
      (!filter.environment || row.environment === filter.environment) &&
      (!filter.team || row.definition?.teamId === filter.team) &&
      (!filter.namespace || row.namespace === filter.namespace) &&
      (!filter.tag || hasTag(row, filter.tag)) &&
      (filter.archived || !row.definition?.archived) &&
      (!filter.only || filter.only.has(serviceKey(row))),
  );
}

export type MetadataField = 'team' | 'owner' | 'repository';

/** What the catalog requirements ask of the row's service by its tier, and it does not have. */
export function missingMetadata(row: CatalogRow, rules: MetadataRules): MetadataField[] {
  const definition = row.definition;
  const tier = definition?.tier;

  if (!definition || !tier) return [];

  const asked = (from: ServiceTier | null) => from !== null && TIER_ORDER[tier] <= TIER_ORDER[from];
  const missing: MetadataField[] = [];

  if (asked(rules.teamFromTier) && !definition.teamId) missing.push('team');
  if (asked(rules.ownerFromTier) && !definition.ownerId) missing.push('owner');
  if (asked(rules.repositoryFromTier) && !definition.repositoryUrl) missing.push('repository');

  return missing;
}

/** The tags of a row's service as `[key, value]`, by key. */
export function tagsOf(row: CatalogRow): Array<[string, string]> {
  return Object.entries(row.definition?.tags ?? {}).sort(([left], [right]) => left.localeCompare(right));
}

/** A tag as people write it: `key=value`, or the key alone. */
export function tagLabel(key: string, value: string): string {
  return value ? `${key}=${value}` : key;
}

/** Whether the row's service has the tag: by its key, or `key=value`. */
export function hasTag(row: CatalogRow, tag: string): boolean {
  const at = tag.indexOf('=');
  const tags = row.definition?.tags ?? {};

  return at < 0 ? tag in tags : tags[tag.slice(0, at)] === tag.slice(at + 1);
}

/** Every tag the services have, each `key=value` once, by key. */
export function tagChoices(rows: readonly CatalogRow[]): string[] {
  return [...new Set(rows.flatMap((row) => tagsOf(row).map(([key, value]) => tagLabel(key, value))))].sort();
}

/** How long a service may go unseen before the catalog calls it inactive. */
export const INACTIVE_AFTER_MICROS = 86_400_000_000;

/** A service seen in the range that had not reported for a day by `asOfMicros`. */
export function isInactive(row: CatalogRow, asOfMicros: number): boolean {
  return row.summary !== null && row.summary.lastSeenAt < asOfMicros - INACTIVE_AFTER_MICROS;
}

export type CatalogSortKey = 'service' | CatalogColumn;

export interface CatalogSort {
  key: CatalogSortKey;
  direction: 'asc' | 'desc';
}

const TIER_ORDER: Record<ServiceTier, number> = { p0: 0, p1: 1, p2: 2, p3: 3, p4: 4, p5: 5 };

/** What a row is sorted by in a column; `null` sorts last whichever way the column goes. */
/** What the catalog names people, teams and alerts by. */
export interface CatalogLookups {
  team: (id: string) => string;
  user: (id: string) => string;
  /** The incidents going on about a row's service; none when unknown. */
  alerts?: (row: CatalogRow) => readonly Incident[];
}

export function sortValue(row: CatalogRow, key: CatalogSortKey, names: CatalogLookups): string | number | null {
  const red = row.summary?.red;

  switch (key) {
    case 'service':
      return row.name.toLocaleLowerCase();
    case 'environment':
      return row.environment;
    case 'type':
      return entryKindsOf(row.summary?.profile).join(',') || null;
    case 'team':
      return row.definition?.teamId ? names.team(row.definition.teamId) : null;
    case 'owner':
      return row.definition?.ownerId ? names.user(row.definition.ownerId) : null;
    case 'on_call':
      return null;
    case 'tier':
      return row.definition?.tier ? TIER_ORDER[row.definition.tier] : null;
    case 'alerts':
      return names.alerts?.(row).length || null;
    case 'lifecycle':
      return row.definition?.lifecycle ?? null;
    case 'tags':
      return tagsOf(row).map(([key, value]) => tagLabel(key, value)).join(' ') || null;
    case 'language':
      return stackOf(row.summary)?.toLocaleLowerCase() ?? null;
    case 'throughput':
      return red ? red.requestCount : null;
    case 'error_rate':
      return red && red.requestCount > 0 ? red.errorRate : null;
    case 'p50':
      return red?.p50Time ?? null;
    case 'p90':
      return red?.p90Time ?? null;
    case 'p95':
      return red?.p95Time ?? null;
    case 'p99':
      return red?.p99Time ?? null;
    case 'average':
      return red?.durationAverage ?? null;
    case 'apdex':
      return red?.apdex ?? null;
    case 'instances':
      return row.summary ? instanceCount(row.summary) : null;
    case 'region':
      return row.summary?.profile?.regions[0]?.value ?? null;
    case 'version':
      return row.summary?.currentVersion ?? null;
    case 'last_deploy':
      return row.summary?.currentVersionSince ?? null;
    case 'last_seen':
      return row.summary?.lastSeenAt ?? null;
  }
}

/** The rows in the order of `sort`, those of the services in `pinned` first. */
export function sortCatalogRows(rows: readonly CatalogRow[], sort: CatalogSort, names: CatalogLookups, pinned?: ReadonlySet<string>): CatalogRow[] {
  const sign = sort.direction === 'asc' ? 1 : -1;
  const rank = (row: CatalogRow) => (pinned?.has(serviceKey(row)) ? 0 : 1);

  return [...rows].sort((left, right) => {
    if (rank(left) !== rank(right)) return rank(left) - rank(right);

    const a = sortValue(left, sort.key, names);
    const b = sortValue(right, sort.key, names);

    if (a === null || b === null) return a === b ? left.name.localeCompare(right.name) : a === null ? 1 : -1;
    if (a < b) return -sign;
    if (a > b) return sign;
    return left.name.localeCompare(right.name);
  });
}

/** Requests a second over the time in view. */
export function throughputOf(requests: number, range: { from: Date; to: Date }): number {
  const seconds = Math.max(1, (range.to.getTime() - range.from.getTime()) / 1_000);
  return requests / seconds;
}

/** How many instances ran the service within the range; the recent count from before profiles where none came. */
export function instanceCount(summary: ServiceSummary): number {
  return summary.profile ? summary.profile.instanceCount : summary.instrumentation.recentInstanceCount;
}
