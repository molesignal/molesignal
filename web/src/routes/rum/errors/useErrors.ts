import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { RumAggregateParams, RumIssueChange, RumIssueFilter, RumIssueListParams, RumIssueSort } from '@/api/rum';
import { toApiError } from '@/lib/http';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { DIMENSIONS, parseFilter, parseOrder, parseSort, type Picked } from './model';
import { type OverviewRange, scopeFilters } from '../overview/model';

/** What the errors page shows, as its address says it. */
export interface ErrorsView {
  status: RumIssueFilter;
  /** Everybody (empty), nobody (`none`) or one user. */
  assignee: string;
  search: string;
  /** What sent the errors, when it is narrowed to one thing. */
  source: string;
  sort: RumIssueSort;
  order: 'asc' | 'desc' | undefined;
  /** The issue open in the drawer. */
  issue: string;
  /** What is picked in the distributions above the list. */
  picked: Picked;
}

/**
 * The state of the errors page lives in its address: the filters, the order, what is picked in the
 * distributions and the issue that is open, so a view can be sent as it stands. A change of the
 * filters replaces the entry; opening an issue adds one, so Back closes it.
 */
export function useErrorsView() {
  const [params, setParams] = useSearchParams();

  const view = React.useMemo<ErrorsView>(
    () => ({
      status: parseFilter(params.get('status')),
      assignee: params.get('assignee') ?? '',
      search: params.get('q') ?? '',
      source: params.get('source') ?? '',
      sort: parseSort(params.get('sort')),
      order: parseOrder(params.get('order')),
      issue: params.get('issue') ?? '',
      picked: Object.fromEntries(
        DIMENSIONS.flatMap(({ key, param }) => {
          const value = params.get(param);

          return value ? [[key, value]] : [];
        }),
      ) as Picked,
    }),
    [params],
  );

  const set = React.useCallback(
    (changes: Record<string, string | null>, options: { push?: boolean } = {}) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);

          for (const [name, value] of Object.entries(changes)) {
            if (value) next.set(name, value);
            else next.delete(name);
          }

          return next;
        },
        { replace: !options.push },
      ),
    [setParams],
  );

  return { view, set };
}

/** The issues of a list, keeping the ones it had while the next are on their way. */
export function useIssueList(params: RumIssueListParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'issues', orgId, 'list', params],
    queryFn: ({ signal }) => rumApi.listIssues(params as RumIssueListParams, signal),
    enabled: Boolean(orgId && params),
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

/** One issue, with its numbers over a range, its latest errors and its stack. */
export function useIssueDetail(id: string, range: { from: number; to: number }) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'issues', orgId, 'detail', id, range],
    queryFn: ({ signal }) => rumApi.getIssue(id, range, signal),
    enabled: Boolean(orgId && id),
    staleTime: 15_000,
  });
}

/** Sets the state, the team or the holder of an issue, and reads the lists again. */
export function useChangeIssue() {
  const { t } = useTranslation('rum');
  const client = useQueryClient();

  return useMutation({
    mutationFn: ({ id, change }: { id: string; change: RumIssueChange }) => rumApi.updateIssue(id, change),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ['rum', 'issues'] });
      toast.success(t('errors.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
}

/** What sent the errors of an application over a range: `browser`, `flutter`… */
export function useSources(application: string, range: OverviewRange) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'issues', orgId, 'sources', application, range],
    queryFn: ({ signal }) =>
      rumApi.facets(
        {
          type: 'errors',
          from: range.from,
          to: range.to,
          filters: scopeFilters({ application, environment: '' }),
          field: 'source',
          limit: 10,
        },
        signal,
      ),
    select: (answer) => answer.values.map((entry) => entry.value),
    enabled: Boolean(orgId && application),
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  });
}

/**
 * One read of the numbers of an issue in its drawer. Unlike the reads of a list it does not keep
 * the numbers of the issue that was open before while the next are on their way: they are another
 * issue's.
 */
export function useIssueAggregate(name: string, params: RumAggregateParams | null) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  return useQuery({
    queryKey: ['rum', 'issues', orgId, 'aggregate', name, params],
    queryFn: ({ signal }) => rumApi.aggregate(params as RumAggregateParams, signal),
    enabled: Boolean(orgId && params),
    staleTime: 30_000,
  });
}
