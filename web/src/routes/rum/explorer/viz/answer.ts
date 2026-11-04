import type { RumAggregate, RumUnit } from '@/api/rum';
import type { TimeSeriesSeries, TimeSeriesThreshold } from '@/viz/timeseries/types';

import { COUNTRY_FIELD, isAdditive, type RumMeasureChoice, type RumVizBars } from './spec';
import { formatDurationMs, formatDurationNs, formatShare, formatSize, regionName } from '../../_helpers';
import { VITALS } from '../../overview/model';

/** A group the rows have no value in. */
export type GroupValue = string | null;

/** What a group is called: a country by its name in the language of the page, any other value as it is. */
export function groupLabel(field: string, value: GroupValue, language: string, none: string): string {
  if (value === null || value === '') return none;

  return field === COUNTRY_FIELD ? regionName(value, language) : value;
}

/** The groups of a row told as one: `Chrome · macOS`. */
export function groupsLabel(fields: readonly string[], group: readonly GroupValue[], language: string, none: string): string {
  return fields.map((field, index) => groupLabel(field, group[index] ?? null, language, none)).join(' · ');
}

/** A number of an answer in the unit it is measured in. A value the reader may not see comes as the text it is masked to. */
export function formatMeasure(value: number | string | null | undefined, unit: RumUnit, language: string): string {
  if (value === null || value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (!Number.isFinite(value)) return '—';

  switch (unit) {
    case 'ns':
      return formatDurationNs(value);
    case 'ms':
      return formatDurationMs(value);
    case 'bytes':
      return formatSize(value);
    case 'percent':
      return formatShare(value / 100);
    case 'score':
      return String(Number(value.toFixed(3)));
    case 'px':
      return `${Number(value.toFixed(1))} px`;
    case 'per_second':
      return `${Number(value.toFixed(2))}/s`;
    default:
      return value.toLocaleString(language, { maximumFractionDigits: 2 });
  }
}

/** The unit as the shared chart writes it: a count and a score have none to say. */
const CHART_UNIT: Partial<Record<RumUnit, string>> = { ns: 'ns', ms: 'ms', bytes: 'bytes', percent: 'percent', px: 'px', per_second: '/s' };

export const chartUnit = (unit: RumUnit): string | undefined => CHART_UNIT[unit];

/** The number of a value, or none: a masked value is text and a bar with no rows has none. */
const numberOf = (value: number | string | null | undefined): number | null =>
  typeof value === 'number' && Number.isFinite(value) ? value : null;

/** What a vital is drawn in, so the lines where it turns poor read as its thresholds are given: 2.5 s, 200 ms. */
export interface Rescale {
  divisor: number;
  unit: string;
}

/** The vital whose level the measure is, if it is one: a count or a sum of its field is not a level. */
const vitalOf = (measure: RumMeasureChoice) =>
  isAdditive(measure.fn) ? undefined : VITALS.find((candidate) => candidate.field === measure.field);

/** How a level of a vital that the SDK sends in nanoseconds is drawn: in seconds from two and a half of them, else in milliseconds. */
export function vitalRescale(measure: RumMeasureChoice): Rescale | null {
  const vital = vitalOf(measure);

  if (!vital?.nanoseconds) return null;

  return vital.good >= 1_000 ? { divisor: 1_000_000_000, unit: 's' } : { divisor: 1_000_000, unit: 'ms' };
}

export interface SeriesInputs {
  bars: RumVizBars;
  measures: readonly RumMeasureChoice[];
  /** The fields the rows are grouped by. */
  groups: readonly string[];
  language: string;
  none: string;
  /** What a measure is called when nothing is grouped. */
  measureLabel: (measure: RumMeasureChoice) => string;
  /** The unit the numbers are drawn in when it is not the one they are stored in. */
  rescale?: Rescale | null;
}

/**
 * The lines or bars of a chart over time: one for each group the server followed, in the order of
 * what they came to, over every bar of the frame. A bar with no rows is nothing counted for a count
 * or a sum and a gap for the rest, which have no value there.
 */
export function chartSeries(answer: RumAggregate, inputs: SeriesInputs): TimeSeriesSeries[] {
  const { bars, measures, groups } = inputs;
  const count = Math.max(1, Math.round((bars.frame.end - bars.frame.start) / bars.binMicros));
  const starts = Array.from({ length: count }, (_, index) => bars.frame.start + index * bars.binMicros);

  const found = new Map<string, { group: GroupValue[]; values: Array<Array<number | null | undefined>> }>();

  for (const row of answer.rows) {
    if (row.startTime === undefined) continue;

    const index = Math.round((row.startTime - bars.frame.start) / bars.binMicros);
    if (index < 0 || index >= count) continue;

    const key = JSON.stringify(row.group);
    const entry = found.get(key) ?? {
      group: row.group,
      values: answer.measures.map(() => new Array<number | null | undefined>(count).fill(undefined)),
    };

    row.values.forEach((value, position) => {
      const line = entry.values[position];
      if (line) line[index] = numberOf(value);
    });
    found.set(key, entry);
  }

  const first = measures[0];
  const standing = (values: ReadonlyArray<number | null | undefined> | undefined) => {
    const numbers = (values ?? []).filter((value): value is number => typeof value === 'number');
    const sum = numbers.reduce((total, value) => total + value, 0);

    return first && isAdditive(first.fn) ? sum : numbers.length > 0 ? sum / numbers.length : -Infinity;
  };

  const ranked = [...found.entries()]
    .map(([key, entry]) => ({ key, ...entry, label: groupsLabel(groups, entry.group, inputs.language, inputs.none) }))
    .sort((a, b) => standing(b.values[0]) - standing(a.values[0]) || a.label.localeCompare(b.label));

  return ranked.flatMap((entry) =>
    answer.measures.map((measure, position) => {
      const choice = measures[position];
      // A bar the server has no row for is nothing counted for a count; a masked value stays a gap.
      const filled = (entry.values[position] ?? []).map((value) =>
        value === undefined ? (choice && isAdditive(choice.fn) ? 0 : null) : value,
      );
      const unit = inputs.rescale?.unit ?? chartUnit(measure.unit);
      const labels = Object.fromEntries(
        groups.flatMap((field, index) => {
          const value = entry.group[index];
          return value === null || value === undefined || value === '' ? [] : [[field, value] as const];
        }),
      );

      return {
        id: `${measure.name}|${entry.key}`,
        name: groups.length > 0 ? entry.label : choice ? inputs.measureLabel(choice) : measure.name,
        data: inputs.rescale ? filled.map((value) => (value === null ? null : value / inputs.rescale!.divisor)) : filled,
        timestamps: starts,
        ...(Object.keys(labels).length > 0 ? { labels } : {}),
        ...(unit ? { unit } : {}),
      };
    }),
  );
}

/** How many groups the lines or bars of a chart stand for. */
export function seriesGroups(answer: RumAggregate): number {
  return new Set(answer.rows.map((row) => JSON.stringify(row.group))).size;
}

/**
 * The lines where a vital turns from good to poor, when the numbers drawn are of one: the grade of
 * a page is read against them. They are in the unit the numbers are drawn in, as `vitalRescale`
 * says it.
 */
export function vitalThresholds(
  measure: RumMeasureChoice,
  labels: { good: string; poor: string },
): TimeSeriesThreshold[] {
  const vital = vitalOf(measure);

  if (!vital) return [];

  // The thresholds are written in milliseconds for the vitals the SDK measures in nanoseconds.
  const scale = vital.nanoseconds ? 1_000_000 / (vitalRescale(measure)?.divisor ?? 1) : 1;

  return [
    { value: vital.good * scale, label: labels.good, color: 'var(--green)', showLine: true },
    { value: vital.poor * scale, label: labels.poor, color: 'var(--red)', showLine: true },
  ];
}

/** A row of a ranking or a table: its groups, and its numbers in the order of the measures. */
export interface RankedRow {
  key: string;
  group: GroupValue[];
  values: Array<number | string | null>;
}

export function rankedRows(answer: RumAggregate): RankedRow[] {
  return answer.rows.map((row, index) => ({
    key: `${JSON.stringify(row.group)}:${index}`,
    group: row.group,
    values: row.values,
  }));
}

/** A country with the number it is drawn by. */
export interface CountryValue {
  code: string;
  value: number;
}

/** The countries of an answer grouped by country, with something to draw: a row with no country has no place. */
export function countryValues(answer: RumAggregate): CountryValue[] {
  return answer.rows.flatMap((row) => {
    const code = row.group[0];
    const value = numberOf(row.values[0]);

    return code && value !== null && value > 0 ? [{ code, value }] : [];
  });
}
