/**
 * Mock scheduled log inspections, as `/logs/inspections` reports them: one that
 * found a problem an hour's run ago, one that is quiet, one whose model was
 * switched off, and one that is switched off itself.
 *
 * The inspections and their runs live in the mock server of one test, so what a
 * test saves, switches or deletes is gone with it. A manual run stays queued
 * until the test moves it along with `updateMockInspectionRun`, the way the
 * backend worker would.
 *
 * Every time is relative to the mock session's frozen clock.
 */
import type { Express, Request, Response } from 'express';

import type {
  InspectionLimits,
  LogInspection,
  LogInspectionInput,
  LogInspectionRun,
} from '../../src/api/logInspections';

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export const MOCK_INSPECTION = {
  payments: '2zA1paymentErrorsHourly0001',
  audit: '2zA2dailyAuditInspection002',
  gateway: '2zA3weeklyGatewayModelOff03',
  paused: '2zA4pausedDebugInspection04',
} as const;

export const MOCK_INSPECTION_RUN = {
  abnormal: '2zR1abnormalLatestRun000001',
  normal: '2zR2normalRunBefore00000002',
  timedOut: '2zR3timedOutRun000000000003',
  manualProse: '2zR4manualProseRun000000004',
  gatewayFailed: '2zR5gatewayModelFailed00005',
  auditNormal: '2zR6auditNormalRun000000006',
} as const;

export const MOCK_INSPECTION_LIMITS: InspectionLimits = {
  minIntervalSecs: 3_600,
  maxPerOrg: 20,
  runTimeoutSecs: 300,
  reportRetentionDays: 30,
};

const DANA = { id: 'u1', displayName: 'Dana Reyes', email: 'sre@example.com' };
const KENJI = { id: 'u2', displayName: 'Kenji Watanabe', email: 'kenji@example.com' };

const ABNORMAL_REPORT = JSON.stringify({
  level: 'abnormal',
  summary:
    'Payment errors tripled after 09:10: 214 ERROR logs from payment-gateway, most of them acquirer timeouts.',
  anomaly_points: [
    {
      metric: 'ERROR logs per 5 minutes',
      observed: '58 at the peak',
      expected: 'under 10',
      description: 'Began at 09:10 and involves payment-gateway only.',
    },
  ],
  evidence: [
    { label: '214 ERROR logs from payment-gateway in the hour', kind: 'logs', stream: 'application_logs' },
    { label: '187 of them say "acquirer timeout after 3000 ms"', kind: 'logs', stream: 'application_logs' },
  ],
  likely_causes: ['The acquirer is slow or unreachable.'],
  limitations: ['No acquirer-side logs are in this stream.'],
  suggested_next_steps: ['Check the acquirer status page.', 'Raise the timeout only after confirming latency.'],
  confidence: 'high',
});

const NORMAL_REPORT = JSON.stringify({
  level: 'normal',
  summary: 'Nothing unusual: 1,204 logs, 3 of them warnings about retried webhooks.',
  evidence: [{ label: '1,204 logs, 99% INFO', kind: 'logs', stream: 'application_logs' }],
  likely_causes: [],
  limitations: [],
  suggested_next_steps: [],
  confidence: 'high',
});

function run(nowMicros: number, overrides: Partial<LogInspectionRun>): LogInspectionRun {
  return {
    id: 'run',
    inspectionId: MOCK_INSPECTION.payments,
    trigger: 'schedule',
    status: 'succeeded',
    level: 'normal',
    windowStart: nowMicros - 2 * HOUR,
    windowEnd: nowMicros - HOUR,
    summary: null,
    report: null,
    failure: null,
    error: null,
    providerId: 'openai-gpt-4o',
    model: 'gpt-4o',
    runAsUserId: 'u1',
    requestedBy: null,
    requestedByUser: null,
    toolCalls: [],
    promptTokens: 0,
    completionTokens: 0,
    notifications: [],
    createdAt: nowMicros - HOUR,
    startedAt: nowMicros - HOUR,
    finishedAt: nowMicros - HOUR + 48 * SECOND,
    ...overrides,
  };
}

