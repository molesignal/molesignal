import { buildScheduleTimeline } from '@/routes/alerts/schedule/model';
import type { Schedule } from '@/types/alerting';

/** How far ahead the next shift is looked for. */
const HORIZON_DAYS = 30;

export interface MyShift {
  schedule: Schedule;
  /** On call now; otherwise this is the next shift. */
  now: boolean;
  startAt: number;
  endAt: number;
}

/**
 * The reader's shift under way, else their next one within a month: the soonest across the
 * schedules. A shift runs on while the same person holds it, across the rotation's segments.
 */
export function myShift(schedules: readonly Schedule[], userId: string, nowMicros: number): MyShift | null {
  let soonest: MyShift | null = null;

  for (const schedule of schedules) {
    if (!schedule.enabled) continue;
    const segments = buildScheduleTimeline(schedule, nowMicros, HORIZON_DAYS);
    const index = segments.findIndex((segment) => segment.userId === userId);
    const first = segments[index];
    if (!first) continue;

    let endAt = first.endAt;
    for (const next of segments.slice(index + 1)) {
      if (next.userId !== userId || next.startAt !== endAt) break;
      endAt = next.endAt;
    }

    if (!soonest || first.startAt < soonest.startAt) {
      soonest = { schedule, now: first.startAt <= nowMicros, startAt: first.startAt, endAt };
    }
  }

  return soonest;
}
