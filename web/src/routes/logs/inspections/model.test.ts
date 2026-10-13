import '@/i18n';

import { beforeEach, describe, expect, it } from 'vitest';

import type { InspectionLimits, LogInspection, LogInspectionRun } from '@/api/logInspections';
import type { NotifyConnector } from '@/api/notify';
import i18n from '@/i18n';

import {
  DEFAULT_LIMITS,
  atCapacity,
  channelConnectors,
  draftFromInspection,
  draftProblems,
  durationLabel,
  formatRunDuration,
  hasActiveRun,
  inputFromDraft,
  intervalOptions,
  minuteChoices,
  newDraft,
  refreshInterval,
  runAsName,
  runDurationMicros,
  scheduleLabel,
  standingOf,
  windowFor,
} from './model';

const t = () => i18n.getFixedT(null, 'logs');

function inspection(overrides: Partial<LogInspection> = {}): LogInspection {
  return {
    id: 'insp-1',
    name: 'Payment errors',
    stream: 'app_logs',
    statement: "level = 'ERROR'",
    window_secs: 3_600,
    requirements: 'Treat USER_CANCEL as expected.',
    schedule: { type: 'interval', every_minutes: 60 },
    timezone: 'Asia/Shanghai',
    provider_id: 'provider-1',
    model: null,
    notify_when: 'abnormal',
    notify_routes: [{ connector_id: 'c1', target_type: 'fixed_group', target: 'oc_ops' }],
    language: 'en-us',
    enabled: true,
    run_as_user_id: 'u1',
    run_as_user: { id: 'u1', display_name: 'Dana Reyes', email: 'dana@example.com' },
    next_run_at_micros: 2_000,
    last_run: null,
    active_run: null,
    created_by: 'u1',
    created_at_micros: 1,
    updated_at_micros: 1,
    ...overrides,
  };
}

