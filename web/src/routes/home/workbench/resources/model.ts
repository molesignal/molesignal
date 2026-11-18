import type { ServiceHealth } from '@/api/apm';
import type { SavedView } from '@/api/savedViews';
import type { StatusPage } from '@/api/statusPages/types';
import type { MonitorState, SyntheticMonitor } from '@/api/synthetics/types';
import type { CatalogRow } from '@/routes/apm/catalog/model';
import { serviceKey, servicePath } from '@/routes/apm/scope';
import { savedViewPath } from '@/routes/saved-views/path';
import type { Dashboard } from '@/types/dashboard';

export const RESOURCE_TABS = ['services', 'dashboards', 'saved_views', 'checks', 'status_pages'] as const;
export type ResourceTab = (typeof RESOURCE_TABS)[number];

/** Whose resources are listed: one's own, one's teams', the ones one keeps, or all of them. */
export const RESOURCE_SCOPES = ['mine', 'team', 'starred', 'all'] as const;
export type ResourceScope = (typeof RESOURCE_SCOPES)[number];

/** How a service or a check stands, as one word says it. */
export type ResourceState = ServiceHealth | MonitorState;

export interface ResourceRow {
  key: string;
  name: string;
  path: string;
  /** Who it belongs to, or who made it. */
  ownerId: string | null;
  teamId: string | null;
  /** A favourite of the reader's, or a pinned view. */
  starred: boolean;
  state?: ResourceState;
  /** What kind, which environment or which language: a word beside the name. */
  detail?: string;
  requestRate?: number;
  errorRate?: number;
  p95Time?: number;
  updatedAt?: number;
}

/** Who the reader is, as far as owning things goes. */
export interface ReaderScope {
  userId: string;
  teamIds: ReadonlySet<string>;
  /** Everyone in the reader's teams, the reader too. */
  teammateIds: ReadonlySet<string>;
}

const STATE_WEIGHT: Record<ResourceState, number> = {
  critical: 0,
  failing: 0,
  warning: 1,
  degraded: 1,
  flaky: 2,
  healthy: 3,
  no_traffic: 4,
  unknown: 4,
};

/** The worst first, then the busiest or the latest. */
export function byAttention(left: ResourceRow, right: ResourceRow): number {
  const weight = (row: ResourceRow) => (row.state ? STATE_WEIGHT[row.state] : 3);

  return (
    weight(left) - weight(right) ||
    (right.requestRate ?? 0) - (left.requestRate ?? 0) ||
    (right.updatedAt ?? 0) - (left.updatedAt ?? 0) ||
    left.name.localeCompare(right.name)
  );
}

export function inScope(row: ResourceRow, scope: ResourceScope, reader: ReaderScope): boolean {
  switch (scope) {
    case 'mine':
      return row.ownerId === reader.userId;
    case 'team':
      return (
        (row.teamId !== null && reader.teamIds.has(row.teamId)) ||
        (row.ownerId !== null && reader.teammateIds.has(row.ownerId))
      );
    case 'starred':
      return row.starred;
    case 'all':
      return true;
  }
}

/**
 * The rows of the scope chosen, and when it holds none the rows of the next one that does: one's
 * teams', then one's favourites, then all. Owners are often not filled in, and an empty list would
 * say nothing.
 */
export function scopedRows(
  rows: readonly ResourceRow[],
  scope: ResourceScope,
  reader: ReaderScope,
): { scope: ResourceScope; rows: ResourceRow[] } {
  const order = RESOURCE_SCOPES.slice(RESOURCE_SCOPES.indexOf(scope));

  for (const candidate of order) {
    const kept = rows.filter((row) => inScope(row, candidate, reader));
    if (kept.length > 0 || candidate === 'all') return { scope: candidate, rows: kept.sort(byAttention) };
  }
  return { scope: 'all', rows: [] };
}

export function serviceRows(
  catalog: readonly CatalogRow[],
  favorites: ReadonlySet<string>,
  windowSecs: number,
): ResourceRow[] {
  return catalog.map((row) => {
    const red = row.summary?.red;
    const environment = row.environment ?? '';

    return {
      key: row.key,
      name: row.name,
      path: servicePath({ namespace: row.namespace, name: row.name, environment }),
      ownerId: row.definition?.ownerId ?? null,
      teamId: row.definition?.teamId ?? null,
      starred: favorites.has(serviceKey({ namespace: row.namespace, name: row.name })),
      ...(row.summary ? { state: row.summary.health } : {}),
      ...(environment ? { detail: environment } : {}),
      ...(red ? { requestRate: red.requestCount / windowSecs, errorRate: red.errorRate } : {}),
      ...(red?.p95Time !== undefined ? { p95Time: red.p95Time } : {}),
    };
  });
}

export function dashboardRows(dashboards: readonly Dashboard[], favoriteIds: ReadonlySet<string>): ResourceRow[] {
  return dashboards.map((dashboard) => ({
    key: dashboard.id,
    name: dashboard.title,
    path: `/dashboards/${encodeURIComponent(dashboard.id)}`,
    ownerId: dashboard.createdBy ?? null,
    teamId: null,
    starred: favoriteIds.has(dashboard.id),
    updatedAt: dashboard.updatedAt,
  }));
}

export function savedViewRows(views: readonly SavedView[]): ResourceRow[] {
  return views.map((view) => ({
    key: view.id,
    name: view.name,
    path: savedViewPath(view),
    ownerId: view.ownerUserId,
    teamId: null,
    starred: view.pinned,
    detail: view.language,
    updatedAt: view.updatedAt,
  }));
}

export function checkRows(monitors: readonly SyntheticMonitor[]): ResourceRow[] {
  return monitors
    .filter((monitor) => monitor.archivedAt === undefined)
    .map((monitor) => ({
      key: monitor.id,
      name: monitor.name,
      path: `/synthetics/checks/${encodeURIComponent(monitor.id)}`,
      ownerId: monitor.createdBy,
      teamId: monitor.teamId ?? null,
      starred: false,
      state: monitor.state,
      detail: monitor.kind,
      updatedAt: monitor.updatedAt,
    }));
}

export function statusPageRows(pages: readonly StatusPage[]): ResourceRow[] {
  return pages.map((page) => ({
    key: page.id,
    name: page.name,
    path: `/status-pages/${encodeURIComponent(page.id)}`,
    ownerId: null,
    teamId: null,
    starred: false,
    detail: page.slug,
    updatedAt: page.updatedAt,
  }));
}
