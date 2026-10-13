import type { TFunction } from 'i18next';

import type {
  InspectionLevel,
  InspectionLimits,
  InspectionNotifyRoute,
  InspectionNotifyWhen,
  InspectionRunStatus,
  InspectionSchedule,
  LogInspection,
  LogInspectionInput,
  LogInspectionRun,
} from '@/api/logInspections';
import type { NotifyConnector } from '@/api/notify';

export type ScheduleKind = InspectionSchedule['type'];

export const SCHEDULE_KINDS: readonly ScheduleKind[] = ['interval', 'daily', 'weekly'];

/** The intervals a form offers, in minutes; an instance offers those it allows. */
export const INTERVAL_MINUTES: readonly number[] = [15, 30, 60, 120, 180, 240, 360, 480, 720];

/** How much time a run may look over, in seconds: from a quarter of an hour to a week. */
export const WINDOW_SECONDS: readonly number[] = [
  900,
  3_600,
  3 * 3_600,
  6 * 3_600,
  12 * 3_600,
  86_400,
  3 * 86_400,
  7 * 86_400,
];

export const NOTIFY_WHEN: readonly InspectionNotifyWhen[] = ['abnormal', 'attention', 'always'];

export const WEEKDAYS: readonly number[] = [1, 2, 3, 4, 5, 6, 7];

/** While a run waits or is under way the page is read again every few seconds. */
export const ACTIVE_REFRESH_MS = 4_000;
/** Otherwise now and then, so a run that began on its schedule shows up. */
export const IDLE_REFRESH_MS = 30_000;

export const NAME_MAX = 120;
export const REQUIREMENTS_MAX = 2_000;
export const MAX_ROUTES = 10;

/** A connector a channel can be on, as the form offers it. */
export interface ChannelConnector {
  id: string;
  name: string;
  /** Whether it posts to groups: a channel on it starts as a group, an address otherwise. */
  group: boolean;
  /**
   * `off` and `gone` are offered only because a channel of the inspection is
   * on them: a channel saved earlier is shown for what it is and can be taken out.
   */
  standing: 'on' | 'off' | 'gone';
}

/**
 * The connectors the channels of an inspection can be on: those enabled, and
 * after them those a channel is already on although they were disabled since.
 */
export function channelConnectors(
  connectors: readonly NotifyConnector[],
  routes: readonly InspectionNotifyRoute[],
): ChannelConnector[] {
  const offered: ChannelConnector[] = connectors
    .filter((connector) => connector.enabled)
    .map((connector) => ({
      id: connector.id,
      name: connector.name,
      group: connector.capabilities.group,
      standing: 'on',
    }));

  for (const route of routes) {
    if (offered.some((connector) => connector.id === route.connector_id)) continue;

    const known = connectors.find((connector) => connector.id === route.connector_id);

    offered.push(
      known
        ? { id: known.id, name: known.name, group: known.capabilities.group, standing: 'off' }
        : {
            id: route.connector_id,
            name: route.connector_id,
            group: route.target_type === 'fixed_group',
            standing: 'gone',
          },
    );
  }

  return offered;
}

/** What the limits are taken to be until the instance has said. */
export const DEFAULT_LIMITS: InspectionLimits = {
  min_interval_secs: 3_600,
  max_per_org: 20,
  run_timeout_secs: 300,
  report_retention_days: 30,
};

/** An inspection as the form holds it: one value per control. */
export interface InspectionDraft {
  name: string;
  stream: string;
  statement: string;
  requirements: string;
  scheduleKind: ScheduleKind;
  everyMinutes: number;
  hour: number;
  minute: number;
  weekday: number;
  windowSecs: number;
  providerId: string;
  notifyWhen: InspectionNotifyWhen;
  routes: InspectionNotifyRoute[];
  enabled: boolean;
}

/** The intervals this instance allows, shortest first. */
export function intervalOptions(limits: InspectionLimits): number[] {
  return INTERVAL_MINUTES.filter((minutes) => minutes * 60 >= limits.min_interval_secs);
}

/** How far apart two runs of a schedule are, in seconds. */
export function periodSeconds(kind: ScheduleKind, everyMinutes: number): number {
  if (kind === 'interval') return everyMinutes * 60;
  return kind === 'daily' ? 86_400 : 7 * 86_400;
}

