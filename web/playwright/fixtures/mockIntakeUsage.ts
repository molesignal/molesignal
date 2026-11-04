import type { Express, Request, Response } from 'express';

import type {
  BudgetListItem,
  IntakeBudget,
  SourceLabel,
  SourceUsage,
  UsageDimension,
  UsageOverview,
} from '../../src/api/intakeUsage';

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;
const HOUR_MICROS = 3_600_000_000;
const DAY_MICROS = 24 * HOUR_MICROS;

interface SeedSource {
  source_key: string;
  label: SourceLabel;
  budgetable: boolean;
  /** Per day of the window: a longer window takes in more. */
  bytes_per_day: number;
  events_per_day: number;
}

const label = (kind: SourceLabel['kind'], name: string, more: Partial<SourceLabel> = {}): SourceLabel => ({
  kind,
  subkind: null,
  name,
  detail: null,
  missing: false,
  ...more,
});

/** Three views of the same 90 GiB a day. */
const SEED: Record<UsageDimension, SeedSource[]> = {
  credential: [
    {
      source_key: 'api_token:tok-collector',
      label: label('api_token', 'otel-collector', { subkind: 'service_account', detail: 'mspat-c01ec70r9a' }),
      budgetable: true,
      bytes_per_day: 60 * GIB,
      events_per_day: 40_000_000,
    },
    {
      source_key: 'api_token:tok-checkout',
      label: label('api_token', 'checkout-prod', { subkind: 'personal', detail: 'mspat-ch3ck07x2m' }),
      budgetable: true,
      bytes_per_day: 20 * GIB,
      events_per_day: 15_000_000,
    },
    {
      source_key: 'connector:conn-edge',
      label: label('connector', 'Cloudflare edge', { subkind: 'cloudflare_logpush' }),
      budgetable: true,
      bytes_per_day: 8 * GIB,
      events_per_day: 5_000_000,
    },
    {
      source_key: 'user:u1',
      label: label('user', 'Dana Reyes', { detail: 'sre@example.com' }),
      budgetable: true,
      bytes_per_day: GIB,
      events_per_day: 1_000,
    },
    {
      source_key: 'internal:',
      label: label('internal', ''),
      budgetable: false,
      bytes_per_day: GIB,
      events_per_day: 800,
    },
    {
      source_key: 'api_token:tok-idle',
      label: label('api_token', 'ci-pipeline', { subkind: 'personal', detail: 'mspat-c1p1p3k1v8' }),
      budgetable: true,
      bytes_per_day: 0,
      events_per_day: 0,
    },
  ],
  stream: [
    {
      source_key: 'logs:app-logs',
      label: label('stream', 'app-logs', { subkind: 'logs' }),
      budgetable: true,
      bytes_per_day: 50 * GIB,
      events_per_day: 30_000_000,
    },
    {
      source_key: 'traces:api-traces',
      label: label('stream', 'api-traces', { subkind: 'traces' }),
      budgetable: true,
      bytes_per_day: 30 * GIB,
      events_per_day: 25_000_000,
    },
    {
      source_key: 'metrics:host-metrics',
      label: label('stream', 'host-metrics', { subkind: 'metrics' }),
      budgetable: true,
      bytes_per_day: 10 * GIB,
      events_per_day: 6_001_800,
    },
  ],
  service: [
    {
      source_key: 'checkout',
      label: label('service', 'checkout'),
      budgetable: true,
      bytes_per_day: 45 * GIB,
      events_per_day: 30_000_000,
    },
    {
      source_key: 'search',
      label: label('service', 'search'),
      budgetable: true,
      bytes_per_day: 25 * GIB,
      events_per_day: 20_000_000,
    },
    {
      source_key: '',
      label: label('service', ''),
      budgetable: false,
      bytes_per_day: 15 * GIB,
      events_per_day: 10_000_000,
    },
    {
      source_key: '*',
      label: label('service', '*'),
      budgetable: false,
      bytes_per_day: 5 * GIB,
      events_per_day: 1_001_800,
    },
  ],
};

interface StoredBudget {
  id: string;
  dimension: UsageDimension;
  sourceKey: string;
  period: 'day' | 'month';
  limitSize: number;
  warnPercent: number;
  notifyUserIds: string[];
}