/** The runs the mock organization starts with. */
export function seedInspectionRuns(nowMicros: number): LogInspectionRun[] {
  return [
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.abnormal,
      level: 'abnormal',
      windowStart: nowMicros - 80 * MINUTE,
      windowEnd: nowMicros - 20 * MINUTE,
      summary:
        'Payment errors tripled after 09:10: 214 ERROR logs from payment-gateway, most of them acquirer timeouts.',
      report: ABNORMAL_REPORT,
      toolCalls: [
        { name: 'get_stream_schema', ok: true },
        { name: 'query_logs', ok: true },
        { name: 'query_logs', ok: false },
        { name: 'search_field_values', ok: true },
      ],
      promptTokens: 18_420,
      completionTokens: 1_260,
      notifications: [
        {
          connectorId: 'lark-ops',
          targetType: 'fixed_group',
          targetMasked: 'oc***7f',
          delivered: true,
          error: null,
        },
        {
          connectorId: 'ops-webhook',
          targetType: 'fixed_address',
          targetMasked: 'h***@example.com',
          delivered: false,
          error: 'webhook returned HTTP 502',
        },
      ],
      createdAt: nowMicros - 20 * MINUTE,
      startedAt: nowMicros - 20 * MINUTE,
      finishedAt: nowMicros - 20 * MINUTE + 72 * SECOND,
    }),
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.normal,
      level: 'normal',
      windowStart: nowMicros - 140 * MINUTE,
      windowEnd: nowMicros - 80 * MINUTE,
      summary: 'Nothing unusual: 1,204 logs, 3 of them warnings about retried webhooks.',
      report: NORMAL_REPORT,
      toolCalls: [
        { name: 'get_stream_schema', ok: true },
        { name: 'query_logs', ok: true },
      ],
      promptTokens: 9_100,
      completionTokens: 420,
      createdAt: nowMicros - 80 * MINUTE,
      startedAt: nowMicros - 80 * MINUTE,
      finishedAt: nowMicros - 80 * MINUTE + 31 * SECOND,
    }),
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.timedOut,
      status: 'timed_out',
      level: null,
      windowStart: nowMicros - 200 * MINUTE,
      windowEnd: nowMicros - 140 * MINUTE,
      failure: 'timed_out',
      error: 'the inspection did not finish within 300 seconds',
      toolCalls: [{ name: 'query_logs', ok: true }],
      createdAt: nowMicros - 140 * MINUTE,
      startedAt: nowMicros - 140 * MINUTE,
      finishedAt: nowMicros - 135 * MINUTE,
    }),
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.manualProse,
      trigger: 'manual',
      level: 'unknown',
      windowStart: nowMicros - 6 * HOUR,
      windowEnd: nowMicros - 5 * HOUR,
      // A model that did not keep to the shape: what it wrote is kept as it is.
      report: 'The logs look steady over the hour. I saw no error bursts worth reporting.',
      requestedBy: 'u2',
      requestedByUser: KENJI,
      promptTokens: 4_800,
      completionTokens: 96,
      createdAt: nowMicros - 5 * HOUR,
      startedAt: nowMicros - 5 * HOUR + 4 * SECOND,
      finishedAt: nowMicros - 5 * HOUR + 26 * SECOND,
    }),
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.gatewayFailed,
      inspectionId: MOCK_INSPECTION.gateway,
      status: 'failed',
      level: null,
      windowStart: nowMicros - 9 * DAY,
      windowEnd: nowMicros - 2 * DAY,
      failure: 'model',
      error: 'model provider `Internal Qwen` is disabled',
      providerId: 'compatible-qwen',
      model: null,
      createdAt: nowMicros - 2 * DAY,
      startedAt: nowMicros - 2 * DAY,
      finishedAt: nowMicros - 2 * DAY + SECOND,
    }),
    run(nowMicros, {
      id: MOCK_INSPECTION_RUN.auditNormal,
      inspectionId: MOCK_INSPECTION.audit,
      level: 'normal',
      windowStart: nowMicros - 25 * HOUR,
      windowEnd: nowMicros - HOUR,
      summary: 'Nothing unusual: 1,204 logs, 3 of them warnings about retried webhooks.',
      report: NORMAL_REPORT,
      createdAt: nowMicros - HOUR,
      startedAt: nowMicros - HOUR,
      finishedAt: nowMicros - HOUR + 40 * SECOND,
    }),
  ];
}