/**
 * The window a schedule starts out with: as long as its period, so that run
 * after run every log is looked over once.
 */
export function windowFor(kind: ScheduleKind, everyMinutes: number): number {
  const period = periodSeconds(kind, everyMinutes);

  return WINDOW_SECONDS.find((seconds) => seconds >= period) ?? WINDOW_SECONDS.at(-1)!;
}

export function scheduleOf(draft: InspectionDraft): InspectionSchedule {
  if (draft.scheduleKind === 'interval') {
    return { type: 'interval', every_minutes: draft.everyMinutes };
  }
  if (draft.scheduleKind === 'daily') {
    return { type: 'daily', hour: draft.hour, minute: draft.minute };
  }
  return { type: 'weekly', weekday: draft.weekday, hour: draft.hour, minute: draft.minute };
}

interface DraftSeed {
  stream?: string | undefined;
  statement?: string | undefined;
  requirements?: string | undefined;
  providerId?: string | undefined;
}

/** A new inspection: once every allowed interval nearest to an hour, told when abnormal. */
export function newDraft(limits: InspectionLimits, seed: DraftSeed = {}): InspectionDraft {
  const allowed = intervalOptions(limits);
  const everyMinutes = allowed.find((minutes) => minutes >= 60) ?? allowed[0] ?? 60;
  const daily = allowed.length === 0 && limits.min_interval_secs <= 86_400;
  const kind: ScheduleKind = allowed.length > 0 ? 'interval' : daily ? 'daily' : 'weekly';

  return {
    name: '',
    stream: seed.stream ?? '',
    statement: seed.statement?.trim() ?? '',
    requirements: seed.requirements?.trim() ?? '',
    scheduleKind: kind,
    everyMinutes,
    hour: 9,
    minute: 0,
    weekday: 1,
    windowSecs: windowFor(kind, everyMinutes),
    providerId: seed.providerId ?? '',
    notifyWhen: 'abnormal',
    routes: [],
    enabled: true,
  };
}

export function draftFromInspection(inspection: LogInspection): InspectionDraft {
  const { schedule } = inspection;

  return {
    name: inspection.name,
    stream: inspection.stream,
    statement: inspection.statement,
    requirements: inspection.requirements,
    scheduleKind: schedule.type,
    everyMinutes: schedule.type === 'interval' ? schedule.every_minutes : 60,
    hour: schedule.type === 'interval' ? 9 : schedule.hour,
    minute: schedule.type === 'interval' ? 0 : schedule.minute,
    weekday: schedule.type === 'weekly' ? schedule.weekday : 1,
    windowSecs: inspection.window_secs,
    providerId: inspection.provider_id,
    notifyWhen: inspection.notify_when,
    routes: inspection.notify_routes.map((route) => ({ ...route })),
    enabled: inspection.enabled,
  };
}

export type DraftProblem =
  | 'name'
  | 'stream'
  | 'provider'
  | 'schedule'
  | 'route_target'
  | 'route_repeated';

/** What keeps a draft from being saved. The backend checks again; this is to say so early. */
export function draftProblems(draft: InspectionDraft, limits: InspectionLimits): DraftProblem[] {
  const problems: DraftProblem[] = [];
  const name = draft.name.trim();

  if (!name || name.length > NAME_MAX) problems.push('name');
  if (!draft.stream.trim()) problems.push('stream');
  if (!draft.providerId) problems.push('provider');
  if (periodSeconds(draft.scheduleKind, draft.everyMinutes) < limits.min_interval_secs) {
    problems.push('schedule');
  }
  if (draft.routes.some((route) => !route.target.trim())) problems.push('route_target');

  const channels = draft.routes.map(
    (route) => `${route.connector_id}\u0000${route.target_type}\u0000${route.target.trim()}`,
  );

  if (new Set(channels).size !== channels.length) problems.push('route_repeated');

  return problems;
}

/** What is sent when a draft is saved. `model` stays null: each run uses the provider's default. */
export function inputFromDraft(
  draft: InspectionDraft,
  context: { timezone: string; language: string; model?: string | null },
): LogInspectionInput {
  return {
    name: draft.name.trim(),
    stream: draft.stream.trim(),
    statement: draft.statement.trim(),
    window_secs: draft.windowSecs,
    requirements: draft.requirements.trim(),
    schedule: scheduleOf(draft),
    timezone: context.timezone,
    provider_id: draft.providerId,
    model: context.model ?? null,
    notify_when: draft.notifyWhen,
    notify_routes: draft.routes.map((route) => ({ ...route, target: route.target.trim() })),
    language: context.language,
    enabled: draft.enabled,
  };
}

