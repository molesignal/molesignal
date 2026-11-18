import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import { apmApi, apmQueryKeys } from '@/api/apm';
import * as dashboardsApi from '@/api/dashboards';
import * as savedViewsApi from '@/api/savedViews';
import * as statusPagesApi from '@/api/statusPages';
import * as syntheticsApi from '@/api/synthetics';
import * as teamsApi from '@/api/teams';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { catalogRows } from '@/routes/apm/catalog/model';
import { useApmFavorites } from '@/routes/apm/frame/useApmFavorites';
import { readFavoriteIds } from '@/routes/dashboards';
import { useAuthStore } from '@/stores/auth';

import {
  checkRows,
  dashboardRows,
  type ReaderScope,
  type ResourceRow,
  type ResourceTab,
  savedViewRows,
  serviceRows,
  statusPageRows,
} from './model';

/** The hour the services' figures are read over. */
export const SERVICE_WINDOW_SECS = 60 * 60;

/** The most services read: the busiest, as APM's catalog sorts them. */
const SERVICE_LIMIT = 200;

/** The page each tab lists the resources of: a tab the reader cannot open is not offered. */
const TAB_PATHS: Record<ResourceTab, string> = {
  services: '/apm',
  dashboards: '/dashboards',
  saved_views: '/saved-views',
  checks: '/synthetics',
  status_pages: '/status-pages',
};

const FRESH = { staleTime: 60_000, refetchInterval: 60_000 } as const;

/**
 * Every resource the workbench lists, by tab, for the tabs the reader can open, and who the reader is
 * as far as owning them goes.
 */
export function useResourceRows() {
  const access = useProductAccess();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const userId = useAuthStore((state) => state.ctx?.userId ?? '');
  const can = (tab: ResourceTab) => Boolean(orgId) && canAccessProductPath(TAB_PATHS[tab], access);
  const favorites = useApmFavorites();
  const [serviceParams] = React.useState(() => {
    const now = Date.now();
    return {
      from: (now - SERVICE_WINDOW_SECS * 1000) * 1000,
      to: now * 1000,
      resolution: 'auto' as const,
      limit: SERVICE_LIMIT,
      sort: 'request_count',
      direction: 'desc' as const,
    };
  });

  const services = useQuery({
    queryKey: [...apmQueryKeys.services(orgId, serviceParams), 'workbench'],
    queryFn: () => apmApi.services(serviceParams),
    enabled: can('services'),
    ...FRESH,
  });
  const definitions = useQuery({
    queryKey: apmQueryKeys.definitions(orgId),
    queryFn: apmApi.definitions,
    enabled: can('services'),
  });
  const dashboards = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
    enabled: can('dashboards'),
    ...FRESH,
  });
  const savedViews = useQuery({
    queryKey: ['saved-views'],
    queryFn: () => savedViewsApi.list(),
    enabled: can('saved_views'),
    ...FRESH,
  });
  const checks = useQuery({
    queryKey: ['synthetics', 'monitors'],
    queryFn: syntheticsApi.listMonitors,
    enabled: can('checks'),
    ...FRESH,
  });
  const statusPages = useQuery({
    queryKey: ['status-pages', 'list', 'active'],
    queryFn: () => statusPagesApi.list(),
    enabled: can('status_pages'),
    ...FRESH,
  });
  const teams = useQuery({ queryKey: ['teams'], queryFn: teamsApi.list, enabled: Boolean(orgId) });

  const reader = React.useMemo<ReaderScope>(() => {
    const mine = (teams.data ?? []).filter((team) => team.memberIds.includes(userId));
    return {
      userId,
      teamIds: new Set(mine.map((team) => team.id)),
      teammateIds: new Set(mine.flatMap((team) => team.memberIds)),
    };
  }, [teams.data, userId]);

  const rows = React.useMemo(() => {
    const byTab = new Map<ResourceTab, ResourceRow[]>();
    if (can('services')) {
      const catalog = catalogRows(services.data?.items ?? [], definitions.data ?? []);
      byTab.set('services', serviceRows(catalog, favorites.keys, SERVICE_WINDOW_SECS));
    }
    if (can('dashboards')) byTab.set('dashboards', dashboardRows(dashboards.data ?? [], readFavoriteIds(orgId)));
    if (can('saved_views')) byTab.set('saved_views', savedViewRows(savedViews.data ?? []));
    if (can('checks')) byTab.set('checks', checkRows(checks.data ?? []));
    if (can('status_pages')) byTab.set('status_pages', statusPageRows(statusPages.data ?? []));
    return byTab;
    // `can` reads `access` and `orgId`, both listed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    access,
    orgId,
    services.data,
    definitions.data,
    favorites.keys,
    dashboards.data,
    savedViews.data,
    checks.data,
    statusPages.data,
  ]);

  const loading: Record<ResourceTab, boolean> = {
    services: services.isLoading || definitions.isLoading,
    dashboards: dashboards.isLoading,
    saved_views: savedViews.isLoading,
    checks: checks.isLoading,
    status_pages: statusPages.isLoading,
  };

  return { rows, reader, loading };
}
