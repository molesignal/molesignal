import type { Schedule } from '@/types/alerting';

import {
  resolveScheduleAt,
  rotationRole,
} from '../../../alerts/schedule/model';

export interface OnCallSnapshot {
  scheduleId: string;
  scheduleName: string;
  primaryUserId: string | null;
}

export function buildOnCallSnapshot(
  schedules: readonly Schedule[],
  nowMicros: number,
): OnCallSnapshot | null {
  const enabled = schedules.filter((schedule) => schedule.enabled);
  if (enabled.length === 0) return null;

  const primary =
    enabled.find(
      (schedule) =>
        rotationRole(schedule) === 'primary'
        && resolveScheduleAt(schedule, nowMicros) !== null,
    )
    ?? enabled.find(
      (schedule) => resolveScheduleAt(schedule, nowMicros) !== null,
    )
    ?? enabled[0];
  if (!primary) return null;

  return {
    scheduleId: primary.id,
    scheduleName: primary.name,
    primaryUserId: resolveScheduleAt(primary, nowMicros)?.userId ?? null,
  };
}