const HOURLY: InspectionLimits = DEFAULT_LIMITS;
const QUARTER_HOURLY: InspectionLimits = { ...DEFAULT_LIMITS, min_interval_secs: 900 };

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('schedules', () => {
  it('offers only the intervals the instance allows', () => {
    expect(intervalOptions(HOURLY)).toEqual([60, 120, 180, 240, 360, 480, 720]);
    expect(intervalOptions(QUARTER_HOURLY)[0]).toBe(15);
    // An instance that allows one run a day has no interval to offer at all.
    expect(intervalOptions({ ...DEFAULT_LIMITS, min_interval_secs: 86_400 })).toEqual([]);
  });

  it('says a schedule in words', () => {
    expect(scheduleLabel(t(), { type: 'interval', every_minutes: 60 })).toBe('Every 1 hour');
    expect(scheduleLabel(t(), { type: 'interval', every_minutes: 30 })).toBe('Every 30 minutes');
    expect(scheduleLabel(t(), { type: 'interval', every_minutes: 720 })).toBe('Every 12 hours');
    expect(scheduleLabel(t(), { type: 'daily', hour: 9, minute: 5 })).toBe('Daily at 09:05');
    expect(scheduleLabel(t(), { type: 'weekly', weekday: 1, hour: 18, minute: 30 })).toBe(
      'Mondays at 18:30',
    );
  });

  it('says a schedule in Chinese too', async () => {
    await i18n.changeLanguage('zh-cn');

    expect(scheduleLabel(t(), { type: 'interval', every_minutes: 120 })).toBe('每 2 小时');
    expect(scheduleLabel(t(), { type: 'weekly', weekday: 7, hour: 8, minute: 0 })).toBe('每周日 08:00');
  });

  it('states a length of time in the largest unit that is whole', () => {
    expect(durationLabel(t(), 900)).toBe('15 minutes');
    expect(durationLabel(t(), 5_400)).toBe('90 minutes');
    expect(durationLabel(t(), 3 * 3_600)).toBe('3 hours');
    expect(durationLabel(t(), 86_400)).toBe('1 day');
    expect(durationLabel(t(), 7 * 86_400)).toBe('7 days');
  });

  it('offers every fifth minute of a time of day, and the minute that was saved', () => {
    expect(minuteChoices(30)).toEqual([0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
    // Saved through the API at a minute the form does not step on: it is not lost on editing.
    expect(minuteChoices(42)).toEqual([0, 5, 10, 15, 20, 25, 30, 35, 40, 42, 45, 50, 55]);
  });

  it('starts a schedule with a window as long as its period', () => {
    expect(windowFor('interval', 60)).toBe(3_600);
    // Two hours is not offered as a window: the next one up covers it.
    expect(windowFor('interval', 120)).toBe(3 * 3_600);
    expect(windowFor('daily', 60)).toBe(86_400);
    expect(windowFor('weekly', 60)).toBe(7 * 86_400);
  });
});

describe('drafts', () => {
  it('starts a new inspection from the query on the page', () => {
    const draft = newDraft(HOURLY, {
      stream: 'app_logs',
      statement: "  level = 'ERROR'  ",
      requirements: ' watch payments ',
      providerId: 'provider-1',
    });

    expect(draft).toMatchObject({
      name: '',
      stream: 'app_logs',
      statement: "level = 'ERROR'",
      requirements: 'watch payments',
      scheduleKind: 'interval',
      everyMinutes: 60,
      windowSecs: 3_600,
      providerId: 'provider-1',
      notifyWhen: 'abnormal',
      routes: [],
      enabled: true,
    });
  });

  it('falls back to a daily schedule where no interval is allowed', () => {
    const draft = newDraft({ ...DEFAULT_LIMITS, min_interval_secs: 86_400 });

    expect(draft.scheduleKind).toBe('daily');
    expect(draft.windowSecs).toBe(86_400);
  });

  it('reads an inspection back into the form and out again unchanged', () => {
    const weekly = inspection({
      schedule: { type: 'weekly', weekday: 3, hour: 7, minute: 45 },
      window_secs: 7 * 86_400,
      notify_when: 'attention',
    });
    const draft = draftFromInspection(weekly);

    expect(draft).toMatchObject({ scheduleKind: 'weekly', weekday: 3, hour: 7, minute: 45 });
    expect(inputFromDraft(draft, { timezone: weekly.timezone, language: weekly.language })).toEqual({
      name: 'Payment errors',
      stream: 'app_logs',
      statement: "level = 'ERROR'",
      window_secs: 7 * 86_400,
      requirements: 'Treat USER_CANCEL as expected.',
      schedule: { type: 'weekly', weekday: 3, hour: 7, minute: 45 },
      timezone: 'Asia/Shanghai',
      provider_id: 'provider-1',
      model: null,
      notify_when: 'attention',
      notify_routes: [{ connector_id: 'c1', target_type: 'fixed_group', target: 'oc_ops' }],
      language: 'en-us',
      enabled: true,
    });
  });

  it('names what keeps a draft from being saved', () => {
    const complete = draftFromInspection(inspection());

    expect(draftProblems(complete, HOURLY)).toEqual([]);
    expect(draftProblems({ ...complete, name: '   ' }, HOURLY)).toEqual(['name']);
    expect(draftProblems({ ...complete, stream: '', providerId: '' }, HOURLY)).toEqual([
      'stream',
      'provider',
    ]);
    // Saved when the instance allowed a run every quarter of an hour; it no longer does.
    expect(draftProblems({ ...complete, everyMinutes: 15 }, HOURLY)).toEqual(['schedule']);
    expect(
      draftProblems(
        { ...complete, routes: [{ connector_id: 'c1', target_type: 'fixed_group', target: ' ' }] },
        HOURLY,
      ),
    ).toEqual(['route_target']);
    expect(
      draftProblems(
        {
          ...complete,
          routes: [
            { connector_id: 'c1', target_type: 'fixed_group', target: 'oc_ops' },
            { connector_id: 'c1', target_type: 'fixed_group', target: ' oc_ops ' },
          ],
        },
        HOURLY,
      ),
    ).toEqual(['route_repeated']);
  });
});

describe('channels', () => {
  const connector = (id: string, overrides: Partial<NotifyConnector> = {}): NotifyConnector => ({
    id,
    organization_id: 'org',
    name: `Connector ${id}`,
    connector_type: 'lark_app',
    config: {},
    capabilities: {
      direct_user: true,
      group: true,
      rich_text: true,
      interactive: false,
      acknowledgement: false,
      attachments: false,
    },
    enabled: true,
    status: 'connected',
    created_at: 1,
    updated_at: 1,
    ...overrides,
  });
  const email = connector('mail', {
    capabilities: { ...connector('mail').capabilities, group: false },
  });

  it('offers the connectors that are enabled, and no other', () => {
    expect(channelConnectors([connector('a'), connector('b', { enabled: false }), email], [])).toEqual([
      { id: 'a', name: 'Connector a', group: true, standing: 'on' },
      { id: 'mail', name: 'Connector mail', group: false, standing: 'on' },
    ]);
  });

  it('keeps a connector a channel is on, said to be disabled or not found', () => {
    const offered = channelConnectors(
      [connector('a'), connector('b', { enabled: false })],
      [
        { connector_id: 'b', target_type: 'fixed_group', target: 'oc_ops' },
        { connector_id: 'b', target_type: 'fixed_group', target: 'oc_security' },
        { connector_id: 'lost', target_type: 'fixed_address', target: 'ops@example.com' },
        { connector_id: 'a', target_type: 'fixed_group', target: 'oc_ops' },
      ],
    );

    expect(offered).toEqual([
      { id: 'a', name: 'Connector a', group: true, standing: 'on' },
      { id: 'b', name: 'Connector b', group: true, standing: 'off' },
      { id: 'lost', name: 'lost', group: false, standing: 'gone' },
    ]);
  });
});

describe('how an inspection stands', () => {
  it('is its run under way before anything else', () => {
    const running = inspection({
      last_run: { at_micros: 1, status: 'succeeded', level: 'normal' },
      active_run: { id: 'r2', status: 'running', trigger: 'manual', created_at_micros: 2, started_at_micros: 3 },
    });

    expect(standingOf(running)).toEqual({ kind: 'active', status: 'running' });
    expect(hasActiveRun([inspection(), running])).toBe(true);
    expect(hasActiveRun([inspection()])).toBe(false);
  });

  it('is the verdict of the last report, or that the last run left none', () => {
    expect(standingOf(inspection())).toEqual({ kind: 'never' });
    expect(
      standingOf(inspection({ last_run: { at_micros: 1, status: 'succeeded', level: 'abnormal' } })),
    ).toEqual({ kind: 'level', level: 'abnormal' });
    expect(
      standingOf(inspection({ last_run: { at_micros: 1, status: 'timed_out', level: null } })),
    ).toEqual({ kind: 'failed', status: 'timed_out' });
  });

  it('is read again sooner while a run is under way', () => {
    expect(refreshInterval(true)).toBeLessThan(refreshInterval(false));
  });

  it('names who it runs as by the best name there is', () => {
    expect(runAsName(inspection())).toBe('Dana Reyes');
    expect(
      runAsName(inspection({ run_as_user: { id: 'u1', display_name: ' ', email: 'dana@example.com' } })),
    ).toBe('dana@example.com');
    expect(runAsName(inspection({ run_as_user: null }))).toBe('u1');
  });

  it('knows when the organization can save no more', () => {
    expect(atCapacity(19, HOURLY)).toBe(false);
    expect(atCapacity(20, HOURLY)).toBe(true);
  });
});

describe('runs', () => {
  const run = (overrides: Partial<LogInspectionRun>): LogInspectionRun => ({
    id: 'r1',
    inspection_id: 'insp-1',
    trigger: 'schedule',
    status: 'succeeded',
    level: 'normal',
    window_start_micros: 0,
    window_end_micros: 1,
    summary: null,
    report: null,
    failure: null,
    error: null,
    provider_id: null,
    model: null,
    run_as_user_id: null,
    requested_by: null,
    requested_by_user: null,
    tool_calls: [],
    prompt_tokens: 0,
    completion_tokens: 0,
    notifications: [],
    created_at_micros: 10,
    started_at_micros: null,
    finished_at_micros: null,
    ...overrides,
  });

  it('times a run from its start to its end, or to now while it is under way', () => {
    const second = 1_000_000;

    expect(runDurationMicros(run({ status: 'queued' }), 100 * second)).toBeNull();
    expect(
      runDurationMicros(run({ started_at_micros: 10 * second, finished_at_micros: 52 * second }), 0),
    ).toBe(42 * second);
    expect(
      runDurationMicros(run({ status: 'running', started_at_micros: 10 * second }), 75 * second),
    ).toBe(65 * second);
    expect(formatRunDuration(42 * second)).toBe('42 s');
    expect(formatRunDuration(185 * second)).toBe('3 min 5 s');
    expect(formatRunDuration(null)).toBe('—');
  });
});
