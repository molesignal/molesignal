import * as React from 'react';

import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { buildTimeSeriesOptions } from './options';
import { framesToTimeSeries } from '../../dataframe';
import { useDashboardText } from '../../i18n';
import type { VisualizationProps } from '../shared/types';

export function TimeSeriesVisualization({
  panel,
  data,
  options,
  height,
  cursorScopeId,
}: VisualizationProps) {
  const tr = useDashboardText();
  const series = React.useMemo(
    () => framesToTimeSeries(data.frames),
    [data.frames],
  );
  return (
    <TimeSeriesChart
      series={series}
      height={Math.max(96, height)}
      xDomain={[data.timeRange.from, data.timeRange.to]}
      options={buildTimeSeriesOptions(panel, options)}
      cursorScopeId={cursorScopeId}
      loading={data.state === 'loading' && data.frames.length === 0}
      loadingLabel={tr('Loading chart…')}
      emptyLabel={tr('No data in this time range')}
      errorLabel={tr('Unable to render chart')}
      error={
        data.error
          ? new Error(data.error.message, { cause: data.error.cause })
          : null
      }
      ariaLabel={tr('Dashboard time series')}
    />
  );
}
