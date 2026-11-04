import type { CatalogLookups, CatalogRow } from './model';

/** What the catalog's rows are grouped by: a team, a namespace, a tier, or the value of a tag. */
export type CatalogGrouping = '' | 'team' | 'namespace' | 'tier' | `tag:${string}`;

export interface CatalogGroup {
  /** The team's id, the namespace, the tier or the tag's value; empty for the rows with none. */
  key: string;
  rows: CatalogRow[];
}

export function parseGrouping(raw: string | null): CatalogGrouping {
  if (raw === 'team' || raw === 'namespace' || raw === 'tier') return raw;
  if (raw?.startsWith('tag:') && raw.length > 4) return raw as CatalogGrouping;
  return '';
}

/** The key of the tag a grouping is by, or `null` when it is by something else. */
export function groupingTag(grouping: CatalogGrouping): string | null {
  return grouping.startsWith('tag:') ? grouping.slice(4) : null;
}

/** Which group a row falls in, and the name the groups are ordered by. */
function groupOf(row: CatalogRow, grouping: CatalogGrouping, names: CatalogLookups): { key: string; order: string } {
  const definition = row.definition;
  const tag = groupingTag(grouping);

  if (tag !== null) {
    const value = definition?.tags?.[tag];
    return value === undefined ? { key: '', order: '' } : { key: value, order: value };
  }

  switch (grouping) {
    case 'team':
      return definition?.teamId ? { key: definition.teamId, order: names.team(definition.teamId) } : { key: '', order: '' };
    case 'namespace':
      return { key: row.namespace, order: row.namespace };
    case 'tier':
      // `p0` to `p5` sort as the tiers go.
      return definition?.tier ? { key: definition.tier, order: definition.tier } : { key: '', order: '' };
    default:
      return { key: '', order: '' };
  }
}

/**
 * The rows in groups, each keeping the order the rows came in; the groups by
 * name, the tiers from P0, and the rows with nothing to group by last.
 * Without a grouping, one group holds every row.
 */
export function groupCatalogRows(rows: readonly CatalogRow[], grouping: CatalogGrouping, names: CatalogLookups): CatalogGroup[] {
  if (!grouping) return [{ key: '', rows: [...rows] }];

  const groups = new Map<string, CatalogGroup & { order: string }>();

  for (const row of rows) {
    const { key, order } = groupOf(row, grouping, names);
    const group = groups.get(key) ?? { key, order, rows: [] };
    group.rows.push(row);
    groups.set(key, group);
  }

  return [...groups.values()]
    .sort((left, right) => (left.key === '' ? 1 : 0) - (right.key === '' ? 1 : 0) || left.order.localeCompare(right.order))
    .map(({ key, rows: members }) => ({ key, rows: members }));
}

/** Every tag key the services have: what the catalog can be grouped by besides team, namespace and tier. */
export function tagKeys(rows: readonly CatalogRow[]): string[] {
  return [...new Set(rows.flatMap((row) => Object.keys(row.definition?.tags ?? {})))].sort();
}
