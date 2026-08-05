import type {
  ActiveWindow,
  Rotation,
  RotationKind,
  Schedule,
} from '@/types/alerting';

export const MICROS_PER_MINUTE = 60_000_000;
export const MICROS_PER_HOUR = 60 * MICROS_PER_MINUTE;
export const MICROS_PER_DAY = 24 * MICROS_PER_HOUR;

export type ScheduleStatus =
  | 'active'
  | 'switching'
  | 'not_started'
  | 'paused'
  | 'gap';

export interface OnCallResolution {
  userId: string;
  source: 'rotation' | 'override';
  rotationId?: string;
  overrideId?: string;
}

export interface ScheduleTimelineSegment {
  id: string;
  startAt: number;
  endAt: number;
  userId: string | null;
  source: 'rotation' | 'override' | 'gap';
  rotationId?: string;
  overrideId?: string;
}

const timeZoneFormatters = new Map<string, Intl.DateTimeFormat>();

function formatterFor(timeZone: string): Intl.DateTimeFormat {
  const normalized = safeTimeZone(timeZone);
  const cached = timeZoneFormatters.get(normalized);
  if (cached) return cached;
  const formatter = new Intl.DateTimeFormat('en-US', {
    timeZone: normalized,
    weekday: 'short',
    hour: '2-digit',
    hourCycle: 'h23',
  });
  timeZoneFormatters.set(normalized, formatter);
  return formatter;
}

export function safeTimeZone(timeZone: string): string {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone }).format();
    return timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

export function rotationPeriodMicros(kind: RotationKind): number {
  if (kind === 'daily') return MICROS_PER_DAY;
  if (kind === 'weekly') return 7 * MICROS_PER_DAY;
  return Math.max(1, kind.custom.period_secs) * 1_000_000;
}

export function activeWindowContains(
  window: ActiveWindow,
  atMicros: number,
  timeZone: string,
): boolean {
  const parts = formatterFor(timeZone).formatToParts(
    new Date(atMicros / 1000),
  );
  const weekday = parts.find((part) => part.type === 'weekday')?.value;
  const hour = Number(parts.find((part) => part.type === 'hour')?.value);
  const weekdayIndex: Record<string, number> = {
    Sun: 0,
    Mon: 1,
    Tue: 2,
    Wed: 3,
    Thu: 4,
    Fri: 5,
    Sat: 6,
  };
  const day = weekday ? weekdayIndex[weekday] : undefined;
  if (day === undefined || !Number.isFinite(hour)) return false;
  if ((window.weekday_mask & (1 << day)) === 0) return false;
  if (window.hour_start <= window.hour_end) {
    return window.hour_start <= hour && hour < window.hour_end;
  }
  return hour >= window.hour_start || hour < window.hour_end;
}

function resolveRotation(
  rotation: Rotation,
  atMicros: number,
  timeZone: string,
): string | null {
  if (rotation.members.length === 0 || atMicros < rotation.start_at) {
    return null;
  }
  if (
    rotation.active_window
    && !activeWindowContains(rotation.active_window, atMicros, timeZone)
  ) {
    return null;
  }
  const period = rotationPeriodMicros(rotation.kind);
  const index = Math.floor((atMicros - rotation.start_at) / period);
  return rotation.members[index % rotation.members.length] ?? null;
}

export function resolveScheduleAt(
  schedule: Schedule,
  atMicros: number,
): OnCallResolution | null {
  if (!schedule.enabled) return null;
  const activeOverride = schedule.overrides.find(
    (override) =>
      override.start_at <= atMicros && atMicros < override.end_at,
  );
  if (activeOverride) {
    return {
      userId: activeOverride.user_id,
      source: 'override',
      overrideId: activeOverride.id,
    };
  }
  for (const rotation of schedule.rotations) {
    const userId = resolveRotation(
      rotation,
      atMicros,
      schedule.timezone,
    );
    if (userId) {
      return {
        userId,
        source: 'rotation',
        rotationId: rotation.id,
      };
    }
  }
  return null;
}

export function scheduleMemberIds(schedule: Schedule): string[] {
  return Array.from(
    new Set(schedule.rotations.flatMap((rotation) => rotation.members)),
  );
}

export function resolutionStartedAt(
  schedule: Schedule,
  resolution: OnCallResolution | null,
  atMicros: number,
): number | null {
  if (!resolution) return null;
  if (resolution.source === 'override') {
    return (
      schedule.overrides.find(
        (override) => override.id === resolution.overrideId,
      )?.start_at ?? null
    );
  }
  const rotation = schedule.rotations.find(
    (item) => item.id === resolution.rotationId,
  );
  if (!rotation || atMicros < rotation.start_at) return null;
  const period = rotationPeriodMicros(rotation.kind);
  const cycle = Math.floor((atMicros - rotation.start_at) / period);
  return rotation.start_at + cycle * period;
}

