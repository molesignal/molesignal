import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { formatCompactNumber } from '@/viz/timeseries/formatters';

import {
  type AxisClock,
  binWidthParts,
  countTicks,
  frameDays,
  momentLabel,
  tickLabelLength,
  timeTicks,
} from './axis';
import { barIndexAt, type Histogram } from './model';
import { HistogramReadout, type ReadoutAnchor } from './Readout';
import { useWidth } from './width';

export interface HistogramLabels {
  /** Names the chart for assistive technology. */
  chart: string;
  ok: string;
  errors: string;
}

/** What the bars of a chart are of: everything the query matches, the loaded results, or nothing yet. */
export type HistogramCounting = 'all' | 'loaded' | 'counting';

interface HistogramPlotProps {
  histogram: Histogram;
  clock: AxisClock;
  /** A handful of results needs less of a chart. */
  short: boolean;
  labels: HistogramLabels;
  /** Whether errors are drawn and read apart from the rest. */
  split: boolean;
  /** What the bars count, in words: said under the chart and to assistive technology. */
  scope: string;
  counting: HistogramCounting;
  /** The bars are those of the query before, kept while the ones asked for are counted. */
  stale: boolean;
  onRangeSelect: (range: { from: number; to: number }) => void;
  /** The element the chart is laid out in, for whoever settles its bars to measure. */
  onModule: (element: HTMLDivElement | null) => void;
  className?: string | undefined;
}

/** The width the time axis is labelled for until the plot has been measured. */
export const ASSUMED_PLOT_PIXELS = 960;
/** A label of the time axis takes its characters and the air that keeps it from the next. */
const LABEL_PIXELS_PER_CHARACTER = 6.5;
const LABEL_AIR_PIXELS = 44;
/** Shorter than this, a press is a click and selects nothing. */
const MIN_DRAG_PIXELS = 4;
const OK_FILL = 'bg-blue-soft';
const ERROR_FILL = 'bg-orange-soft';

interface Cursor {
  index: number;
  anchor: ReadoutAnchor;
  /** Moved with the keyboard: what the bar holds is also read out. */
  spoken: boolean;
}

/**
 * A histogram, drawn as a histogram is read. A bar stands for a length of time
 * a reader counts in, which is named under the chart with what the bars count;
 * the count axis runs in whole numbers to a line no bar passes; the time axis
 * is labelled on the edges of bars.
 *
 * Nothing is laid on the bars: the readout of the bar under the pointer stands
 * above the plot. Dragging across the bars narrows the time range to them.
 */
