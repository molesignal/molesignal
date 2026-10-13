import dayjs from 'dayjs';

import { DATE_PATTERNS, type DateFormat, type TimeFormat } from '@/lib/time';

import { alignUp, DAY_MICROS, type HistogramRange, TIME_STEPS } from './model';

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;

/** How the reader has asked to see time. */
export interface AxisClock {
  /** IANA name of the zone. */
  tz: string;
  format: TimeFormat;
  dateFormat: DateFormat;
}

export interface TimeTick {
  micros: number;
  label: string;
  /** The label is a day: the tick is where one begins, or the axis counts in days. */
  day: boolean;
}

export type BinUnit = 'ms' | 's' | 'min' | 'h' | 'd';

/** How far the reader's clock is ahead of UTC at `micros`. */
export function utcOffsetMicros(micros: number, tz: string): number {
  return dayjs(micros / 1_000).tz(tz).utcOffset() * MINUTE;
}

/**
 * The marks of a count axis: whole numbers from zero, a step a reader counts
 * in (1, 2, 5, 10, …), few enough to fit, the last at or above `max` so no bar
 * runs past the top line.
 */
export function countTicks(max: number, maxIntervals: number): number[] {
  const top = Math.max(1, Math.ceil(max));
  const intervals = Math.max(1, Math.floor(maxIntervals));
  let step = 1;

  for (let magnitude = 1; Math.ceil(top / step) > intervals; magnitude *= 10) {
    step =
      [1, 2, 5, 10].map((count) => count * magnitude).find((value) => Math.ceil(top / value) <= intervals) ??
      10 * magnitude;
  }

  return Array.from({ length: Math.ceil(top / step) + 1 }, (_, index) => index * step);
}

/**
 * The step between two labels of the time axis: a listed length of time that
 * is a whole number of bars, so a label falls on the edge of a bar, and long
 * enough that no more than `maxLabels` fit across the frame.
 */
export function tickStepFor(frame: HistogramRange, binMicros: number, maxLabels: number): number {
  const span = frame.end - frame.start;
  const labels = Math.max(1, Math.floor(maxLabels));
  const fits = (step: number) => Math.floor(span / step) + 1 <= labels;
  const listed = TIME_STEPS.find((step) => step >= binMicros && step % binMicros === 0 && fits(step));
  if (listed !== undefined) return listed;

  // No listed step is both a whole number of bars and long enough: count in bars.
  return binMicros * Math.max(1, Math.ceil(span / binMicros / Math.max(1, labels - 1)));
}

/** The labelled marks of the time axis across `frame`, on the reader's clock. */
export function timeTicks(
  frame: HistogramRange,
  binMicros: number,
  maxLabels: number,
  clock: AxisClock,
): TimeTick[] {
  const step = tickStepFor(frame, binMicros, maxLabels);
  const offset = utcOffsetMicros(frame.start, clock.tz);
  const ticks: TimeTick[] = [];

  for (let micros = alignUp(frame.start, step, offset); micros <= frame.end; micros += step) {
    ticks.push({ micros, ...tickLabel(micros, step, clock, ticks.length === 0) });
  }

  return ticks;
}

/**
 * What a mark of the time axis says: the time of day, as fine as the step
 * needs; the day where one begins, and where the axis counts in days.
 *
 * Where the axis steps by less than a second, only the fraction changes from
 * mark to mark: a mark on a second says the second, the `leading` mark says
 * the time in full, and the others say the fraction alone.
 */
export function tickLabel(
  micros: number,
  step: number,
  clock: AxisClock,
  leading = true,
): Pick<TimeTick, 'label' | 'day'> {
  const moment = dayjs(micros / 1_000).tz(clock.tz);
  const onSecond = moment.millisecond() === 0;
  const startsDay = moment.hour() === 0 && moment.minute() === 0 && moment.second() === 0 && onSecond;

  if (step >= DAY_MICROS || startsDay) return { label: moment.format(dayOfYearPattern(clock.dateFormat)), day: true };
  if (step < SECOND && onSecond) return { label: moment.format(timeOfDayPattern(SECOND, clock.format)), day: false };
  if (step < SECOND && !leading) return { label: moment.format('.SSS'), day: false };

  return { label: moment.format(timeOfDayPattern(step, clock.format)), day: false };
}

/** The day, or the days, the frame lies on, written as the reader writes dates. */
export function frameDays(frame: HistogramRange, clock: AxisClock): string[] {
  const pattern = DATE_PATTERNS[clock.dateFormat];
  const first = dayjs(frame.start / 1_000).tz(clock.tz).format(pattern);
  // The frame ends where its last bar does: that instant belongs to the next bar.
  const last = dayjs((frame.end - 1) / 1_000).tz(clock.tz).format(pattern);

  return first === last ? [first] : [first, last];
}

/**
 * An edge of a bar in words: the time of day as fine as the bars are, with the
 * day when `withDay`; the day alone where a bar is a day or more.
 */
export function momentLabel(micros: number, step: number, clock: AxisClock, withDay: boolean): string {
  const moment = dayjs(micros / 1_000).tz(clock.tz);
  const day = moment.format(DATE_PATTERNS[clock.dateFormat]);
  if (step >= DAY_MICROS) return day;

  const time = moment.format(timeOfDayPattern(step, clock.format));

  return withDay ? `${day} ${time}` : time;
}

/** The length of a bar in the largest unit it is a whole number of. */
export function binWidthParts(binMicros: number): { value: number; unit: BinUnit } {
  const units: [number, BinUnit][] = [
    [DAY_MICROS, 'd'],
    [HOUR, 'h'],
    [MINUTE, 'min'],
    [SECOND, 's'],
  ];
  const [size, unit] = units.find(([length]) => binMicros >= length && binMicros % length === 0) ?? [1_000, 'ms'];

  return { value: binMicros / size, unit };
}

/** How many characters a label of the time axis takes when the axis steps by `step`. */
export function tickLabelLength(step: number, clock: AxisClock): number {
  const pattern =
    step >= DAY_MICROS
      ? dayOfYearPattern(clock.dateFormat)
      : // Under a second most marks say less than the time in full; see `tickLabel`.
        timeOfDayPattern(Math.max(step, SECOND), clock.format);

  // "A" is written AM or PM.
  return pattern.length + (pattern.endsWith(' A') ? 1 : 0);
}

function timeOfDayPattern(step: number, format: TimeFormat): string {
  const fineness = step < SECOND ? ':ss.SSS' : step < MINUTE ? ':ss' : '';

  return format === 'local_12h' ? `hh:mm${fineness} A` : `HH:mm${fineness}`;
}

/** A day without its year: the axis is of days close together, and the year is said beside it. */
function dayOfYearPattern(dateFormat: DateFormat): string {
  return DATE_PATTERNS[dateFormat].replace(/[-/]?YYYY[-/]?/, '');
}
