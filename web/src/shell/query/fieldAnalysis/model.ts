/** What a field's values are, which decides what can be said of them. */
export type FieldAnalysisKind = 'number' | 'time' | 'text' | 'bool';

/** How a number is spread. A statistic the values do not have is `null`. */
export interface FieldSpread {
  avg: number | null;
  q1: number | null;
  median: number | null;
  q3: number | null;
  stddevSamp: number | null;
  stddevPop: number | null;
  /** Of the population: the third standardised moment. */
  skewness: number | null;
  /** Of the population, in excess of a normal distribution's 3. */
  kurtosis: number | null;
}

/** The values counted into bars of one width, the first beginning at `origin`. */
export interface FieldValueBars {
  origin: number;
  width: number;
  counts: number[];
}

export interface FieldTopValue {
  value: string;
  count: number;
}

/**
 * The commonest values over time: stretches `bucketMicros` long, the first
 * beginning at `originMicros`, and for each value a count for every stretch.
 */
export interface FieldValueTrend {
  originMicros: number;
  bucketMicros: number;
  series: Array<{ value: string; counts: number[] }>;
}

/** What a field holds across the rows that were read. */
export interface FieldAnalysis {
  kind: FieldAnalysisKind;
  /** The rows that were read. */
  total: number;
  /** Those of them that have a value for the field. */
  present: number;
  distinct: number;
  /** Whether `distinct` was counted. Otherwise it is an estimate. */
  distinctExact: boolean;
  /** From what to what the values run, as written: a count of nanoseconds is past what a number holds exactly. */
  range: { min: string; max: string } | null;
  spread: FieldSpread | null;
  bars: FieldValueBars | null;
  /** The commonest values, commonest first. */
  top: FieldTopValue[];
  /** The values are nearly all different, so none is commoner than the rest. */
  topSkipped: boolean;
  /** How the commonest values are spread over time; `null` where none is commoner than the rest. */
  trend: FieldValueTrend | null;
}

/** How many of the commonest values are listed. */
export const FIELD_TOP_VALUES = 10;
/** How many of them are followed over time: as many lines as a small chart keeps apart. */
export const FIELD_TREND_VALUES = 5;
/** About how many bars a distribution is drawn in. */
const TARGET_BARS = 24;
const MAX_BARS = 64;

/**
 * The bars a number's distribution is counted into: as wide as a number a
 * reader counts in (1, 2, 5, 10 and their tenfolds), beginning on a multiple of
 * that width, so the edges of the bars are the labels of the axis. Whole
 * numbers are never counted into bars narrower than one.
 *
 * `null` where there is no distribution to draw. The backend plans the bars of
 * everything a query matches the same way.
 */
export function planValueBars(
  min: number,
  max: number,
  whole: boolean,
): { origin: number; width: number; bars: number } | null {
  if (!Number.isFinite(min) || !Number.isFinite(max) || max <= min) return null;

  const wanted = (max - min) / TARGET_BARS;
  const magnitude = 10 ** Math.floor(Math.log10(wanted));
  const steps = whole ? [1, 2, 5, 10] : [1, 2, 2.5, 5, 10];
  const round = steps.map((step) => step * magnitude).find((step) => step >= wanted) ?? 10 * magnitude;
  const width = whole ? Math.max(1, round) : round;
  const origin = Math.floor(min / width) * width;
  const bars = Math.floor((max - origin) / width) + 1;

  return Number.isFinite(origin) && bars >= 1 && bars <= MAX_BARS ? { origin, width, bars } : null;
}

const SECOND = 1_000_000;
/** The lengths a stretch of a trend can have, in seconds. */
const TREND_LENGTHS = [1, 5, 10, 15, 30, 60, 300, 600, 900, 1_800, 3_600, 7_200, 10_800, 21_600, 43_200, 86_400, 604_800];
/** About how many points a trend is drawn in. */
const TARGET_STRETCHES = 48;
const MAX_STRETCHES = 128;

/**
 * The stretches of time a trend is counted over: each a length a reader counts
 * in (a minute, five, an hour), the first beginning on a multiple of it at or
 * before `from`, covering `[from, to)`. `null` for a range that is empty or too
 * long. The backend plans the trend of everything a query matches the same way.
 */
