import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { countTicks } from '@/shell/query/histogram/axis';
import { formatCompactNumber } from '@/viz/timeseries/formatters';

import type { FieldValueBars } from './model';

interface FieldValueBarsChartProps {
  bars: FieldValueBars;
  /** How a value on the axis is written. */
  format: (value: number) => string;
  /** Names the field for assistive technology. */
  name: string;
}

/** As many edges as the axis has room to label. */
const MAX_EDGE_LABELS = 5;

/**
 * How the values of a number are spread: a bar for each stretch of one width,
 * as tall as the values that fall in it. It is drawn the way the explorer's own
 * histogram is: a count axis in whole numbers to a line no bar passes, a value
 * axis labelled on the edges of bars, and what a bar stands for said in words.
 *
 * Nothing is laid on the bars: what the bar under the pointer holds is read in
 * the line above the plot.
 */
export function FieldValueBarsChart({ bars, format, name }: FieldValueBarsChartProps) {
  const { t } = useTranslation('common');
  const plot = React.useRef<HTMLDivElement>(null);
  const [cursor, setCursor] = React.useState<{ index: number; spoken: boolean } | null>(null);
  const { origin, width, counts } = bars;
  const last = counts.length - 1;
  const tallest = Math.max(0, ...counts);
  const ticks = countTicks(tallest, 3);
  const top = Math.max(1, ticks.at(-1) ?? 1);
  const tickLabels = ticks.map((tick) => formatCompactNumber(tick));
  const edge = (index: number) => format(origin + index * width);
  const share = (bars: number) => `${(bars / counts.length) * 100}%`;
  const step = Math.max(1, Math.ceil(counts.length / (MAX_EDGE_LABELS - 1)));
  const edges = Array.from({ length: Math.floor(counts.length / step) + 1 }, (_, index) => index * step);
  const pointed = cursor ? counts[cursor.index] : undefined;

  const indexUnder = (clientX: number) => {
    const box = plot.current?.getBoundingClientRect();
    if (!box || box.width === 0) return null;

    return Math.min(last, Math.max(0, Math.floor(((clientX - box.left) / box.width) * counts.length)));
  };
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      setCursor(null);
      return;
    }

    const move = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0;
    // The first arrow lands on the first bar; from there the arrows walk.
    const moved =
      move !== 0 ? (cursor ? Math.max(0, Math.min(last, cursor.index + move)) : 0)
      : event.key === 'Home' ? 0
      : event.key === 'End' ? last
      : null;

    if (moved !== null) {
      event.preventDefault();
      setCursor({ index: moved, spoken: true });
    }
  };

  return (
    <div data-field-analysis-bars>
      {/* A line of its own, always there: the plot does not move when a bar is pointed at. */}
      <p className="type-micro mb-[6px] flex h-[14px] items-center justify-between gap-2 font-mono leading-none tabular-nums text-tx-2">
        {cursor && pointed !== undefined ? (
          <>
            <span data-field-analysis-bar-range className="truncate">
              {t('field_analysis.bar_range', { from: edge(cursor.index), to: edge(cursor.index + 1) })}
            </span>
            <span className="shrink-0 font-strong text-tx-0">
              {t('field_analysis.bar_count', { count: pointed, formatted: pointed.toLocaleString() })}
            </span>
          </>
        ) : (
          <span className="truncate font-sans text-tx-3">{t('field_analysis.bar_width', { width: format(width) })}</span>
        )}
      </p>
      <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-[8px]">
        <div
          aria-hidden="true"
          className="relative h-[96px]"
          style={{ width: `${Math.max(...tickLabels.map((label) => label.length))}ch` }}
        >
          {ticks.map((tick, index) => (
            <span
              key={tick}
              className="type-micro absolute right-0 translate-y-1/2 font-mono leading-none tabular-nums text-tx-3"
              style={{ bottom: `${(tick / top) * 100}%` }}
            >
              {tickLabels[index]}
            </span>
          ))}
        </div>
        <div
          ref={plot}
          role="group"
          tabIndex={0}
          aria-label={t('field_analysis.distribution_aria', {
            name,
            count: counts.reduce((sum, held) => sum + held, 0),
            from: edge(0),
            to: edge(counts.length),
            width: format(width),
            max: tallest,
          })}
          data-field-analysis-plot
          className="relative h-[96px] cursor-crosshair select-none focus-visible:bg-tx-0/[0.04]"
          onPointerMove={(event) => {
            const index = indexUnder(event.clientX);
            if (index !== null) setCursor({ index, spoken: false });
          }}
          onPointerLeave={() => setCursor(null)}
          onKeyDown={onKeyDown}
          onBlur={() => setCursor(null)}
        >
          {ticks.map((tick) => (
            <span
              key={tick}
              aria-hidden="true"
              // The line the bars stand on is the one drawn firmly.
              className={cn('absolute inset-x-0 h-px', tick === 0 ? 'bg-tx-0/25' : 'bg-tx-0/10')}
              style={{ bottom: `${(tick / top) * 100}%` }}
            />
          ))}
          {cursor && (
            <span
              aria-hidden="true"
              className="absolute inset-y-0 bg-tx-0/[0.06]"
              style={{ left: share(cursor.index), width: share(1) }}
            />
          )}
          {counts.map(
            (held, index) =>
              held > 0 && (
                <span
                  // A bar is told by its place: the bars of a distribution are never reordered.
                  key={index}
                  aria-hidden="true"
                  data-field-analysis-bar={held}
                  className="absolute bottom-0 flex h-full flex-col-reverse px-[0.5px]"
                  style={{ left: share(index), width: share(1) }}
                >
                  <span
                    className="min-h-[2px] w-full rounded-t-[2px] bg-blue-soft"
                    style={{ height: `${(held / top) * 100}%` }}
                  />
                </span>
              ),
          )}
        </div>
        <span />
        <div aria-hidden="true" className="relative h-[20px]">
          {edges.map((index) => {
            const at = (index / counts.length) * 100;

            return (
              <React.Fragment key={index}>
                <span className="absolute top-0 h-[4px] w-px bg-tx-0/25" style={{ left: `${at}%` }} />
                <span
                  className={cn(
                    'type-micro absolute top-[7px] whitespace-nowrap font-mono leading-none tabular-nums text-tx-3',
                    // A label at an end of the axis stays inside it.
                    at < 4 ? 'translate-x-0' : at > 96 ? '-translate-x-full' : '-translate-x-1/2',
                  )}
                  style={{ left: `${at}%` }}
                >
                  {edge(index)}
                </span>
              </React.Fragment>
            );
          })}
        </div>
      </div>
      <span className="sr-only" aria-live="polite">
        {cursor?.spoken && pointed !== undefined
          ? t('field_analysis.bar_readout', {
              from: edge(cursor.index),
              to: edge(cursor.index + 1),
              formatted: pointed.toLocaleString(),
            })
          : ''}
      </span>
    </div>
  );
}