/** One budget past its limit, one close to it, one with room to spare. */
const seedBudgets = (): StoredBudget[] => [
  {
    id: 'budget-checkout',
    dimension: 'credential',
    sourceKey: 'api_token:tok-checkout',
    period: 'day',
    limitSize: 10 * 1024,
    warnPercent: 80,
    notifyUserIds: ['u1'],
  },
  {
    id: 'budget-collector',
    dimension: 'credential',
    sourceKey: 'api_token:tok-collector',
    period: 'day',
    limitSize: 70 * 1024,
    warnPercent: 80,
    notifyUserIds: ['u1', 'u2'],
  },
  {
    id: 'budget-edge',
    dimension: 'credential',
    sourceKey: 'connector:conn-edge',
    period: 'month',
    limitSize: 1024 * 1024,
    warnPercent: 0,
    notifyUserIds: ['u1'],
  },
];

export interface MockIntakeUsageState {
  /** False: the rows cannot be read, so every figure is unknown. */
  available: boolean;
  /** The start of the oldest hour kept, in days before now; `null` for never. */
  metering_days_ago: number | null;
}

/** What a test can change: also whether the seeded budgets are there, which ring the bell. */
export type MockIntakeUsageChange = Partial<MockIntakeUsageState> & { seed_budgets?: boolean };