export function planValueTrend(
  from: number,
  to: number,
): { originMicros: number; bucketMicros: number; stretches: number } | null {
  const span = to - from;
  if (!Number.isFinite(span) || span <= 0) return null;

  const bucketMicros =
    TREND_LENGTHS.map((seconds) => seconds * SECOND).find((stretch) => Math.floor((span - 1) / stretch) < TARGET_STRETCHES) ??
    TREND_LENGTHS.at(-1)! * SECOND;
  const originMicros = Math.floor(from / bucketMicros) * bucketMicros;
  const stretches = Math.floor((to - 1 - originMicros) / bucketMicros) + 1;

  return stretches >= 1 && stretches <= MAX_STRETCHES ? { originMicros, bucketMicros, stretches } : null;
}

/** The value a share of the way through `sorted`, between the two it falls between. */
export function quantile(sorted: readonly number[], share: number): number | null {
  if (sorted.length === 0) return null;

  const at = (sorted.length - 1) * share;
  const below = Math.floor(at);
  const above = Math.ceil(at);

  return sorted[below]! + (sorted[above]! - sorted[below]!) * (at - below);
}

const hasValue = (value: unknown): boolean => value !== null && value !== undefined && value !== '';

function asText(value: unknown): string {
  return typeof value === 'object' ? JSON.stringify(value) : String(value);
}

function asNumber(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null;
  if (typeof value !== 'string' || value.trim() === '') return null;

  const parsed = Number(value);

  return Number.isFinite(parsed) ? parsed : null;
}

/** A moment as microseconds since the epoch: a number is taken as it is stored, a date is read. */
function asMicros(value: unknown): number | null {
  const stored = asNumber(value);
  if (stored !== null) return stored;
  if (typeof value !== 'string') return null;

  const read = Date.parse(value);

  return Number.isNaN(read) ? null : read * 1_000;
}

function spreadOf(numbers: readonly number[]): FieldSpread {
  const count = numbers.length;
  const sorted = [...numbers].sort((left, right) => left - right);
  const avg = numbers.reduce((sum, value) => sum + value, 0) / count;
  // The moments about the mean: the second is the variance, the third and fourth its shape.
  const moment = (power: number) => numbers.reduce((sum, value) => sum + (value - avg) ** power, 0) / count;
  const variance = moment(2);
  const shaped = variance > 0;

  return {
    avg,
    q1: quantile(sorted, 0.25),
    median: quantile(sorted, 0.5),
    q3: quantile(sorted, 0.75),
    stddevSamp: count > 1 ? Math.sqrt((variance * count) / (count - 1)) : null,
    stddevPop: Math.sqrt(variance),
    skewness: shaped ? moment(3) / variance ** 1.5 : null,
    kurtosis: shaped ? moment(4) / variance ** 2 - 3 : null,
  };
}

function barsOf(numbers: readonly number[], min: number, max: number): FieldValueBars | null {
  const plan = planValueBars(min, max, numbers.every(Number.isInteger));
  if (!plan) return null;

  const counts = Array<number>(plan.bars).fill(0);

  for (const value of numbers) {
    // A value on the far edge of the last bar is counted in it.
    const index = Math.min(plan.bars - 1, Math.max(0, Math.floor((value - plan.origin) / plan.width)));
    counts[index]! += 1;
  }

  return { origin: plan.origin, width: plan.width, counts };
}

/** When the rows that are in hand happened, for the commonest values to be followed over time. */
export interface LoadedMoments {
  /** The moment of each row, in microseconds, in the order of the values; `null` for a row that has none. */
  micros: ReadonlyArray<number | null>;
  /** The stretch of time the rows were asked from. */
  range: { start: number; end: number };
}

