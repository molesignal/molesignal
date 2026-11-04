import type { RumDistribution } from '@/api/rum';

/** Where a value falls along the bars, from 0 at the first to 1 at the last. */
function placeOf(answer: RumDistribution, value: number): number | null {
  const count = answer.bars.length;

  if (count === 0) return null;

  const index = answer.bars.findIndex((bar) => value < bar.to);
  const at = index < 0 ? count - 1 : index;
  const bar = answer.bars[at]!;
  const width = bar.to - bar.from;
  const within = width > 0 ? Math.min(1, Math.max(0, (value - bar.from) / width)) : 0;

  return (at + within) / count;
}

export interface DistributionMark {
  name: string;
  place: number;
  /** The line its name stands on: a name that would run into the one before takes the next. */
  line: number;
}

/** How near two marks can stand with their names side by side, as a share of the chart. */
const ROOM_OF_A_NAME = 0.05;

/** The percentiles that can be marked on the bars, and where; a single bar says nothing of them. */
export function markerPlaces(answer: RumDistribution, names: ReadonlyArray<'p50' | 'p75' | 'p95' | 'p99'>): DistributionMark[] {
  if (answer.bars.length < 2) return [];

  const marks: DistributionMark[] = [];

  for (const name of names) {
    const value = answer[name];
    const place = value === null || value === undefined ? null : placeOf(answer, value);

    if (place === null) continue;

    const before = marks.at(-1);
    const line = before && place - before.place < ROOM_OF_A_NAME ? before.line + 1 : 0;

    marks.push({ name, place, line });
  }

  return marks;
}