export function registerIntakeUsageRoutes(app: Express, nowIso: string): void {
  const nowMicros = Date.parse(nowIso) * 1000;
  // No budgets unless a test asks: they would ring the bell of every other spec.
  let budgets: StoredBudget[] = [];
  let state: MockIntakeUsageState = { available: true, metering_days_ago: 60 };
  let settings = { retentionDays: 90, maxServicesPerOrg: 1000 };
  let nextBudget = 1;

  const startedAt = () =>
    state.metering_days_ago === null ? null : nowMicros - state.metering_days_ago * DAY_MICROS;

  const periodStart = (period: StoredBudget['period']) => {
    const now = new Date(nowMicros / 1000);
    const start =
      period === 'day'
        ? Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())
        : Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1);
    return start * 1000;
  };

  const seedOf = (dimension: UsageDimension, key: string) =>
    SEED[dimension].find((source) => source.source_key === key);

  const budgetView = (budget: StoredBudget): IntakeBudget => {
    const start = periodStart(budget.period);
    const started = startedAt();
    const complete = started !== null && started <= start;
    const days = Math.max(1, Math.round((nowMicros - start) / DAY_MICROS));
    const perDay = seedOf(budget.dimension, budget.sourceKey)?.bytes_per_day ?? 0;
    const used = !state.available ? null : perDay * days;
    const limit = budget.limitSize * MIB;
    const ratio = used === null ? null : used / limit;

    return {
      ...budget,
      updatedAt: nowMicros,
      periodStart: start,
      usedBytes: used,
      usedComplete: complete,
      usedRatio: ratio,
      state:
        used === null
          ? null
          : used >= limit
            ? 'exceeded'
            : budget.warnPercent > 0 && used * 100 >= limit * budget.warnPercent
              ? 'warning'
              : 'ok',
    };
  };

  app.get('/api/v1/streams/usage', (req: Request, res: Response) => {
    const dimension = String(req.query.dimension ?? 'credential') as UsageDimension;
    const windowSecs = Number(req.query.windowSecs ?? 86_400);
    const days = windowSecs / 86_400;
    const windowStart = nowMicros - windowSecs * 1_000_000;
    const started = startedAt();
    const covered = started !== null && started <= windowStart;
    const seeded = SEED[dimension] ?? [];

    const bytes = (source: SeedSource) => source.bytes_per_day * days;
    const total = seeded.reduce((sum, source) => sum + bytes(source), 0);

    const sources: SourceUsage[] = seeded.map((source) => {
      const stored = budgets.find(
        (budget) => budget.dimension === dimension && budget.sourceKey === source.source_key,
      );

      const known = state.available && (covered || source.bytes_per_day > 0);

      return {
        sourceKey: source.source_key,
        label: source.label,
        budgetable: source.budgetable,
        intakeBytes: known ? bytes(source) : null,
        events: known ? source.events_per_day * days : null,
        complete: covered,
        share: known ? bytes(source) / total : null,
        budget: stored ? budgetView(stored) : null,
      };
    });

    const body: UsageOverview = {
      generatedAt: nowMicros,
      windowStart: windowStart,
      windowEnd: nowMicros,
      windowSecs: windowSecs,
      dimension,
      available: state.available,
      meteringStartedAt: started,
      totalBytes: state.available ? total : null,
      totalEvents: state.available
        ? seeded.reduce((sum, source) => sum + source.events_per_day * days, 0)
        : null,
      truncated: false,
      sources,
    };

    res.json(body);
  });

  app.get('/api/v1/streams/usage/detail', (req: Request, res: Response) => {
    const dimension = String(req.query.dimension) as UsageDimension;
    const key = String(req.query.sourceKey ?? '');
    const windowSecs = Number(req.query.windowSecs ?? 86_400);
    const bucketCount = Number(req.query.bucketCount ?? 24);
    const windowStart = nowMicros - windowSecs * 1_000_000;
    const seeded = seedOf(dimension, key);
    const totalBytes = (seeded?.bytes_per_day ?? 0) * (windowSecs / 86_400);
    const totalEvents = (seeded?.events_per_day ?? 0) * (windowSecs / 86_400);
    const width = (windowSecs * 1_000_000) / bucketCount;

    res.json({
      generatedAt: nowMicros,
      windowStart: windowStart,
      windowEnd: nowMicros,
      windowSecs: windowSecs,
      meteringStartedAt: startedAt(),
      intakeBytes: totalBytes,
      events: totalEvents,
      buckets: Array.from({ length: bucketCount }, (_, index) => ({
        startTime: windowStart + index * width,
        endTime: windowStart + (index + 1) * width,
        intakeBytes: Math.round((totalBytes / bucketCount) * (index % 2 === 0 ? 1.5 : 0.5)),
        events: Math.round(totalEvents / bucketCount),
      })),
      breakdowns: (['credential', 'stream', 'service'] as const)
        .filter((other) => other !== dimension)
        .map((other) => ({
          dimension: other,
          sources: SEED[other]
            .filter((source) => source.bytes_per_day > 0)
            .slice(0, 3)
            .map((source) => ({
              sourceKey: source.source_key,
              label: source.label,
              intakeBytes: Math.round((source.bytes_per_day / (90 * GIB)) * totalBytes),
              events: Math.round((source.events_per_day / 61_001_800) * totalEvents),
            })),
        })),
    });
  });

  app.get('/api/v1/streams/budgets', (_req: Request, res: Response) => {
    const items: BudgetListItem[] = budgets.map((budget) => ({
      ...budgetView(budget),
      label: seedOf(budget.dimension, budget.sourceKey)?.label ?? label('service', budget.sourceKey),
    }));

    res.json({ budgets: items });
  });

  app.post('/api/v1/streams/budgets', (req: Request, res: Response) => {
    const body = req.body as Omit<StoredBudget, 'id' | 'notifyUserIds'> & { notifyUserIds?: string[] };

    if (!(body.limitSize >= 1 && body.limitSize <= 1024 ** 3)) {
      return res.status(400).json({ error: 'limit_size must be between 1 and 1073741824 MiB' });
    }

    const existing = budgets.find(
      (budget) => budget.dimension === body.dimension && budget.sourceKey === body.sourceKey,
    );

    const saved: StoredBudget = {
      id: existing?.id ?? `budget-new-${nextBudget++}`,
      dimension: body.dimension,
      sourceKey: body.sourceKey,
      period: body.period,
      limitSize: body.limitSize,
      warnPercent: body.warnPercent,
      notifyUserIds: body.notifyUserIds ?? ['mock-user'],
    };

    budgets = [...budgets.filter((budget) => budget.id !== saved.id), saved];
    return res.json(budgetView(saved));
  });

  app.delete('/api/v1/streams/budgets/:id', (req: Request, res: Response) => {
    budgets = budgets.filter((budget) => budget.id !== req.params.id);
    res.status(204).end();
  });

  app.get('/api/v1/settings/intake_usage', (_req: Request, res: Response) => res.json(settings));

  app.put('/api/v1/settings/intake_usage', (req: Request, res: Response) => {
    settings = { ...settings, ...(req.body as typeof settings) };
    res.json(settings);
  });

  app.put('/api/v1/__mock/intake-usage', (req: Request, res: Response) => {
    const { seed_budgets: seed, ...change } = req.body as MockIntakeUsageChange;
    state = { ...state, ...change };
    if (seed) budgets = seedBudgets();
    res.status(204).end();
  });
}

/** What the backend can tell about usage, for a test about figures that are not known. */
export async function setMockIntakeUsage(port: number, change: MockIntakeUsageChange): Promise<void> {
  const response = await fetch(`http://127.0.0.1:${port}/api/v1/__mock/intake-usage`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(change),
  });

  if (!response.ok) throw new Error(`mock intake usage was not changed: ${response.status}`);
}
