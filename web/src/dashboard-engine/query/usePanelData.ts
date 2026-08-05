import { keepPreviousData, useQueries } from '@tanstack/react-query';
import * as React from 'react';

import {
  executePanelQuery,
  type DataSourceQueryContext,
} from '../dataSources';
import { applyFieldConfig } from '../fieldConfig';
import { flattenElements } from '../model';
import {
  parseIntervalMicroseconds,
  resolveRefreshIntervalMilliseconds,
  type DashboardRefreshCadence,
  type DashboardTimeRangeResolver,
} from '../refresh/policy';
import type {
  DashboardElement,
  DashboardPanel,
  DashboardTimeRange,
  DataFrame,
  PanelData,
  PanelQuery,
} from '../schema';
import { applyTransformations } from '../transformations';
import type { DashboardVariableValues } from '../variables';
import {
  applyQueryPresentation,
  toExecutablePanelQuery,
} from './presentation';

export type DashboardPanelQueryExecutor = (
  panelId: string,
  query: PanelQuery,
  context: DataSourceQueryContext,
) => Promise<DataFrame[]>;

export interface PanelQueryRuntimeContext {
  dashboardUid: string;
  orgId: string;
  timeRange: DashboardTimeRange;
  timeRangeKey: string;
  resolveTimeRange: DashboardTimeRangeResolver;
  refreshCadence: DashboardRefreshCadence;
  containerWidth: number;
  dashboardColumns: number;
  queryLookup: Map<string, PanelQuery>;
  panelQueryExecutor?: DashboardPanelQueryExecutor | undefined;
  maxTimeRangeMicros?: number | undefined;
}

interface PanelQueryResult {
  frames: DataFrame[];
  timeRange: DashboardTimeRange;
}

export function usePanelData(
  panel: DashboardPanel,
  variables: DashboardVariableValues,
  context: PanelQueryRuntimeContext,
): PanelData {
  const lastSuccessfulData = React.useRef<{
    frames: DataFrame[];
    timeRange: DashboardTimeRange;
  } | null>(null);
  const fallbackTimeRange = React.useMemo(
    () => panelQueryTimeRange(panel, context.timeRange, context.maxTimeRangeMicros),
    [context.maxTimeRangeMicros, context.timeRange, panel],
  );
  const panelWidth =
    (context.containerWidth *
      Math.min(context.dashboardColumns, Math.max(1, panel.gridPos.w))) /
    context.dashboardColumns;
  const refreshInterval = resolveRefreshIntervalMilliseconds(
    context.refreshCadence,
    fallbackTimeRange,
    panelWidth,
  );
  const requests = panel.queries
    .filter((query) => query.enabled)
    .map((query) =>
      resolveSharedQuery(query, context.queryLookup, baseElementId(panel.id)),
    )
    .map((request) => ({
      ...request,
      executionQuery: toExecutablePanelQuery(request.query),
    }));
  const results = useQueries({
    queries: requests.map(({ executionQuery, cacheId }) => ({
      queryKey: [
        'dashboard-engine-query',
        context.dashboardUid,
        cacheId,
        executionQuery,
        variables,
        context.timeRangeKey,
        panel.timeOverride ?? null,
      ],
      queryFn: async (): Promise<PanelQueryResult> => {
        const timeRange = panelQueryTimeRange(
          panel,
          context.resolveTimeRange(),
          context.maxTimeRangeMicros,
        );
        const queryContext = {
          orgId: context.orgId,
          timeRange,
          variables,
        };
        const frames = context.panelQueryExecutor
          ? await context.panelQueryExecutor(
              baseElementId(panel.id),
              executionQuery,
              queryContext,
            )
          : await executePanelQuery(executionQuery, queryContext);
        return { frames, timeRange };
      },
      enabled: Boolean(context.orgId),
      staleTime: 15_000,
      gcTime: 60_000,
      placeholderData: keepPreviousData,
      refetchInterval: refreshInterval,
    })),
  });
  const failed = results.find((result) => result.isError);
  const rawFrames = results.flatMap((result, index) => {
    const frames = result.data?.frames ?? [];
    const request = requests[index];
    return request
      ? applyQueryPresentation(frames, request.query)
      : frames;
  });
  const frames = React.useMemo(
    () =>
      applyFieldConfig(
        applyTransformations(rawFrames, panel.transformations),
        panel.fieldConfig,
        panel.overrides,
      ),
    [panel.fieldConfig, panel.overrides, panel.transformations, rawFrames],
  );
  const resultTimeRange = results.reduce<DashboardTimeRange | undefined>(
    (latest, result) =>
      result.data && (!latest || result.data.timeRange.to > latest.to)
        ? result.data.timeRange
        : latest,
    undefined,
  );
  const pendingWithoutData = results.some(
    (result) => result.isPending && result.data === undefined,
  );
  const showPreviousData =
    pendingWithoutData &&
    frames.length === 0 &&
    lastSuccessfulData.current !== null;
  const visibleFrames = showPreviousData
    ? lastSuccessfulData.current?.frames ?? frames
    : frames;
  const visibleTimeRange = showPreviousData
    ? lastSuccessfulData.current?.timeRange ?? fallbackTimeRange
    : resultTimeRange ?? fallbackTimeRange;
  const initialLoading =
    visibleFrames.length === 0 &&
    pendingWithoutData &&
    !showPreviousData;
  const backgroundFetching =
    !initialLoading &&
    (showPreviousData || results.some((result) => result.isFetching));
  const terminalError = failed && visibleFrames.length === 0;

  React.useEffect(() => {
    if (
      failed ||
      results.some((result) => result.isFetching) ||
      results.some((result) => result.data === undefined)
    ) {
      return;
    }
    lastSuccessfulData.current = {
      frames,
      timeRange: resultTimeRange ?? fallbackTimeRange,
    };
  }, [failed, fallbackTimeRange, frames, resultTimeRange, results]);

  return {
    state: terminalError
      ? 'error'
      : initialLoading
        ? 'loading'
        : backgroundFetching
          ? 'streaming'
          : 'done',
    frames: visibleFrames,
    ...(terminalError
      ? {
          error: {
            message:
              failed.error instanceof Error
                ? failed.error.message
                : String(failed.error),
            cause: failed.error,
          },
        }
      : {}),
    timeRange: visibleTimeRange,
  };
}