function inspection(nowMicros: number, overrides: Partial<LogInspection>): LogInspection {
  return {
    id: 'inspection',
    name: 'Inspection',
    stream: 'application_logs',
    statement: '',
    windowSecs: 3_600,
    requirements: '',
    schedule: { type: 'interval', everyMinutes: 60 },
    timezone: 'UTC',
    providerId: 'openai-gpt-4o',
    model: null,
    notifyWhen: 'abnormal',
    notifyRoutes: [],
    language: 'en-us',
    enabled: true,
    runAsUserId: 'u1',
    runAsUser: DANA,
    nextRunAt: nowMicros + 40 * MINUTE,
    lastRun: null,
    activeRun: null,
    createdBy: 'u1',
    createdAt: nowMicros - 12 * DAY,
    updatedAt: nowMicros - 3 * DAY,
    ...overrides,
  };
}

/** The inspections the mock organization starts with, by name. */
export function seedInspections(nowMicros: number): LogInspection[] {
  return [
    inspection(nowMicros, {
      id: MOCK_INSPECTION.audit,
      name: 'Daily audit of application logs',
      schedule: { type: 'daily', hour: 9, minute: 0 },
      windowSecs: 86_400,
      notifyWhen: 'always',
      notifyRoutes: [{ connectorId: 'lark-ops', targetType: 'fixed_group', target: 'oc_audit_7f' }],
      nextRunAt: nowMicros + 23 * HOUR,
      lastRun: { atTime: nowMicros - HOUR + 40 * SECOND, status: 'succeeded', level: 'normal' },
      runAsUserId: 'u2',
      runAsUser: KENJI,
    }),
    inspection(nowMicros, {
      id: MOCK_INSPECTION.paused,
      name: 'Debug noise (paused)',
      statement: "level = 'DEBUG'",
      enabled: false,
      nextRunAt: null,
    }),
    inspection(nowMicros, {
      id: MOCK_INSPECTION.payments,
      name: 'Payment errors',
      statement: "level = 'ERROR' AND service = 'payment-gateway'",
      requirements: 'Treat USER_CANCEL as an expected business result.',
      notifyRoutes: [
        { connectorId: 'lark-ops', targetType: 'fixed_group', target: 'oc_ops_7f' },
        { connectorId: 'ops-webhook', targetType: 'fixed_address', target: 'hooks@example.com' },
      ],
      lastRun: {
        atTime: nowMicros - 20 * MINUTE + 72 * SECOND,
        status: 'succeeded',
        level: 'abnormal',
      },
    }),
    inspection(nowMicros, {
      id: MOCK_INSPECTION.gateway,
      name: 'Weekly gateway review',
      stream: 'access_logs',
      schedule: { type: 'weekly', weekday: 1, hour: 8, minute: 30 },
      windowSecs: 7 * 86_400,
      providerId: 'compatible-qwen',
      notifyWhen: 'attention',
      notifyRoutes: [{ connectorId: 'lark-ops', targetType: 'fixed_group', target: 'oc_ops_7f' }],
      nextRunAt: nowMicros + 5 * DAY,
      lastRun: { atTime: nowMicros - 2 * DAY + SECOND, status: 'failed', level: null },
    }),
  ];
}

