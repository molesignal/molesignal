import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import { useCursorPagination } from '@/pagination/useCursorPagination';
import { useAuthStore } from '@/stores/auth';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';

import {
  apiParamsFromFilters,
  parseApmFilters,
  writeApmFilter,
  type ApmUrlFilters,
} from './model';

export function useApmFilters() {
  const [searchParams, setSearchParams] = useSearchParams();
  const window = useTimeStore((state) => state.window);
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const filters = React.useMemo(
    () => parseApmFilters(searchParams.toString()),
    [searchParams],
  );
  const range = React.useMemo(() => resolveWindow(window), [window]);
  const paginationContext = React.useMemo(
    () =>
      JSON.stringify({
        orgId,
        from: range.from.getTime(),
        to: range.to.getTime(),
        namespace: filters.namespace,
        service: filters.service,
        environment: filters.environment,
        version: filters.version,
        search: filters.search,
        category: filters.category,
        resolution: filters.resolution,
        sort: filters.sort,
        direction: filters.direction,
      }),
    [filters, orgId, range],
  );
  const pagination = useCursorPagination({
    contextKey: paginationContext,
    defaultPageSize: 50,
  });
  const params = React.useMemo(
    () => apiParamsFromFilters(filters, range, pagination),
    [filters, pagination, range],
  );
  const setFilter = React.useCallback(
    (key: keyof ApmUrlFilters, value: string) => {
      setSearchParams(writeApmFilter(searchParams, key, value), { replace: true });
    },
    [searchParams, setSearchParams],
  );
  const clearFilters = React.useCallback(() => {
    const next = new URLSearchParams(searchParams);
    for (const key of ['namespace', 'service', 'environment', 'version', 'q', 'category', 'cursor']) {
      next.delete(key);
    }
    setSearchParams(next, { replace: true });
  }, [searchParams, setSearchParams]);
  const setFilters = React.useCallback(
    (values: Partial<Record<keyof ApmUrlFilters, string>>) => {
      let next = new URLSearchParams(searchParams);
      for (const [key, value] of Object.entries(values) as Array<
        [keyof ApmUrlFilters, string]
      >) {
        next = writeApmFilter(next, key, value);
      }
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams],
  );
  return {
    orgId,
    filters,
    params,
    setFilter,
    setFilters,
    clearFilters,
    range,
    pagination,
  };
}
