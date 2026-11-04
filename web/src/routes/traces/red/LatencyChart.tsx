import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { countTicks } from '@/shell/query/histogram/axis';
import { barIndexAt } from '@/shell/query/histogram/model';
import { HistogramReadout, type ReadoutAnchor } from '@/shell/query/histogram/Readout';
import { formatCompactNumber } from '@/viz/timeseries/formatters';

import { formatDuration, formatDurationEdge } from './format';
import { LATENCY_DECADES_NS, type LatencySlot } from './model';

/** Shorter than this, a press is a click: it picks the bar it was on. */
const MIN_DRAG_PIXELS = 4;
const OK_FILL = 'bg-blue-soft';
// Failures are red throughout the view; orange is the 95th percentile's.
const ERROR_FILL = 'bg-red-soft';

interface Cursor {
  index: number;
  anchor: ReadoutAnchor;
  /** Moved with the keyboard: what the bar holds is also read out. */
  spoken: boolean;
}

interface LatencyChartProps {
  slots: LatencySlot[];
  /** Whether failures are drawn apart from the rest. */
  split: boolean;
  /** Where the slowest twentieth and hundredth begin, in nanoseconds. */
  p95: number | null;
  p99: number | null;
  labels: { chart: string; ok: string; errors: string };
  /** A stretch of durations was picked: from where, up to where (no end: longer). */
  onSelect: (fromNs: number, toNs: number | null) => void;
  className?: string | undefined;
}

/** Where a duration stands along the axis, as a share of it: bars are of equal width on a scale of 1, 2, 5. */
function shareOf(ns: number, slots: readonly LatencySlot[]): number | null {
  const index = slots.findIndex((slot) => ns >= slot.fromNs && (slot.toNs === null || ns < slot.toNs));
  if (index < 0) return null;

  const slot = slots[index]!;
  const within =
    slot.toNs === null || slot.fromNs <= 0
      ? 0.5
      : (Math.log(ns) - Math.log(slot.fromNs)) / (Math.log(slot.toNs) - Math.log(slot.fromNs));

  return (index + Math.max(0, Math.min(1, within))) / slots.length;
}

/** Two marks closer than this share a label. */
const SHARED_LABEL = 0.12;
/** A label this near an end of the axis is set inward. */
const NEAR_END = 0.15;

/**
 * Where the names of the percentile marks go: each beside its line, the 95th
 * before and the 99th after so they part; one label for both where the lines
 * are close; and inward of a line near an end of the axis.
 */
function markLabels(markers: ReadonlyArray<{ key: string; at: number }>) {
  const close =
    markers.length === 2 && Math.abs(markers[0]!.at - markers[1]!.at) < SHARED_LABEL;
  const labels = close
    ? [{ text: markers.map((marker) => marker.key).join(' · '), at: (markers[0]!.at + markers[1]!.at) / 2, side: 'middle' }]
    : markers.map((marker) => ({ text: marker.key, at: marker.at, side: marker.key === 'p95' ? 'before' : 'after' }));

  return labels.map((label) => ({
    ...label,
    side: label.at > 1 - NEAR_END ? 'before' : label.at < NEAR_END ? 'after' : label.side,
  }));
}

/**
 * How long the requests took: a bar for each stretch of durations on a scale
 * of 1, 2 and 5, the failures stacked on the rest, the 95th and 99th
 * percentile marked. Pointing at a bar reads it out; dragging across bars, or
 * pressing one, narrows the query to the requests that took that long.
 */
