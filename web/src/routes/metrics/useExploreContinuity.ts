import * as React from 'react';

import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { useQueryTabSearchParams } from '@/shell/queryTabs';
import { useTimeStore } from '@/stores/useTimeStore';

import type { MetricsDrawStyle, MetricsStackMode } from './model';
import type { MetricsQueryOptions } from './queryOptions/model';
import { writeExecutedMetricsState } from './urlState';

interface MetricsExploreContinuityOptions {
  executedPromql: string | null;
  queryOptions: MetricsQueryOptions;
  drawStyle: MetricsDrawStyle;
  stackMode: MetricsStackMode;
  window: { from: Date; to: Date };
}

/** Keep the last successful query shareable and expose a predictable recovery range. */
export function useMetricsExploreContinuity({
  executedPromql,
  queryOptions,
  drawStyle,
  stackMode,
  window,
}: MetricsExploreContinuityOptions) {
  const [, setSearchParams] = useQueryTabSearchParams();
  const setTimeWindow = useTimeStore((state) => state.setWindow);

  React.useEffect(() => {
    if (!executedPromql) return;
    setSearchParams(
      (current) =>
        writeExecutedMetricsState(current, {
          promql: executedPromql,
          queryOptions,
          drawStyle,
          stackMode,
        }),
      { replace: true },
    );
  }, [
    drawStyle,
    executedPromql,
    queryOptions,
    setSearchParams,
    stackMode,
  ]);

  const widenResultWindow = React.useCallback(() => {
    setTimeWindow(widenTimeWindow({
      mode: 'absolute',
      from: window.from.toISOString(),
      to: window.to.toISOString(),
    }));
  }, [setTimeWindow, window]);

  return { widenResultWindow };
}
