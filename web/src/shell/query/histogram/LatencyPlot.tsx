import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { formatTraceDurationNs } from '@/viz/trace/duration';

import { type AxisClock, momentLabel, tickLabelLength, timeTicks } from './axis';
import { barIndexAt, type Histogram, type HistogramLatency } from './model';
import { ASSUMED_PLOT_PIXELS } from './Plot';
import { useWidth } from './width';

export interface LatencyLabels {
  /** Names the chart for assistive technology. */
  chart: string;
  p50: string;
  p95: string;
  p99: string;
}

const LINES: ReadonlyArray<{ key: keyof HistogramLatency; color: string; label: keyof LatencyLabels }> = [
  { key: 'p50Ns', color: 'var(--chart-7)', label: 'p50' },
  { key: 'p95Ns', color: 'var(--chart-1)', label: 'p95' },
  { key: 'p99Ns', color: 'var(--chart-5)', label: 'p99' },
];

const LABEL_PIXELS_PER_CHARACTER = 6.5;
const LABEL_AIR_PIXELS = 44;

/** The marks of a duration axis: 1, 2 or 5 of a power of ten nanoseconds, a few of them to the top line. */
function durationTicks(max: number, intervals: number): number[] {
  if (!(max > 0)) return [0];

  const least = max / intervals;
  let power = 10 ** Math.floor(Math.log10(least));
  let step = power;

  for (;;) {
    const found = [1, 2, 5].map((factor) => factor * power).find((candidate) => candidate >= least);
    if (found) {
      step = found;
      break;
    }
    power *= 10;
  }

  const ticks: number[] = [];
  for (let tick = 0; tick < max + step; tick += step) ticks.push(tick);
  return ticks;
}

/**
 * How long the results of each bar took, at the median and the 95th and 99th
 * percentile, on the same bars of time as the histogram beside it: whether
 * the slow ones got slower, and when.
 */
export function LatencyPlot({
  histogram,
  clock,
  short,
  labels,
  className,
}: {
  histogram: Histogram;
  clock: AxisClock;
  short: boolean;
  labels: LatencyLabels;
  className?: string | undefined;
}) {
  const [plot, setPlot] = React.useState<HTMLDivElement | null>(null);
  const plotPixels = useWidth(plot) || ASSUMED_PLOT_PIXELS / 2;
  const [pointed, setPointed] = React.useState<number | null>(null);
  const { buckets, binMicros, frame } = histogram;
  const span = frame.end - frame.start;
  const slowest = Math.max(0, ...buckets.map((bucket) => bucket.latency?.p99Ns ?? 0));
  const ticks = durationTicks(slowest, short ? 2 : 3);
  const top = Math.max(1, ticks.at(-1) ?? 1);
  const labelsOfTicks = ticks.map((tick) => (tick === 0 ? '0' : formatTraceDurationNs(tick)));

  const timeMarks = React.useMemo(() => {
    const labelPixels = tickLabelLength(binMicros, clock) * LABEL_PIXELS_PER_CHARACTER + LABEL_AIR_PIXELS;
    return timeTicks(frame, binMicros, Math.max(2, plotPixels / labelPixels), clock);
  }, [binMicros, clock, frame, plotPixels]);

  const x = (index: number) => ((index + 0.5) / buckets.length) * 100;
  const y = (ns: number) => 100 - (ns / top) * 100;

  const paths = LINES.map((line) => {
    const segments: string[] = [];
    let open = false;

    buckets.forEach((bucket, index) => {
      const value = bucket.latency?.[line.key];
      if (value === undefined) {
        open = false;
        return;
      }
      segments.push(`${open ? 'L' : 'M'} ${x(index)} ${y(value)}`);
      open = true;
    });

    return { ...line, d: segments.join(' ') };
  });

  const bucket = pointed !== null ? buckets[pointed] : undefined;

  return (
    <div data-explorer-module="latency" className={cn('shrink-0 px-4 pb-[8px] pt-[10px]', className)}>
      <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-[8px]">
        <div
          aria-hidden="true"
          className={cn('relative', short ? 'h-[64px]' : 'h-[88px]')}
          style={{ width: `${Math.max(...labelsOfTicks.map((label) => label.length))}ch` }}
        >
          {ticks.map((tick, index) => (
            <span
              key={tick}
              className="type-micro absolute right-0 translate-y-1/2 leading-none tabular-nums text-tx-3"
              style={{ bottom: `${(tick / top) * 100}%` }}
            >
              {labelsOfTicks[index]}
            </span>
          ))}
        </div>
        <div
          ref={setPlot}
          role="img"
          aria-label={labels.chart}
          data-latency-plot
          className={cn('relative', short ? 'h-[64px]' : 'h-[88px]')}
          onPointerMove={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            setPointed(barIndexAt(event.clientX - box.left, box.width, buckets.length));
          }}
          onPointerLeave={() => setPointed(null)}
        >
          {ticks.map((tick) => (
            <span
              key={tick}
              aria-hidden="true"
              className={cn('absolute inset-x-0 h-px', tick === 0 ? 'bg-tx-0/25' : 'bg-tx-0/10')}
              style={{ bottom: `${(tick / top) * 100}%` }}
            />
          ))}
          <svg viewBox="0 0 100 100" preserveAspectRatio="none" className="absolute inset-0 h-full w-full overflow-visible">
            {paths.map((path) => (
              <path key={path.key} d={path.d} fill="none" stroke={path.color} strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
            ))}
          </svg>
          {pointed !== null && (
            <span aria-hidden="true" className="absolute inset-y-0 w-px bg-tx-0/30" style={{ left: `${x(pointed)}%` }} />
          )}
          {bucket?.latency && (
            <div className="pointer-events-none absolute right-0 top-0 z-10 rounded-md bg-bg-0 px-2 py-1 font-sans text-xs shadow-drawer">
              <div className="tabular-nums text-tx-3">{momentLabel(bucket.startTime, binMicros, clock, false)}</div>
              {LINES.map((line) => (
                <div key={line.key} className="flex items-center gap-1.5 tabular-nums">
                  <span aria-hidden="true" className="h-[2px] w-2.5 rounded-full" style={{ backgroundColor: line.color }} />
                  <span className="text-tx-2">{labels[line.label]}</span>
                  <span className="ml-auto pl-2 font-strong text-tx-0">{formatTraceDurationNs(bucket.latency![line.key])}</span>
                </div>
              ))}
            </div>
          )}
        </div>
        <span />
        <div aria-hidden="true" className="relative h-[20px]">
          {timeMarks.map((tick) => {
            const at = ((tick.micros - frame.start) / span) * 100;
            return (
              <React.Fragment key={tick.micros}>
                <span className="absolute top-0 h-[4px] w-px bg-tx-0/25" style={{ left: `${at}%` }} />
                <span
                  className={cn(
                    'type-micro absolute top-[7px] whitespace-nowrap leading-none tabular-nums',
                    tick.day ? 'font-semibold text-tx-1' : 'text-tx-3',
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
        <ul data-latency-legend className="type-micro flex flex-wrap justify-end gap-x-3 font-sans leading-none text-tx-3">
          {LINES.map((line) => (
            <li key={line.key} className="flex items-center gap-1">
              <span aria-hidden="true" className="h-[2px] w-3 rounded-full" style={{ backgroundColor: line.color }} />
              {labels[line.label]}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** Whether any bar of a histogram says how long its results took. */
export function hasLatency(histogram: Histogram): boolean {
  return histogram.buckets.some((bucket) => bucket.latency !== undefined);
}
