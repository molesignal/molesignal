/**
 * Who is on call when, for the mock backend: the server works this out in a schedule's timezone
 * (`Schedule::segments`); the mock schedules are all in UTC, so this walks them in UTC, a quarter of
 * an hour at a time plus every override boundary, and merges what stays the same.
 */
import type { CoverageSegment, Schedule, ScheduleCoverage, WeeklyRange } from '../../src/types/alerting';

const MINUTE = 60_000_000;
const STEP = 15 * MINUTE;
const DAY = 24 * 60 * MINUTE;

function minutes(clock: string): number {
  const [hours, mins] = clock.split(':').map(Number);
  return (hours ?? 0) * 60 + (mins ?? 0);
}

function inRanges(ranges: readonly WeeklyRange[], at: number): boolean {
  if (ranges.length === 0) return true;
  const date = new Date(at / 1000);
  const weekday = date.getUTCDay();
  const minute = date.getUTCHours() * 60 + date.getUTCMinutes();
  return ranges.some((range) => {
    const start = minutes(range.start);
    const end = minutes(range.end);
    const today = ((range.weekdayMask >> weekday) & 1) === 1;
    const yesterday = ((range.weekdayMask >> ((weekday + 6) % 7)) & 1) === 1;
    return end > start ? today && minute >= start && minute < end : (today && minute >= start) || (yesterday && minute < end);
  });
}

function periodOf(kind: Schedule['rotations'][number]['kind']): number {
  if (kind === 'daily') return DAY;
  if (kind === 'weekly') return 7 * DAY;
  return Math.max(1, kind.custom.periodSecs) * 1_000_000;
}

function segmentAt(schedule: Schedule, at: number): Omit<CoverageSegment, 'startAt' | 'endAt'> {
  if (!schedule.enabled) return { source: 'gap' };
  const cover = schedule.overrides.find((override) => override.startAt <= at && at < override.endAt);
  const rotation = rotationAt(schedule, at);
  if (cover) {
    return {
      source: 'override',
      userId: cover.userId,
      overrideId: cover.id,
      ...(rotation.userId ? { replacedUserId: rotation.userId } : {}),
    };
  }
  return rotation;
}

function rotationAt(schedule: Schedule, at: number): Omit<CoverageSegment, 'startAt' | 'endAt'> {
  for (const rotation of schedule.rotations) {
    if (rotation.members.length === 0 || at < rotation.startAt) continue;
    if (!inRanges(rotation.activeWindows ?? [], at)) continue;
    const index = Math.floor((at - rotation.startAt) / periodOf(rotation.kind));
    const userId = rotation.members[index % rotation.members.length];
    if (userId) return { source: 'rotation', userId: userId, rotationId: rotation.id };
  }
  return { source: 'gap' };
}

export function scheduleCoverage(schedule: Schedule, from: number, to: number): ScheduleCoverage {
  const points = new Set<number>([from]);
  for (let at = Math.ceil(from / STEP) * STEP; at < to; at += STEP) points.add(at);
  for (const override of schedule.overrides) {
    for (const at of [override.startAt, override.endAt]) if (at > from && at < to) points.add(at);
  }
  const sorted = [...points].sort((left, right) => left - right);
  const segments: CoverageSegment[] = [];
  sorted.forEach((start, index) => {
    const end = sorted[index + 1] ?? to;
    const here = segmentAt(schedule, start);
    const last = segments[segments.length - 1];
    if (
      last
      && last.endAt === start
      && last.source === here.source
      && last.userId === here.userId
      && last.rotationId === here.rotationId
      && last.overrideId === here.overrideId
      && last.replacedUserId === here.replacedUserId
    ) {
      last.endAt = end;
    } else {
      segments.push({ startAt: start, endAt: end, ...here });
    }
  });
  return { from, to, segments };
}

/** A schedule as the server sends it: with its coverage from a day back to 14 days ahead. */
export function withCoverage<T extends Schedule>(schedule: T, now: number): T {
  return { ...schedule, coverage: scheduleCoverage(schedule, now - DAY, now + 14 * DAY) };
}