export function registerLogInspectionRoutes(app: Express, nowIso: string): void {
  const nowMicros = Date.parse(nowIso) * 1000;
  let inspections = seedInspections(nowMicros);
  let runs = seedInspectionRuns(nowMicros);
  let limits = { ...MOCK_INSPECTION_LIMITS };
  let serial = 0;

  const missing = (res: Response) => res.status(404).json({ error: 'not found: log inspection' });
  const find = (id: string | undefined) => inspections.find((item) => item.id === id);
  const activeRun = (id: string) =>
    runs.find((item) => item.inspectionId === id && (item.status === 'queued' || item.status === 'running'));
  // The list says of each inspection whether a run of it waits or is under way.
  const shown = (item: LogInspection): LogInspection => {
    const active = activeRun(item.id);

    return {
      ...item,
      activeRun: active
        ? {
            id: active.id,
            status: active.status,
            trigger: active.trigger,
            createdAt: active.createdAt,
            startedAt: active.startedAt,
          }
        : null,
    };
  };
  const nextRun = (input: LogInspectionInput) =>
    input.enabled ? nowMicros + (input.schedule.type === 'interval' ? input.schedule.everyMinutes : 1_440) * MINUTE : null;

  app.get('/api/v1/logs/inspections', (_req, res) => {
    res.json([...inspections].sort((a, b) => a.name.localeCompare(b.name)).map(shown));
  });

  // Before `/:id`, which would otherwise take "limits" for an inspection.
  app.get('/api/v1/logs/inspections/limits', (_req, res) => res.json(limits));

  app.post('/api/v1/logs/inspections', (req: Request, res: Response) => {
    const input = req.body as LogInspectionInput;

    if (inspections.length >= limits.maxPerOrg) {
      return res.status(409).json({
        error: `conflict: this organization already has ${inspections.length} scheduled log inspections; the limit is ${limits.maxPerOrg}`,
      });
    }
    if (inspections.some((item) => item.name === input.name)) {
      return res
        .status(409)
        .json({ error: 'conflict: a scheduled log inspection with this name already exists' });
    }

    serial += 1;

    const saved = inspection(nowMicros, {
      ...input,
      id: `2zNewInspection${String(serial).padStart(12, '0')}`,
      nextRunAt: nextRun(input),
      createdAt: nowMicros,
      updatedAt: nowMicros,
    });

    inspections.push(saved);
    return res.json(shown(saved));
  });

  app.get('/api/v1/logs/inspections/:id', (req, res) => {
    const item = find(req.params.id);
    return item ? res.json(shown(item)) : missing(res);
  });

  app.put('/api/v1/logs/inspections/:id', (req: Request, res: Response) => {
    const item = find(req.params.id);
    if (!item) return missing(res);

    const input = req.body as LogInspectionInput;

    // Saving hands the inspection to the caller, who is Dana in the mock session.
    Object.assign(item, input, {
      run_as_user_id: DANA.id,
      run_as_user: DANA,
      next_run_at: nextRun(input),
      updated_at: nowMicros,
    });
    return res.json(shown(item));
  });

  app.post('/api/v1/logs/inspections/:id/enabled', (req: Request, res: Response) => {
    const item = find(req.params.id);
    if (!item) return missing(res);

    item.enabled = Boolean((req.body as { enabled?: unknown }).enabled);
    item.nextRunAt = item.enabled ? nowMicros + HOUR : null;
    return res.json(shown(item));
  });

  app.delete('/api/v1/logs/inspections/:id', (req, res) => {
    if (!find(req.params.id)) return missing(res);

    inspections = inspections.filter((item) => item.id !== req.params.id);
    runs = runs.filter((item) => item.inspectionId !== req.params.id);
    return res.json({ deleted: true });
  });

  app.post('/api/v1/logs/inspections/:id/run', (req, res) => {
    const item = find(req.params.id);
    if (!item) return missing(res);
    if (activeRun(item.id)) {
      return res
        .status(409)
        .json({ error: 'conflict: this log inspection already has a run in progress' });
    }

    serial += 1;

    const queued = run(nowMicros, {
      id: `2zQueuedRun${String(serial).padStart(16, '0')}`,
      inspectionId: item.id,
      trigger: 'manual',
      status: 'queued',
      level: null,
      windowStart: nowMicros - item.windowSecs * SECOND,
      windowEnd: nowMicros,
      requestedBy: DANA.id,
      requestedByUser: DANA,
      createdAt: nowMicros,
      startedAt: null,
      finishedAt: null,
    });

    runs.push(queued);
    return res.json(queued);
  });

  app.get('/api/v1/logs/inspections/:id/runs', (req, res) => {
    if (!find(req.params.id)) return missing(res);

    const limit = Math.min(200, Math.max(1, Number(req.query.limit ?? 50) || 50));
    const before = Number(req.query.before ?? 0) || null;
    const listed = runs
      .filter((item) => item.inspectionId === req.params.id)
      .filter((item) => before === null || item.createdAt < before)
      .sort((a, b) => b.createdAt - a.createdAt)
      .slice(0, limit)
      // A list carries no report.
      .map((item) => ({ ...item, report: null }));

    return res.json(listed);
  });

  app.get('/api/v1/logs/inspections/:id/runs/:runId', (req, res) => {
    const found = runs.find(
      (item) => item.inspectionId === req.params.id && item.id === req.params.runId,
    );

    return found
      ? res.json(found)
      : res.status(404).json({ error: 'not found: log inspection run' });
  });

  // What a test uses to stand in for the worker, and to set the scene.
  app.put('/api/v1/__mock/log-inspections', (req: Request, res: Response) => {
    const body = req.body as { inspections?: LogInspection[]; runs?: LogInspectionRun[] };

    inspections = Array.isArray(body.inspections) ? body.inspections : [];
    runs = Array.isArray(body.runs) ? body.runs : [];
    res.json({ inspections: inspections.length, runs: runs.length });
  });
  app.put('/api/v1/__mock/log-inspections/limits', (req: Request, res: Response) => {
    limits = { ...limits, ...(req.body as Partial<InspectionLimits>) };
    res.json(limits);
  });
  app.post('/api/v1/__mock/log-inspections/runs/:runId', (req: Request, res: Response) => {
    const found = runs.find((item) => item.id === req.params.runId);
    if (!found) return res.status(404).json({ error: 'not found: log inspection run' });

    Object.assign(found, req.body as Partial<LogInspectionRun>);

    // A run that ended is what its inspection last did.
    const owner = find(found.inspectionId);

    if (owner && found.finishedAt) {
      owner.lastRun = {
        atTime: found.finishedAt,
        status: found.status,
        level: found.level,
      };
    }

    return res.json(found);
  });
}

async function send(port: number, method: 'PUT' | 'POST', path: string, body: unknown) {
  const response = await fetch(`http://127.0.0.1:${port}/api/v1/__mock/log-inspections${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`mock log inspections were not changed: ${response.status}`);
}

/** Replaces the organization's inspections and runs, for a test about an empty or a full list. */
export function setMockLogInspections(
  port: number,
  inspections: readonly LogInspection[],
  runs: readonly LogInspectionRun[] = [],
): Promise<void> {
  return send(port, 'PUT', '', { inspections, runs });
}

/** Changes what the instance allows, for a test about its limits. */
export function setMockInspectionLimits(
  port: number,
  limits: Partial<InspectionLimits>,
): Promise<void> {
  return send(port, 'PUT', '/limits', limits);
}

/** Moves one run along the way the worker would, for a test about a run that starts and ends. */
export function updateMockInspectionRun(
  port: number,
  runId: string,
  change: Partial<LogInspectionRun>,
): Promise<void> {
  return send(port, 'POST', `/runs/${encodeURIComponent(runId)}`, change);
}
