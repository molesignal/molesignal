export interface HistogramRange {
  /** Microseconds since the epoch. */
  start: number;
  end: number;
}

export interface HistogramBucket {
  ok: number;
  err: number;
  startMicros: number;
  endMicros: number;
}

export interface HistogramPoint {
  /** When the event happened, in microseconds; `null` when it carries no usable time. */
  micros: number | null;
  error: boolean;
}

export interface Histogram {
  buckets: HistogramBucket[];
  /** What one bar stands for, in microseconds. */
  binMicros: number;
  /** What the bars span: the range that was given, widened to whole bars. */
  frame: HistogramRange;
}

/** The bars a range is drawn in, before anything is counted into them. */
export interface HistogramPlan {
  /** What one bar stands for, in microseconds. */
  binMicros: number;
  /** The range widened to whole bars: the first bar begins where it begins. */
  frame: HistogramRange;
}

/** What one bar holds, counted somewhere else. */
export interface HistogramCount {
  /** Where the bar begins, in microseconds. */
  startMicros: number;
  total: number;
  errors: number;
}

const MILLISECOND = 1_000;
const SECOND = 1_000 * MILLISECOND;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
export const DAY_MICROS = 24 * HOUR;

/**
 * The lengths of time a bar, or the step between two axis labels, can stand
 * for: the ones a reader counts in, from a millisecond to a year.
 */
export const TIME_STEPS: readonly number[] = [
  ...[1, 2, 5, 10, 20, 50, 100, 200, 500].map((count) => count * MILLISECOND),
  ...[1, 2, 5, 10, 15, 30].map((count) => count * SECOND),
  ...[1, 2, 5, 10, 15, 30].map((count) => count * MINUTE),
  ...[1, 2, 3, 6, 12].map((count) => count * HOUR),
  ...[1, 2, 7, 14, 30, 90, 365].map((count) => count * DAY_MICROS),
];

/** A margin of this share of the results' own span is left at either end, so the outer bars are whole. */
const FIT_MARGIN = 0.04;
/** One event, or several at one instant, still get a second of frame to stand in. */
const MIN_FIT_SPAN_MICROS = SECOND;

/**
 * The stretch of time a histogram of loaded results should span.
 *
 * An explorer shows a page of results, not everything in the queried range. A
 * page that sits in a sliver of it (the newest twenty traces of a day) drawn
 * across the whole range collapses into a single bar at one edge. So the frame
 * is the results themselves, with a margin, kept inside the range that was
 * asked for. Results that carry no time leave the range as it is.
 */
export function fitHistogramRange(
  points: readonly HistogramPoint[],
  range: HistogramRange | null,
): HistogramRange | null {
  const times = timesOf(points);
  if (times.length === 0) return range;

  const first = Math.min(...times);
  const last = Math.max(...times);
  const span = Math.max(last - first, MIN_FIT_SPAN_MICROS);
  const middle = (first + last) / 2;
  const reach = span / 2 + span * FIT_MARGIN;
  const asked = range !== null && range.end > range.start ? range : null;
  const start = Math.max(Math.floor(middle - reach), asked?.start ?? -Infinity);
  const end = Math.min(Math.ceil(middle + reach), asked?.end ?? Infinity);

  // Results that all lie outside the range asked for have no frame inside it.
  return end > start ? { start, end } : range;
}

/** The shortest listed step that covers `span` in no more than `maxBins` bars. */
export function binWidthFor(span: number, maxBins: number): number {
  const bins = Math.max(1, Math.floor(maxBins));
  const listed = TIME_STEPS.find((step) => Math.ceil(span / step) <= bins);
  if (listed !== undefined) return listed;

  // Longer than the list reaches: whole years to a bar.
  const year = TIME_STEPS.at(-1)!;

  return year * Math.ceil(span / year / bins);
}

/**
 * Counts the events that carry a time into bars of one length, errors apart
 * from the rest.
 *
 * A bar stands for a length of time a reader counts in (a second, five
 * minutes), and begins on a multiple of it on the clock the reader sees:
 * `utcOffsetMicros` is how far that clock is ahead of UTC. So the frame is the
 * range widened to whole bars, and a label on the axis falls on the edge of a
 * bar. An event outside the range is counted in the bar at that end of it.
 *
 * With no event that carries a time there is nothing to draw across time.
 */
