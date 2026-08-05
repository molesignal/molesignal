import { useIsFetching, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';

import {
  resolveWindow,
  useTimeStore,
} from '@/stores/useTimeStore';

import type { DashboardTimeRangeResolver } from './policy';
import type { DashboardTimeRange } from '../schema';
import { useElementSize } from '../visualizations/shared/MeasuredContainer';

interface DashboardRefreshRuntimeOptions {
  dashboardUid: string;
  refreshNonce: number;
  maxTimeRangeMicros?: number | undefined;
  onRenderStateChange?: ((state: 'loading' | 'ready') => void) | undefined;
  onRefreshStateChange?: ((refreshing: boolean) => void) | undefined;
}

export interface DashboardRefreshRuntime {
  containerRef: React.RefObject<HTMLDivElement>;
  containerWidth: number;
  timeRange: DashboardTimeRange;
  timeRangeKey: string;
  resolveTimeRange: DashboardTimeRangeResolver;
}

export function useDashboardRefresh({
  dashboardUid,
  refreshNonce,
  maxTimeRangeMicros,
  onRenderStateChange,
  onRefreshStateChange,
}: DashboardRefreshRuntimeOptions): DashboardRefreshRuntime {
  const queryClient = useQueryClient();
  const timeWindow = useTimeStore((state) => state.window);
  const [containerRef, containerSize] = useElementSize({
    width: 1_200,
    height: 1,
  });
  const resolveTimeRange = React.useCallback<DashboardTimeRangeResolver>(
    (now = new Date()) => {
      const resolved = resolveWindow(timeWindow, now);
      const to = resolved.to.getTime() * 1_000;
      const from = resolved.from.getTime() * 1_000;
      return {
        from:
          maxTimeRangeMicros === undefined
            ? from
            : Math.max(from, to - maxTimeRangeMicros),
        to,
      };
    },
    [maxTimeRangeMicros, timeWindow],
  );
  const timeRangeKey = JSON.stringify([
    timeWindow.mode,
    timeWindow.from,
    timeWindow.to,
    maxTimeRangeMicros ?? null,
  ]);
  const activeQueryCount = useIsFetching({
    predicate: (query) => isDashboardQuery(query.queryKey, dashboardUid),
  });
  const previousRefreshNonce = React.useRef(refreshNonce);
  const pendingManualRefresh = React.useRef(false);
  const readyState = React.useRef({ uid: dashboardUid, ready: false });

  React.useEffect(() => {
    readyState.current = { uid: dashboardUid, ready: false };
  }, [dashboardUid]);

  const refetchDashboard = React.useCallback(() => {
    void queryClient.refetchQueries(
      {
        type: 'active',
        predicate: (query) => isDashboardQuery(query.queryKey, dashboardUid),
      },
      { cancelRefetch: false },
    );
  }, [dashboardUid, queryClient]);

  React.useEffect(() => {
    if (previousRefreshNonce.current === refreshNonce) return;
    previousRefreshNonce.current = refreshNonce;
    pendingManualRefresh.current = true;
    if (activeQueryCount === 0) {
      pendingManualRefresh.current = false;
      refetchDashboard();
    }
  }, [activeQueryCount, refreshNonce, refetchDashboard]);

  React.useEffect(() => {
    if (activeQueryCount !== 0 || !pendingManualRefresh.current) return;
    pendingManualRefresh.current = false;
    refetchDashboard();
  }, [activeQueryCount, refetchDashboard]);

  React.useEffect(() => {
    if (readyState.current.ready) {
      onRefreshStateChange?.(activeQueryCount > 0);
      return;
    }
    onRefreshStateChange?.(false);
    onRenderStateChange?.('loading');
    if (activeQueryCount > 0) return;
    const timer = globalThis.setTimeout(() => {
      readyState.current.ready = true;
      onRenderStateChange?.('ready');
      onRefreshStateChange?.(false);
    }, 250);
    return () => globalThis.clearTimeout(timer);
  }, [activeQueryCount, dashboardUid, onRefreshStateChange, onRenderStateChange]);

  return {
    containerRef,
    containerWidth: containerSize.width,
    timeRange: resolveTimeRange(),
    timeRangeKey,
    resolveTimeRange,
  };
}

function isDashboardQuery(
  queryKey: readonly unknown[],
  dashboardUid: string,
): boolean {
  return (
    (queryKey[0] === 'dashboard-engine-query' ||
      queryKey[0] === 'dashboard-engine-variable') &&
    queryKey[1] === dashboardUid
  );
}