export function nextScheduleBoundary(
  schedule: Schedule,
  afterMicros: number,
  horizonMicros = 14 * MICROS_PER_DAY,
): number | null {
  if (!schedule.enabled) return null;
  const limit = afterMicros + horizonMicros;
  const candidates: number[] = [];

  for (const override of schedule.overrides) {
    if (override.start_at > afterMicros && override.start_at <= limit) {
      candidates.push(override.start_at);
    }
    if (override.end_at > afterMicros && override.end_at <= limit) {
      candidates.push(override.end_at);
    }
  }

  for (const rotation of schedule.rotations) {
    if (rotation.members.length === 0) continue;
    if (rotation.start_at > afterMicros) {
      if (rotation.start_at <= limit) candidates.push(rotation.start_at);
      continue;
    }
    const period = rotationPeriodMicros(rotation.kind);
    const elapsed = Math.max(0, afterMicros - rotation.start_at);
    const cycle = Math.floor(elapsed / period) + 1;
    const next = rotation.start_at + cycle * period;
    if (next <= limit) candidates.push(next);
  }

  return candidates.length > 0 ? Math.min(...candidates) : null;
}

export function scheduleStatus(
  schedule: Schedule,
  nowMicros: number,
): ScheduleStatus {
  if (!schedule.enabled) return 'paused';
  const members = scheduleMemberIds(schedule);
  if (members.length === 0) return 'gap';
  const firstStart = Math.min(
    ...schedule.rotations
      .filter((rotation) => rotation.members.length > 0)
      .map((rotation) => rotation.start_at),
  );
  if (Number.isFinite(firstStart) && nowMicros < firstStart) {
    return 'not_started';
  }
  if (!resolveScheduleAt(schedule, nowMicros)) return 'gap';
  const next = nextScheduleBoundary(schedule, nowMicros);
  if (next && next - nowMicros <= 2 * MICROS_PER_HOUR) {
    return 'switching';
  }
  return 'active';
}

export function liveOrFutureOverrides(
  schedule: Schedule,
  nowMicros: number,
): Schedule['overrides'] {
  return schedule.overrides
    .filter((override) => override.end_at > nowMicros)
    .sort((a, b) => a.start_at - b.start_at);
}

export function buildScheduleTimeline(
  schedule: Schedule,
  fromMicros: number,
  days = 7,
): ScheduleTimelineSegment[] {
  const endAt = fromMicros + Math.max(1, days) * MICROS_PER_DAY;
  const segments: ScheduleTimelineSegment[] = [];
  let cursor = fromMicros;

  while (cursor < endAt && segments.length < 80) {
    const resolved = resolveScheduleAt(schedule, cursor);
    const boundary =
      nextScheduleBoundary(schedule, cursor, endAt - cursor) ?? endAt;
    const next = Math.min(endAt, Math.max(cursor + 1, boundary));
    segments.push({
      id: `${cursor}-${resolved?.overrideId ?? resolved?.rotationId ?? 'gap'}`,
      startAt: cursor,
      endAt: next,
      userId: resolved?.userId ?? null,
      source: resolved?.source ?? 'gap',
      ...(resolved?.rotationId
        ? { rotationId: resolved.rotationId }
        : {}),
      ...(resolved?.overrideId
        ? { overrideId: resolved.overrideId }
        : {}),
    });
    cursor = next;
  }

  return segments;
}

export function rotationRole(schedule: Schedule): 'primary' | 'secondary' {
  const source = `${schedule.name} ${schedule.rotations[0]?.name ?? ''}`
    .toLowerCase();
  return /(secondary|backup|备用)/.test(source) ? 'secondary' : 'primary';
}

export function rotationKindKey(
  rotation: Rotation,
): 'daily' | 'weekly' | 'workdays' | 'custom' {
  if (
    rotation.kind === 'daily'
    && rotation.active_window?.weekday_mask === 62
    && rotation.active_window.hour_start === 0
    && rotation.active_window.hour_end === 24
  ) {
    return 'workdays';
  }
  if (typeof rotation.kind === 'string') return rotation.kind;
  return 'custom';
}

export function timezoneDisplay(timeZone: string): {
  technical: string;
  label: string;
} {
  const normalized = safeTimeZone(timeZone);
  const labels: Record<string, string> = {
    UTC: '协调世界时',
    'Etc/UTC': '协调世界时',
    'Asia/Shanghai': '北京时间',
    'Asia/Hong_Kong': '香港时间',
    'Asia/Tokyo': '日本标准时间',
    'Europe/London': '伦敦时间',
    'America/New_York': '纽约时间',
    'America/Los_Angeles': '洛杉矶时间',
  };
  const offset = new Intl.DateTimeFormat('en-US', {
    timeZone: normalized,
    timeZoneName: 'shortOffset',
  })
    .formatToParts(new Date())
    .find((part) => part.type === 'timeZoneName')?.value;
  return {
    technical:
      normalized === 'UTC' || normalized === 'Etc/UTC'
        ? 'UTC'
        : offset?.replace('GMT', 'UTC') ?? normalized,
    label: labels[normalized] ?? normalized,
  };
}
