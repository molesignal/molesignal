import { BarChart3 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumFilter, RumRow } from '@/api/rum';
import { ResultHistogram } from '@/shell/query/histogram/Chart';
import { answerFromPayload, type HistogramCountsSource } from '@/shell/query/histogram/counts';
import type { HistogramPoint, HistogramRange } from '@/shell/query/histogram/model';
import { ModuleToggle } from '@/shell/query/ModuleToggle';
import { useExplorerLayoutStore } from '@/stores/useExplorerLayoutStore';
import type { TimeWindow } from '@/stores/useTimeStore';

import { isTroubled, type RumTab } from './model';

/** What the table needs to draw the histogram and its switch. */
export interface RumHistogramModel {
  visible: boolean;
  onVisibleChange: (visible: boolean) => void;
  points: HistogramPoint[];
  /** The range the rows were asked from. */
  range: HistogramRange;
  /** Where the counts of every matching row come from. */
  counts: HistogramCountsSource;
  onRangeSelect: (range: { from: number; to: number }) => void;
}

/** The list query whose matches the histogram counts. */
export interface CountedRumQuery {
  orgId: string;
  tab: RumTab;
  filters: RumFilter[];
  freeText: string[];
  /** Goes up when the reader asks for the results again. */
  epoch: number;
}

/**
 * The histogram as a module of the explorer: when the rows the query matches happened, the
 * troubled ones apart from the rest. Like the other histograms it can be closed, the choice is
 * remembered, and dragging across it narrows the time range.
 *
 * It counts every row the list query matches in the range, which is asked of the server with the
 * query the list itself sends; the loaded rows are what is drawn until that answers.
 */
export function useRumHistogram({
  rows,
  range,
  onTimeWindowChange,
  counted,
}: {
  rows: readonly RumRow[];
  range: { from: string; to: string };
  onTimeWindowChange: (window: TimeWindow) => void;
  counted: CountedRumQuery;
}): RumHistogramModel {
  const visible = useExplorerLayoutStore((state) => state.rumHistogramVisible);
  const onVisibleChange = useExplorerLayoutStore((state) => state.setRumHistogramVisible);

  const counts = React.useMemo<HistogramCountsSource>(() => {
    const { orgId, tab, filters, freeText, epoch } = counted;

    return {
      key: ['rum', orgId, tab, JSON.stringify(filters), JSON.stringify(freeText), epoch],
      load: async ({ range: asked, binMicros, originTime: originMicros }, signal) =>
        answerFromPayload(
          await rumApi.histogram(
            {
              type: tab,
              from: asked.start,
              to: asked.end,
              ...(filters.length > 0 ? { filters } : {}),
              ...(freeText.length > 0 ? { freeText: freeText } : {}),
              bucketTime: binMicros,
              originTime: originMicros,
            },
            signal,
          ),
        ),
    };
  }, [counted]);

  const tab = counted.tab;
  const points = React.useMemo<HistogramPoint[]>(
    () =>
      rows.map((row) => ({
        micros:
          Number.isFinite(row.timestamp) && row.timestamp > 0 ? row.timestamp : null,
        error: isTroubled(tab, row),
      })),
    [rows, tab],
  );
  const microsRange = React.useMemo(
    () => ({ start: Date.parse(range.from) * 1_000, end: Date.parse(range.to) * 1_000 }),
    [range.from, range.to],
  );
  const onRangeSelect = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      onTimeWindowChange({
        mode: 'absolute',
        from: new Date(from / 1_000).toISOString(),
        to: new Date(to / 1_000).toISOString(),
      }),
    [onTimeWindowChange],
  );

  return { visible, onVisibleChange, points, range: microsRange, counts, onRangeSelect };
}

/** The switch for the results header. */
export function RumHistogramToggle({ model }: { model: RumHistogramModel }) {
  const { t } = useTranslation('rum');

  return (
    <ModuleToggle
      size="sm"
      open={model.visible}
      icon={BarChart3}
      actionLabel={model.visible ? t('explorer.histogram.hide') : t('explorer.histogram.show')}
      onOpenChange={model.onVisibleChange}
    />
  );
}

/** The band under the results header, as wide as the results. Without rows there is nothing to draw. */
export function RumHistogramBand({
  model,
  tab,
  className,
}: {
  model: RumHistogramModel;
  tab: RumTab;
  className?: string;
}) {
  const { t } = useTranslation('rum');

  if (!model.visible || model.points.length === 0) return null;

  return (
    <ResultHistogram
      points={model.points}
      range={model.range}
      counts={model.counts}
      short
      labels={{
        chart: t('explorer.histogram.label', { type: t(`explorer.types.${tab}`) }),
        ok: t('explorer.histogram.ok'),
        errors: t(`explorer.histogram.troubled.${tab}`),
      }}
      onRangeSelect={model.onRangeSelect}
      {...(className ? { className } : {})}
    />
  );
}