function trendOf(
  values: readonly unknown[],
  top: readonly FieldTopValue[],
  when: LoadedMoments | undefined,
): FieldValueTrend | null {
  // Values met once each have no trend: every line would lie along the axis.
  const plan = when && (top[0]?.count ?? 0) > 1 ? planValueTrend(when.range.start, when.range.end) : null;
  if (!when || !plan) return null;

  const series = top.slice(0, FIELD_TREND_VALUES).map(({ value }) => ({
    value,
    counts: Array<number>(plan.stretches).fill(0),
  }));
  const followed = new Map(series.map((item) => [item.value, item.counts]));

  values.forEach((value, index) => {
    const micros = when.micros[index];
    const counts = hasValue(value) ? followed.get(asText(value)) : undefined;
    if (!counts || micros === null || micros === undefined) return;

    const stretch = Math.floor((micros - plan.originMicros) / plan.bucketMicros);
    if (stretch >= 0 && stretch < plan.stretches) counts[stretch]! += 1;
  });

  return { originMicros: plan.originMicros, bucketMicros: plan.bucketMicros, series };
}

/**
 * What a field holds across rows that are in hand, worked out here: the
 * analysis of the loaded results, for when everything a query matches cannot
 * be read. An empty string is no value, as in the field list. With `when`,
 * the commonest values are also followed over time.
 */
export function analyzeLoadedValues(
  values: readonly unknown[],
  kind: FieldAnalysisKind,
  when?: LoadedMoments,
): FieldAnalysis {
  const present = values.filter(hasValue);
  const counts = new Map<string, number>();

  for (const value of present) {
    const text = asText(value);
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }

  // Equally common values keep the order they were met in.
  const ranked = [...counts.entries()].sort((left, right) => right[1] - left[1]);
  const top = ranked.slice(0, FIELD_TOP_VALUES).map(([value, count]) => ({ value, count }));
  const analysis: FieldAnalysis = {
    kind,
    total: values.length,
    present: present.length,
    distinct: counts.size,
    distinctExact: true,
    range: null,
    spread: null,
    bars: null,
    top,
    topSkipped: false,
    // A moment is not followed over time: its values are the time.
    trend: kind === 'time' ? null : trendOf(values, top, when),
  };

  if (kind === 'time') {
    const moments = present.map(asMicros).filter((micros): micros is number => micros !== null);
    if (moments.length === 0) return analysis;

    return { ...analysis, range: { min: String(Math.min(...moments)), max: String(Math.max(...moments)) } };
  }
  if (kind !== 'number') return analysis;

  const numbers = present.map(asNumber).filter((number): number is number => number !== null);
  if (numbers.length === 0) return analysis;

  const min = numbers.reduce((least, value) => Math.min(least, value), Infinity);
  const max = numbers.reduce((most, value) => Math.max(most, value), -Infinity);

  return {
    ...analysis,
    range: { min: String(min), max: String(max) },
    spread: spreadOf(numbers),
    bars: barsOf(numbers, min, max),
  };
}

/** Whether the commonest values say anything: a list of values each met once does not. */
export function hasCommonValues(analysis: FieldAnalysis): boolean {
  return !analysis.topSkipped && (analysis.top[0]?.count ?? 0) > 1;
}

const STATISTIC = new Intl.NumberFormat(undefined, { maximumSignificantDigits: 6 });

/** A statistic as a reader takes it in: six figures that mean something, no more. */
export function formatStatistic(value: number): string {
  return STATISTIC.format(value);
}

const SHAPE = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });

/** A skewness or a kurtosis: read by its sign and its size, not to its last figure. */
export function formatShape(value: number): string {
  return SHAPE.format(value);
}

const SHARE = new Intl.NumberFormat(undefined, { style: 'percent', maximumFractionDigits: 1 });

/** A part of a whole, as a percentage. */
export function formatShare(part: number, whole: number): string {
  return SHARE.format(whole > 0 ? part / whole : 0);
}

/**
 * A value that was kept as text so that nothing of it is lost. A whole number
 * a double holds exactly has its digits grouped, all of them; a fraction is
 * cut to the figures that mean something; anything else stays as written.
 */
export function formatWrittenNumber(value: string): string {
  const number = Number(value);
  if (value.trim() === '' || !Number.isFinite(number)) return value;
  if (!Number.isInteger(number)) return STATISTIC.format(number);

  return Number.isSafeInteger(number) ? number.toLocaleString() : value;
}