export function binHistogram(
  points: readonly HistogramPoint[],
  range: HistogramRange | null,
  { maxBins, utcOffsetMicros = 0 }: { maxBins: number; utcOffsetMicros?: number },
): Histogram | null {
  const times = timesOf(points);
  if (times.length === 0) return null;

  const asked =
    range !== null && range.end > range.start
      ? range
      : { start: Math.min(...times), end: Math.max(...times) + 1 };
  const plan = planHistogram(asked, { maxBins, utcOffsetMicros });
  const buckets = emptyBuckets(plan);
  const last = buckets.length - 1;

  for (const point of points) {
    if (point.micros == null) continue;

    const index = Math.max(0, Math.min(last, Math.floor((point.micros - plan.frame.start) / plan.binMicros)));
    if (point.error) buckets[index]!.err += 1;
    else buckets[index]!.ok += 1;
  }

  return { buckets, ...plan };
}

/**
 * The bars `range` is drawn in: a length of time a reader counts in, short
 * enough to show a shape and long enough that no more than `maxBins` fit, each
 * beginning on a multiple of it on the reader's clock. `utcOffsetMicros` is
 * how far that clock is ahead of UTC.
 *
 * It is settled before anything is counted, so that counts can be asked for
 * exactly these bars.
 */
export function planHistogram(
  range: HistogramRange,
  { maxBins, utcOffsetMicros = 0 }: { maxBins: number; utcOffsetMicros?: number },
): HistogramPlan {
  const binMicros = binWidthFor(range.end - range.start, maxBins);
  const start = alignDown(range.start, binMicros, utcOffsetMicros);
  const end = Math.max(alignUp(range.end, binMicros, utcOffsetMicros), start + binMicros);

  return { binMicros, frame: { start, end } };
}

/**
 * The bars of `plan` filled with counts that were made for it, errors apart
 * from the rest. A count that begins on none of the bars is left out: it was
 * made for other bars.
 */
export function histogramFromCounts(plan: HistogramPlan, counts: readonly HistogramCount[]): Histogram {
  const buckets = emptyBuckets(plan);

  for (const count of counts) {
    const index = (count.startMicros - plan.frame.start) / plan.binMicros;
    const bucket = Number.isInteger(index) ? buckets[index] : undefined;
    if (!bucket) continue;

    const total = Math.max(0, count.total);
    const errors = Math.max(0, Math.min(count.errors, total));
    bucket.err += errors;
    bucket.ok += total - errors;
  }

  return { buckets, ...plan };
}

function emptyBuckets({ binMicros, frame }: HistogramPlan): HistogramBucket[] {
  const count = Math.round((frame.end - frame.start) / binMicros);

  return Array.from({ length: count }, (_, index) => ({
    ok: 0,
    err: 0,
    startMicros: frame.start + binMicros * index,
    endMicros: frame.start + binMicros * (index + 1),
  }));
}

/** The last multiple of `step` on the reader's clock that is not after `micros`. */
export function alignDown(micros: number, step: number, utcOffsetMicros = 0): number {
  return Math.floor((micros + utcOffsetMicros) / step) * step - utcOffsetMicros;
}

/** The first multiple of `step` on the reader's clock that is not before `micros`. */
export function alignUp(micros: number, step: number, utcOffsetMicros = 0): number {
  return Math.ceil((micros + utcOffsetMicros) / step) * step - utcOffsetMicros;
}

/** Which of `count` bars side by side across `width` a point `x` along them falls on. */
export function barIndexAt(x: number, width: number, count: number): number {
  if (count <= 0 || !(width > 0)) return 0;

  return Math.max(0, Math.min(count - 1, Math.floor((x / width) * count)));
}

function timesOf(points: readonly HistogramPoint[]): number[] {
  return points.map((point) => point.micros).filter((value): value is number => value != null);
}