export function HistogramPlot({
  histogram,
  clock,
  short,
  labels,
  split,
  scope,
  counting,
  stale,
  onRangeSelect,
  onModule,
  className,
}: HistogramPlotProps) {
  const { t } = useTranslation('common');
  const moduleRef = React.useRef<HTMLDivElement | null>(null);
  const [plot, setPlot] = React.useState<HTMLDivElement | null>(null);
  const plotPixels = useWidth(plot) || ASSUMED_PLOT_PIXELS;
  const [cursor, setCursor] = React.useState<Cursor | null>(null);
  const [selection, setSelection] = React.useState<{ from: number; to: number } | null>(null);
  const press = React.useRef<{ x: number; index: number } | null>(null);

  const setModule = React.useCallback(
    (element: HTMLDivElement | null) => {
      moduleRef.current = element;
      onModule(element);
    },
    [onModule],
  );
  const ticks = React.useMemo(() => {
    const labelPixels =
      tickLabelLength(histogram.binMicros, clock) * LABEL_PIXELS_PER_CHARACTER + LABEL_AIR_PIXELS;

    return timeTicks(histogram.frame, histogram.binMicros, Math.max(2, plotPixels / labelPixels), clock);
  }, [clock, histogram, plotPixels]);

  const { buckets, binMicros, frame } = histogram;
  const span = frame.end - frame.start;
  const totals = buckets.map((bucket) => bucket.ok + bucket.err);
  const tallest = Math.max(0, ...totals);
  // With nothing counted there is a line to stand on and no scale to read against.
  const counts = tallest > 0 ? countTicks(tallest, short ? 3 : 4) : [0];
  const top = Math.max(1, counts.at(-1) ?? 1);
  const countLabels = counts.map((count) => formatCompactNumber(count));
  const days = frameDays(frame, clock);
  const width = binWidthParts(binMicros);
  const widthLabel = t(`histogram.unit_${width.unit}`, { value: width.value });
  const spansDays = days.length > 1;
  const rangeOf = (from: number, to: number) =>
    `${momentLabel(buckets[from]!.startMicros, binMicros, clock, spansDays)} – ${momentLabel(buckets[to]!.endMicros, binMicros, clock, spansDays)}`;
  const share = (count: number) => `${(count / buckets.length) * 100}%`;

  const cursorAt = (index: number, spoken: boolean): Cursor | null => {
    const plotBox = plot?.getBoundingClientRect();
    const moduleBox = moduleRef.current?.getBoundingClientRect();
    if (!plotBox || !moduleBox) return null;

    return {
      index,
      spoken,
      anchor: {
        x: plotBox.left + ((index + 0.5) / buckets.length) * plotBox.width,
        above: plotBox.top,
        below: moduleBox.bottom,
      },
    };
  };
  const indexUnder = (clientX: number) => {
    const box = plot?.getBoundingClientRect();

    return box ? barIndexAt(clientX - box.left, box.width, buckets.length) : 0;
  };
  const select = (from: number, to: number) => {
    const first = buckets[Math.min(from, to)];
    const last = buckets[Math.max(from, to)];

    // The results may have been replaced while a stretch was being marked.
    if (first && last) onRangeSelect({ from: first.startMicros, to: last.endMicros });
  };
  const release = () => {
    press.current = null;
    setSelection(null);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const index = indexUnder(event.clientX);

    if (press.current && Math.abs(event.clientX - press.current.x) >= MIN_DRAG_PIXELS) {
      setSelection({ from: press.current.index, to: index });
      setCursor(null);
      return;
    }
    if (!selection) setCursor(cursorAt(index, false));
  };
  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;

    press.current = { x: event.clientX, index: indexUnder(event.clientX) };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };
  const onPointerUp = () => {
    if (selection) select(selection.from, selection.to);
    release();
  };
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      release();
      setCursor(null);
      return;
    }

    const last = buckets.length - 1;
    const step = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0;
    // The first arrow lands on the newest bar; from there the arrows walk.
    const moved =
      step !== 0 ? (cursor ? Math.max(0, Math.min(last, cursor.index + step)) : last)
      : event.key === 'Home' ? 0
      : event.key === 'End' ? last
      : null;

    if (moved !== null) {
      event.preventDefault();
      setCursor(cursorAt(moved, true));
    } else if ((event.key === 'Enter' || event.key === ' ') && cursor) {
      event.preventDefault();
      select(cursor.index, cursor.index);
    }
  };

  const pointed = cursor ? buckets[cursor.index] : undefined;
  const selected = selection
    ? {
        first: Math.min(selection.from, selection.to),
        last: Math.min(Math.max(selection.from, selection.to), buckets.length - 1),
      }
    : null;

  return (
    <div
      ref={setModule}
      data-explorer-module="histogram"
      data-histogram-counting={counting}
      aria-busy={counting === 'counting' || undefined}
      // It stands on the surface of the results, as a chart does: no field of its own behind the bars.
      className={cn('shrink-0 px-4 pb-[8px] pt-[10px]', className)}
    >
      <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-[8px]">
        <div
          aria-hidden="true"
          data-histogram-axis="count"
          className={cn('relative', short ? 'h-[64px]' : 'h-[88px]')}
          style={{ width: `${Math.max(...countLabels.map((label) => label.length))}ch` }}
        >
          {counts.map((count, index) => (
            <span
              key={count}
              className="type-micro absolute right-0 translate-y-1/2 font-mono leading-none tabular-nums text-tx-3"
              style={{ bottom: `${(count / top) * 100}%` }}
            >
              {countLabels[index]}
            </span>
          ))}
        </div>
        <div
          ref={setPlot}
          role="group"
          tabIndex={0}
          aria-label={t('histogram.summary', {
            name: labels.chart,
            scope,
            from: momentLabel(frame.start, binMicros, clock, true),
            to: momentLabel(frame.end, binMicros, clock, true),
            width: widthLabel,
            max: tallest,
          })}
          data-histogram-plot
          className={cn(
            // Its own fill under the keyboard; the bar the keys are on is marked like the one pointed at.
            'relative cursor-crosshair touch-pan-y select-none focus-visible:bg-tx-0/[0.04]',
            short ? 'h-[64px]' : 'h-[88px]',
          )}
          onPointerMove={onPointerMove}
          onPointerDown={onPointerDown}
          onPointerUp={onPointerUp}
          onPointerCancel={release}
          onPointerLeave={() => {
            if (!press.current) setCursor(null);
          }}
          onKeyDown={onKeyDown}
          onBlur={() => setCursor(null)}
        >
          {counts.map((count) => (
            <span
              key={count}
              aria-hidden="true"
              // A share of the text colour is as faint on a dark surface as on a light one. The
              // line the bars stand on is the one drawn firmly.
              className={cn('absolute inset-x-0 h-px', count === 0 ? 'bg-tx-0/25' : 'bg-tx-0/10')}
              style={{ bottom: `${(count / top) * 100}%` }}
            />
          ))}
          {counting === 'counting' && tallest === 0 && (
            // The frame is known before its bars are: it waits, and is seen to.
            <span
              aria-hidden="true"
              data-histogram-waiting
              className="absolute inset-0 animate-pulse bg-tx-0/[0.04] motion-reduce:animate-none"
            />
          )}
          {cursor && pointed && (
            <span
              aria-hidden="true"
              data-histogram-cursor
              className="absolute inset-y-0 bg-tx-0/[0.06]"
              style={{ left: share(cursor.index), width: share(1) }}
            />
          )}
          {buckets.map((bucket, index) => {
            const total = bucket.ok + bucket.err;
            if (total === 0) return null;

            const errors = split ? bucket.err : 0;
            const rest = total - errors;

            return (
              <span
                key={bucket.startMicros}
                aria-hidden="true"
                data-histogram-bar={total}
                className={cn(
                  'absolute bottom-0 flex h-full flex-col-reverse px-[0.5px]',
                  // Bars of the query before stand back until their own are counted.
                  stale && 'opacity-40',
                )}
                style={{ left: share(index), width: share(1) }}
              >
                {rest > 0 && (
                  <span
                    className={cn('min-h-[2px] w-full', OK_FILL, errors === 0 && 'rounded-t-[2px]')}
                    style={{ height: `${(rest / top) * 100}%` }}
                  />
                )}
                {errors > 0 && (
                  <span
                    className={cn('min-h-[2px] w-full rounded-t-[2px]', ERROR_FILL)}
                    style={{ height: `${(errors / top) * 100}%` }}
                  />
                )}
              </span>
            );
          })}
          {selected && (
            <span
              aria-hidden="true"
              data-histogram-selection
              className="absolute inset-y-0 bg-indigo/15 shadow-[inset_1px_0_0_var(--indigo),inset_-1px_0_0_var(--indigo)]"
              style={{ left: share(selected.first), width: share(selected.last - selected.first + 1) }}
            />
          )}
        </div>
        <span />
        <div aria-hidden="true" data-histogram-axis="time" className="relative h-[20px]">
          {ticks.map((tick) => {
            const at = ((tick.micros - frame.start) / span) * 100;

            return (
              <React.Fragment key={tick.micros}>
                <span className="absolute top-0 h-[4px] w-px bg-tx-0/25" style={{ left: `${at}%` }} />
                <span
                  className={cn(
                    'type-micro absolute top-[7px] whitespace-nowrap font-mono leading-none tabular-nums',
                    tick.day ? 'font-semibold text-tx-1' : 'text-tx-3',
                    // A label at an end of the axis stays inside it.
                    at < 2 ? 'translate-x-0' : at > 98 ? '-translate-x-full' : '-translate-x-1/2',
                  )}
                  style={{ left: `${at}%` }}
                >
                  {tick.label}
                </span>
              </React.Fragment>
            );
          })}
        </div>
        <span />
        {/* What the axis leaves unsaid: the day it lies on, what the bars count, and what a bar stands for. */}
        <div
          data-histogram-frame
          data-frame-start={frame.start}
          data-frame-end={frame.end}
          data-bar-micros={binMicros}
          className="type-micro flex flex-wrap justify-between gap-x-3 gap-y-[4px] font-mono leading-none tabular-nums text-tx-3"
        >
          <span className="truncate">
            {days.map((day, index) => (
              <React.Fragment key={day}>
                {index > 0 && ' – '}
                <time>{day}</time>
              </React.Fragment>
            ))}
          </span>
          <span className="min-w-0 text-right font-sans">
            <span data-histogram-scope>{scope}</span>
            {' · '}
            {t('histogram.bar_width', { width: widthLabel })}
          </span>
        </div>
      </div>
      {pointed && cursor && (
        <HistogramReadout
          anchor={cursor.anchor}
          range={rangeOf(cursor.index, cursor.index)}
          total={pointed.ok + pointed.err}
          rows={
            split
              ? [
                  { name: labels.ok, fill: OK_FILL, count: pointed.ok },
                  { name: labels.errors, fill: ERROR_FILL, count: pointed.err },
                ]
              : []
          }
        />
      )}
      <span className="sr-only" aria-live="polite">
        {pointed && cursor?.spoken
          ? t(split ? 'histogram.bar_readout' : 'histogram.bar_readout_whole', {
              from: momentLabel(pointed.startMicros, binMicros, clock, spansDays),
              to: momentLabel(pointed.endMicros, binMicros, clock, spansDays),
              total: pointed.ok + pointed.err,
              errors: pointed.err,
            })
          : ''}
      </span>
    </div>
  );
}