export function buildDashboardQueryLookup(
  elements: readonly DashboardElement[],
): Map<string, PanelQuery> {
  const lookup = new Map<string, PanelQuery>();
  for (const element of flattenElements(elements)) {
    if (element.kind !== 'panel') continue;
    for (const query of element.queries) {
      lookup.set(`${element.id}:${query.refId}`, query);
    }
  }
  return lookup;
}

function resolveSharedQuery(
  query: PanelQuery,
  lookup: Map<string, PanelQuery>,
  panelId: string,
): { query: PanelQuery; cacheId: string } {
  if (!query.sharedQuery) {
    return { query, cacheId: `${panelId}:${query.refId}` };
  }
  const cacheId = `${query.sharedQuery.sourcePanelId}:${query.sharedQuery.sourceRefId}`;
  return { query: lookup.get(cacheId) ?? query, cacheId };
}

function panelQueryTimeRange(
  panel: DashboardPanel,
  dashboardRange: DashboardTimeRange,
  maxTimeRangeMicros?: number,
): DashboardTimeRange {
  const relative = parseIntervalMicroseconds(panel.timeOverride?.relativeTime);
  const shift = parseIntervalMicroseconds(panel.timeOverride?.timeShift);
  const to = dashboardRange.to - (shift || 0);
  return clampTimeRange(
    {
      from: relative ? to - relative : dashboardRange.from - (shift || 0),
      to,
    },
    maxTimeRangeMicros,
  );
}

function clampTimeRange(
  range: DashboardTimeRange,
  maxTimeRangeMicros?: number,
): DashboardTimeRange {
  if (
    maxTimeRangeMicros === undefined ||
    maxTimeRangeMicros <= 0 ||
    range.to - range.from <= maxTimeRangeMicros
  ) {
    return range;
  }
  return { from: range.to - maxTimeRangeMicros, to: range.to };
}

function baseElementId(id: string): string {
  return id.split('::repeat:')[0] ?? id;
}