/** A length of time in the largest unit that states it whole: "15 minutes", "3 hours", "7 days". */
export function durationLabel(t: TFunction<'logs'>, seconds: number): string {
  if (seconds % 86_400 === 0) return t('inspections.units.days', { count: seconds / 86_400 });
  if (seconds % 3_600 === 0) return t('inspections.units.hours', { count: seconds / 3_600 });

  return t('inspections.units.minutes', { count: Math.round(seconds / 60) });
}

export function timeOfDay(hour: number, minute: number): string {
  return `${twoDigits(hour)}:${twoDigits(minute)}`;
}

export function twoDigits(value: number): string {
  return String(value).padStart(2, '0');
}

export const HOURS_OF_DAY: readonly number[] = Array.from({ length: 24 }, (_, hour) => hour);

/**
 * The minutes a time of day is chosen from: every fifth one, and the minute an
 * inspection was saved with where it lies between them.
 */
export function minuteChoices(minute: number): number[] {
  const steps = Array.from({ length: 12 }, (_, index) => index * 5);

  return steps.includes(minute) ? steps : [...steps, minute].sort((a, b) => a - b);
}

/** A schedule in words: "Every 1 hour", "Daily at 09:30", "Mondays at 09:30". */
export function scheduleLabel(t: TFunction<'logs'>, schedule: InspectionSchedule): string {
  if (schedule.type === 'interval') {
    return t('inspections.schedule.every', {
      duration: durationLabel(t, schedule.every_minutes * 60),
    });
  }

  const time = timeOfDay(schedule.hour, schedule.minute);

  if (schedule.type === 'daily') return t('inspections.schedule.daily_at', { time });

  return t('inspections.schedule.weekly_at', {
    weekday: t(`inspections.weekdays.${schedule.weekday}`),
    time,
  });
}

/** How an inspection stands, as its row says it. */
export type InspectionStanding =
  | { kind: 'active'; status: Extract<InspectionRunStatus, 'queued' | 'running'> }
  | { kind: 'never' }
  | { kind: 'failed'; status: Extract<InspectionRunStatus, 'failed' | 'timed_out'> }
  | { kind: 'level'; level: InspectionLevel };

export function standingOf(inspection: LogInspection): InspectionStanding {
  const active = inspection.active_run?.status;

  if (active === 'queued' || active === 'running') return { kind: 'active', status: active };

  const last = inspection.last_run;

  if (!last) return { kind: 'never' };
  if (last.status === 'failed' || last.status === 'timed_out') {
    return { kind: 'failed', status: last.status };
  }

  return { kind: 'level', level: last.level ?? 'unknown' };
}

export function isActiveRun(status: InspectionRunStatus): boolean {
  return status === 'queued' || status === 'running';
}

export function hasActiveRun(inspections: readonly LogInspection[] | undefined): boolean {
  return Boolean(inspections?.some((inspection) => inspection.active_run));
}

export function refreshInterval(active: boolean): number {
  return active ? ACTIVE_REFRESH_MS : IDLE_REFRESH_MS;
}

/** How long a run took, or has taken so far; null for one that has not started. */
export function runDurationMicros(run: LogInspectionRun, nowMicros: number): number | null {
  if (!run.started_at_micros) return null;

  const end = run.finished_at_micros ?? (run.status === 'running' ? nowMicros : null);

  return end === null ? null : Math.max(0, end - run.started_at_micros);
}

export function formatRunDuration(micros: number | null): string {
  if (micros === null) return '—';

  const seconds = Math.round(micros / 1_000_000);

  if (seconds < 60) return `${seconds} s`;

  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

/** Who an inspection runs as, by the best name there is for them. */
export function runAsName(inspection: LogInspection): string {
  return (
    inspection.run_as_user?.display_name.trim() ||
    inspection.run_as_user?.email.trim() ||
    inspection.run_as_user_id
  );
}

/** Whether another inspection may be saved in the organization. */
export function atCapacity(count: number, limits: InspectionLimits): boolean {
  return count >= limits.max_per_org;
}
