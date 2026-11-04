import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { chartSeries, chartUnit, vitalRescale, vitalThresholds } from './answer';
import type { VizFilters } from './labels';
import { isAdditive, type RumVizBars, type RumVizSpec } from './spec';

/**
 * The number over time: a line for a percentile, an average, a lowest or a highest, a bar stacked
 * by group for a count or a sum. Where the number is a vital, two dashed lines mark where it turns
 * from good to poor. The groups are the lines, and what the pointer is on is read in all of them.
 * Dragging across the chart narrows the time range, as it does on the histogram.
 */
export function RumTimeseries({
  answer,
  bars,
  spec,
  title,
  measureName,
  language,
  none,
  filters,
  onRangeSelect,
}: {
  answer: RumAggregate;
  bars: RumVizBars;
  spec: RumVizSpec;
  title: string;
  /** What the one line is called when nothing is grouped. */
  measureName: string;
  language: string;
  /** What a group with no value is called. */
  none: string;
  filters: VizFilters;
  onRangeSelect: (range: { from: number; to: number }) => void;
}) {
  const { t } = useTranslation('rum');
  const measure = spec.measures[0]!;
  const additive = isAdditive(measure.fn);
  const grouped = spec.groups.length > 0;
  // A vital is drawn in the unit its thresholds are told in.
  const rescale = React.useMemo(() => vitalRescale(measure), [measure]);
  const unit = rescale?.unit ?? chartUnit(answer.measures[0]?.unit ?? 'none');

  const series = React.useMemo(
    () => chartSeries(answer, { bars, measures: spec.measures, groups: spec.groups, language, none, measureLabel: () => measureName, rescale }),
    [answer, bars, language, measureName, none, rescale, spec.groups, spec.measures],
  );
  const thresholds = React.useMemo(
    () => vitalThresholds(measure, { good: t('explorer.viz.vital_good'), poor: t('explorer.viz.vital_poor') }),
    [measure, t],
  );

  return (
    <div data-rum-viz-body="timeseries" className="flex min-h-0 flex-1 flex-col overflow-auto p-3">
      <h2 className="mb-2 font-sans text-sm font-strong text-tx-0">{title}</h2>
      <TimeSeriesChart
        height="auto"
        className="flex-[1_0_auto]"
        series={series}
        xDomain={[bars.frame.start, bars.frame.end]}
        ariaLabel={title}
        cursorScopeId="rum-viz"
        showLegend={grouped}
        onRangeSelect={onRangeSelect}
        onSeriesFilter={(labels, mode) =>
          filters.apply(Object.entries(labels).map(([field, value]) => ({ field, value, op: mode === 'include' ? '=' : '!=' })))
        }
        options={{
          drawStyle: additive ? 'bar' : 'line',
          stackMode: additive ? 'normal' : 'none',
          // A stacked bar has no use for a point on it, least of all on a bar with nothing in it.
          showPoints: additive ? 'never' : 'auto',
          legendMode: grouped ? 'list' : 'hidden',
          tooltipMode: 'all',
          tooltipSort: 'desc',
          // The scale reaches past the poor line, so that both lines and their words are in view.
          leftAxis: {
            scale: 'linear',
            ...(additive ? { min: 0 } : { softMin: 0 }),
            ...(thresholds.length > 0 ? { softMax: Math.max(...thresholds.map((line) => line.value)) * 1.12 } : {}),
            ...(unit ? { unit } : {}),
          },
          thresholds,
        }}
      />
    </div>
  );
}