export function LatencyChart({ slots, split, p95, p99, labels, onSelect, className }: LatencyChartProps) {
  const { t } = useTranslation('traces');
  const moduleRef = React.useRef<HTMLDivElement | null>(null);
  const [plot, setPlot] = React.useState<HTMLDivElement | null>(null);
  const [cursor, setCursor] = React.useState<Cursor | null>(null);
  const [selection, setSelection] = React.useState<{ from: number; to: number } | null>(null);
  const press = React.useRef<{ x: number; index: number } | null>(null);

  const totals = slots.map((slot) => slot.ok + slot.errors);
  const tallest = Math.max(0, ...totals);
  const counts = tallest > 0 ? countTicks(tallest, 3) : [0];
  const top = Math.max(1, counts.at(-1) ?? 1);
  const countLabels = counts.map((count) => formatCompactNumber(count));
  const share = (count: number) => `${(count / Math.max(1, slots.length)) * 100}%`;
  const rangeOf = (from: number, to: number) => {
    const end = slots[to]?.toNs;

    return `${formatDurationEdge(slots[from]?.fromNs ?? 0)} – ${end === null || end === undefined ? '∞' : formatDurationEdge(end)}`;
  };
  // The axis is labelled where a power of ten begins.
  const edges = slots
    .map((slot, index) => ({ index, ns: slot.fromNs }))
    .filter(({ ns }) => LATENCY_DECADES_NS.has(ns));
  const markers = [
    { key: 'p95', ns: p95 },
    { key: 'p99', ns: p99 },
  ].flatMap(({ key, ns }) => {
    const at = ns === null ? null : shareOf(ns, slots);

    return at === null ? [] : [{ key, at }];
  });
  const marks = markLabels(markers);

  const cursorAt = (index: number, spoken: boolean): Cursor | null => {
    const plotBox = plot?.getBoundingClientRect();
    const moduleBox = moduleRef.current?.getBoundingClientRect();
    if (!plotBox || !moduleBox) return null;

    return {
      index,
      spoken,
      anchor: {
        x: plotBox.left + ((index + 0.5) / slots.length) * plotBox.width,
        above: plotBox.top,
        below: moduleBox.bottom,
      },
    };
  };
  const indexUnder = (clientX: number) => {
    const box = plot?.getBoundingClientRect();

    return box ? barIndexAt(clientX - box.left, box.width, slots.length) : 0;
  };
  const select = (from: number, to: number) => {
    const first = slots[Math.min(from, to)];
    const last = slots[Math.max(from, to)];

    if (first && last) onSelect(first.fromNs, last.toNs);
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
    else if (press.current && totals[press.current.index]) select(press.current.index, press.current.index);
    release();
  };
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      release();
      setCursor(null);
      return;
    }

    const last = slots.length - 1;
    const step = event.key === 'ArrowLeft' ? -1 : event.key === 'ArrowRight' ? 1 : 0;
    const moved =
      step !== 0 ? (cursor ? Math.max(0, Math.min(last, cursor.index + step)) : 0)
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

  const pointed = cursor ? slots[cursor.index] : undefined;
  const selected = selection
    ? { first: Math.min(selection.from, selection.to), last: Math.max(selection.from, selection.to) }
    : null;

  return (
    <div ref={moduleRef} data-red-latency className={cn('pt-[14px]', className)}>
      <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-[8px]">
        <div
          aria-hidden="true"
          className="relative h-[96px]"
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
          aria-label={t('explore.red.duration.distribution_aria', { name: labels.chart, max: tallest })}
          data-red-latency-plot
          className="relative h-[96px] cursor-crosshair touch-pan-y select-none focus-visible:bg-tx-0/[0.04]"
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
              className={cn('absolute inset-x-0 h-px', count === 0 ? 'bg-tx-0/25' : 'bg-tx-0/10')}
              style={{ bottom: `${(count / top) * 100}%` }}
            />
          ))}
          {cursor && pointed && (
            <span
              aria-hidden="true"
              className="absolute inset-y-0 bg-tx-0/[0.06]"
              style={{ left: share(cursor.index), width: share(1) }}
            />
          )}
          {slots.map((slot, index) => {
            const errors = split ? slot.errors : 0;
            const rest = slot.ok + slot.errors - errors;
            if (rest + errors === 0) return null;

            return (
              <span
                key={slot.fromNs}
                aria-hidden="true"
                data-red-latency-bar={rest + errors}
                className="absolute bottom-0 flex h-full flex-col-reverse px-[0.5px]"
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
          {markers.map((marker) => (
            <span
              key={marker.key}
              aria-hidden="true"
              data-red-latency-marker={marker.key}
              className="absolute inset-y-0 w-0 border-l border-dashed border-tx-2/60"
              style={{ left: `${marker.at * 100}%` }}
            />
          ))}
          {marks.map((mark) => (
            <span
              key={mark.text}
              aria-hidden="true"
              data-red-latency-mark
              className={cn(
                'type-micro absolute -top-[13px] whitespace-nowrap font-mono leading-none text-tx-2',
                mark.side === 'before' ? '-translate-x-full pr-[3px]' : mark.side === 'after' ? 'pl-[3px]' : '-translate-x-1/2',
              )}
              style={{ left: `${mark.at * 100}%` }}
            >
              {mark.text}
            </span>
          ))}
          {selected && (
            <span
              aria-hidden="true"
              className="absolute inset-y-0 bg-indigo/15 shadow-[inset_1px_0_0_var(--indigo),inset_-1px_0_0_var(--indigo)]"
              style={{ left: share(selected.first), width: share(selected.last - selected.first + 1) }}
            />
          )}
        </div>
        <span />
        <div aria-hidden="true" className="relative h-[20px]">
          {edges.map(({ index, ns }) => {
            const at = (index / slots.length) * 100;

            return (
              <React.Fragment key={ns}>
                <span className="absolute top-0 h-[4px] w-px bg-tx-0/25" style={{ left: `${at}%` }} />
                <span
                  className={cn(
                    'type-micro absolute top-[7px] whitespace-nowrap font-mono leading-none tabular-nums text-tx-3',
                    at < 2 ? 'translate-x-0' : at > 98 ? '-translate-x-full' : '-translate-x-1/2',
                  )}
                  style={{ left: `${at}%` }}
                >
                  {ns === 0 ? '0' : formatDurationEdge(ns)}
                </span>
              </React.Fragment>
            );
          })}
        </div>
      </div>
      {pointed && cursor && (
        <HistogramReadout
          anchor={cursor.anchor}
          range={rangeOf(cursor.index, cursor.index)}
          total={pointed.ok + pointed.errors}
          rows={
            split
              ? [
                  { name: labels.ok, fill: OK_FILL, count: pointed.ok },
                  { name: labels.errors, fill: ERROR_FILL, count: pointed.errors },
                ]
              : []
          }
        />
      )}
      <span className="sr-only" aria-live="polite">
        {pointed && cursor?.spoken
          ? t('explore.red.duration.bar_readout', {
              range: rangeOf(cursor.index, cursor.index),
              total: pointed.ok + pointed.errors,
              errors: pointed.errors,
            })
          : ''}
      </span>
      {(p95 !== null || p99 !== null) && (
        <span className="sr-only">
          {t('explore.red.duration.percentiles_aria', { p95: formatDuration(p95), p99: formatDuration(p99) })}
        </span>
      )}
    </div>
  );
}
