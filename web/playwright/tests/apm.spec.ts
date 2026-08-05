import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import {
  expect,
  FROZEN_NOW_ISO,
  mountMockRoutes,
  test,
} from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const RANGE = { from: NOW - 3_600_000_000, to: NOW };
const SERVICE = {
  namespace: 'shop',
  name: 'checkout',
  environment: 'prod',
};
const TRACE_ID = '0123456789abcdef0123456789abcdef';
const SPAN_ID = '0123456789abcdef';

const meta = {
  range: RANGE,
  resolution: 'minute',
  projection_started_at: RANGE.from,
  last_complete_bucket_at: NOW - 30_000_000,
  data_quality: {
    partial: false,
    gaps: [],
    overflow_dimensions: [],
  },
  activation_boundary: false,
};

const traces = {
  namespace: SERVICE.namespace,
  service: SERVICE.name,
  environment: SERVICE.environment,
  version: '2.0.0',
  from: RANGE.from,
  to: RANGE.to,
};

function red(requestCount = 1_240, errorCount = 24, p95 = 420_000) {
  return {
    request_count: requestCount,
    error_count: errorCount,
    error_rate: requestCount === 0 ? 0 : errorCount / requestCount,
    duration_sum_micros: requestCount * 180_000,
    duration_average_micros: 180_000,
    p50_micros: 120_000,
    p95_micros: p95,
    p99_micros: p95 * 2,
    latency_partial: false,
    exemplars: [
      {
        trace_id: TRACE_ID,
        span_id: SPAN_ID,
        event_time: NOW - 60_000_000,
        duration_micros: p95,
        trace_available: true,
      },
    ],
  };
}

const service = {
  service: SERVICE,
  first_seen_at: RANGE.from - 86_400_000_000,
  last_seen_at: NOW - 5_000_000,
  instrumentation: {
    runtime_language: 'rust',
    telemetry_sdk_name: 'opentelemetry',
    telemetry_sdk_version: '0.29.0',
    recent_instance_count: 6,
  },
  versions: ['2.0.0', '1.0.0'],
  red: red(),
  health: 'warning',
  traces,
};

const transaction = {
  service: SERVICE,
  version: '2.0.0',
  transaction: { name: 'POST /orders', kind: 'http' },
  red: red(880, 18, 510_000),
  total_time_micros: 158_400_000,
  traces: { ...traces, transaction: 'POST /orders' },
};

const dependency = {
  service: SERVICE,
  version: '2.0.0',
  dependency: {
    category: 'database',
    target: 'postgres/orders',
    operation: 'SELECT',
  },
  red: red(760, 7, 240_000),
  total_time_micros: 98_800_000,
  traces: { ...traces, dependency: 'postgres/orders' },
};

const error = {
  error: {
    fingerprint: 'fp-checkout',
    error_type: 'PaymentDeclined',
    application_frame: 'checkout::payment',
    transaction_name: 'POST /orders',
    overflow: false,
  },
  service: SERVICE,
  first_seen_at: RANGE.from,
  last_seen_at: NOW - 20_000_000,
  occurrence_count: 24,
  representative_message: 'Payment provider rejected the request',
  red: red(24, 24, 300_000),
  traces: { ...traces, error_fingerprint: 'fp-checkout' },
};

const versions = [
  {
    service: SERVICE,
    version: '2.0.0',
    first_seen_at: NOW - 1_800_000_000,
    last_seen_at: NOW,
    observation_count: 720,
  },
  {
    service: SERVICE,
    version: '1.0.0',
    first_seen_at: RANGE.from,
    last_seen_at: NOW - 1_800_000_000,
    observation_count: 520,
  },
];

const trend = [
  { bucket_at: NOW - 120_000_000, red: red(90, 1, 250_000) },
  { bucket_at: NOW - 60_000_000, red: red(110, 3, 420_000) },
];

const responses: Record<string, unknown> = {
  '/api/v1/apm/overview': {
    meta,
    red: red(),
    trend,
    service_health: {
      healthy: 4,
      warning: 1,
      critical: 0,
      no_traffic: 1,
    },
    services: [service],
    top_transactions: [transaction],
    top_dependencies: [dependency],
    top_errors: [error],
    recent_versions: versions,
  },
  '/api/v1/apm/services': {
    meta,
    items: [service],
    total: 1,
    sort: 'request_count',
  },
  '/api/v1/apm/services/checkout': {
    meta,
    service,
    red: service.red,
    trend,
    transactions: [transaction],
    dependencies: [dependency],
    errors: [error],
    versions,
  },
  '/api/v1/apm/transactions': {
    meta,
    items: [transaction],
    total: 1,
    sort: 'total_time',
  },
  '/api/v1/apm/transactions/POST%20%2Forders': {
    meta,
    transaction,
    trend,
    errors: [error],
    versions,
  },
  '/api/v1/apm/dependencies': {
    meta,
    items: [dependency],
    total: 1,
    sort: 'total_time',
  },
  '/api/v1/apm/errors': {
    meta,
    items: [error],
    total: 1,
    sort: 'occurrence_count',
  },
  '/api/v1/apm/errors/fp-checkout': {
    meta,
    group: error,
    trend,
    affected_transactions: [transaction],
    affected_versions: ['1.0.0', '2.0.0'],
    representative_stack: [
      'checkout::payment::authorize',
      'checkout::orders::submit',
    ],
    samples: [
      {
        event_time: NOW - 20_000_000,
        trace_id: TRACE_ID,
        span_id: SPAN_ID,
        trace_available: true,
        representative_message: error.representative_message,
        representative_stack: ['checkout::payment::authorize'],
      },
    ],
  },
  '/api/v1/apm/versions/compare': {
    meta,
    baseline: { version: '1.0.0', sample_count: 520, red: red(520, 4, 220_000) },
    candidate: { version: '2.0.0', sample_count: 720, red: red(720, 18, 510_000) },
    sufficient_data: true,
    status: 'regressed',
    delta: {
      request_count_absolute: 200,
      request_count_relative: 0.3846,
      error_rate_absolute: 0.0173,
      error_rate_relative: 2.25,
      p95_absolute_micros: 290_000,
      p95_relative: 1.318,
    },
    regressed_transactions: [transaction],
    regressed_errors: [error],
  },
  '/api/v1/apm/health': {
    meta,
    enabled: true,
    degraded: false,
    runtime: { queue_depth: 0 },
  },
};

async function installApmRoutes(page: Page): Promise<void> {
  await page.route('**/api/v1/apm/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    const body = responses[path];
    if (!body) {
      await route.fulfill({ status: 404, json: { error: `No APM fixture for ${path}` } });
      return;
    }
    await route.fulfill({ json: body });
  });
  await page.route('**/api/v1/debug-artifacts', (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route('**/api/v1/rum/sessions/*/related-traces', (route) =>
    route.fulfill({
      json: {
        session_id: 'demo',
        primary_service: SERVICE.name,
        traces: [
          {
            trace_id: TRACE_ID,
            service: SERVICE.name,
            span_count: 8,
            duration_ms: 420,
            started_at_micros: NOW - 120_000_000,
            relation: 'direct',
          },
        ],
      },
    }),
  );
  await page.route('**/api/v1/rum/replay/*', (route) =>
    route.fulfill({
      json: { session_id: 'demo', segment_count: 0, events: [] },
    }),
  );
  await page.route('**/api/v1/query', (route) => {
    const request = route.request().postDataJSON() as {
      stream?: { name?: string };
    };
    if (request.stream?.name === 'rum_sessions') {
      return route.fulfill({
        json: {
          columns: [
            'session_id',
            'user_id',
            'application',
            'environment',
            'version',
            'duration_ms',
            'started_at_micros',
            'error_count',
          ],
          rows: [
            [
              'demo',
              'user-7',
              'storefront',
              'prod',
              '2.0.0',
              12_000,
              NOW - 120_000_000,
              1,
            ],
          ],
          scanned_rows: 1,
          took_ms: 1,
        },
      });
    }
    if (request.stream?.name === 'rum_actions') {
      return route.fulfill({
        json: {
          columns: [
            'session_id',
            'ts_micros',
            'type',
            'url',
            'trace_id',
            'service',
            'duration_ms',
            'status',
            'payload',
          ],
          rows: [
            [
              'demo',
              NOW - 100_000_000,
              'network_error',
              '/checkout',
              TRACE_ID,
              SERVICE.name,
              420,
              500,
              {},
            ],
          ],
          scanned_rows: 1,
          took_ms: 1,
        },
      });
    }
    return route.fulfill({
      json: { columns: [], rows: [], scanned_rows: 0, took_ms: 1 },
    });
  });
}

async function boot(
  page: Page,
  port: number,
  theme: 'light' | 'dark' = 'light',
): Promise<void> {
  await mountMockRoutes(page, port, { theme });
  await installApmRoutes(page);
}

test('APM routes render aggregates, drill-downs, and safe topology', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  const routes: Array<[string, string]> = [
    ['/apm/overview', 'Application performance'],
    ['/apm/services', 'Services'],
    ['/apm/services/checkout?namespace=shop&environment=prod', 'checkout'],
    ['/apm/services/checkout/runtime?namespace=shop&environment=prod', 'checkout'],
    ['/apm/transactions', 'Transactions'],
    [
      '/apm/transactions/POST%20%2Forders?namespace=shop&service=checkout&environment=prod&kind=http',
      'POST /orders',
    ],
    ['/apm/errors', 'Backend errors'],
    ['/apm/errors/fp-checkout', 'PaymentDeclined'],
    [
      '/apm/deployments?service=checkout&baseline=1.0.0&candidate=2.0.0',
      'Deployments',
    ],
  ];
  for (const [path, heading] of routes) {
    await page.goto(path);
    await expect(page.getByRole('heading', { name: heading, exact: true }).first()).toBeVisible();
  }

  await page.goto('/apm/dependencies');
  await expect(page.getByRole('heading', { name: 'Dependencies', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Topology' }).click();
  await expect(page.getByText('postgres/orders')).toBeVisible();

  await page.goto('/apm/user-experience/overview');
  await expect(page).toHaveURL(/\/rum\/overview$/);
  await expect(page.getByRole('link', { name: 'Applications' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Session replay' })).toBeVisible();
});

test('transaction ranking opens an analysis workbench instead of skipping to Trace search', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/transactions');
  await page.getByText('POST /orders', { exact: true }).click();

  await expect(page).toHaveURL(/\/apm\/transactions\/POST%20%2Forders/);
  await expect(
    page.getByText(
      'Transactions are independently analyzable application operations such as endpoints, jobs, and message handlers.',
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Top errors' })).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Related deployments' }),
  ).toBeVisible();
  await expect(
    page.locator('main').getByRole('link', { name: 'Profiles' }),
  ).toBeVisible();
  await page.getByRole('link', { name: `Open Trace: ${TRACE_ID}` }).click();
  await expect(page).toHaveURL(
    new RegExp(`/traces/${TRACE_ID}\\?spanId=${SPAN_ID}$`),
  );
});

test('service-scoped overview replaces the redundant service ranking with investigation context', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  await page.goto(
    '/apm/overview?namespace=shop&service=checkout&environment=prod',
  );

  await expect(
    page.getByRole('heading', { name: 'Highest-impact Transactions' }),
  ).toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Highest-impact services' }),
  ).toHaveCount(0);
  await expect(page.getByText('POST /orders')).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Open service workbench' }),
  ).toHaveAttribute(
    'href',
    '/apm/services/checkout?namespace=shop&environment=prod',
  );
  await expect(page.getByText('Request throughput', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Latency' }).click();
  await expect(page.getByText('P99 latency', { exact: true }).first()).toBeVisible();
  await expect(
    page
      .getByRole('list', { name: 'Series legend' })
      .getByRole('listitem')
      .first(),
  ).toHaveCSS('font-size', '11px');
});

test('legacy RUM and service URLs preserve suffixes and query state', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/user-experience/sessions/view/demo?app=shop#event-2');
  await expect(page).toHaveURL(
    /\/rum\/sessions\/view\/demo\?app=shop#event-2$/,
  );
  const traceReference = page.locator('[data-signal-type="trace_id"]').first();
  await expect(traceReference).toBeVisible();
  await traceReference.click();
  const traceJump = page.getByRole('link', { name: /Open current trace/ });
  await expect(traceJump).toHaveAttribute('href', new RegExp(`trace_id.*${TRACE_ID}`));
  await expect(traceJump).toHaveAttribute('href', /from=.*to=/);

  await page.goto('/rum/source-maps?app=storefront');
  await expect(page).toHaveURL(
    /\/rum\/settings\/source-maps\?app=storefront$/,
  );
  await expect(page.getByRole('link', { name: 'SDK setup' })).toBeVisible();

  await page.goto(
    '/apm/versions/compare?service=checkout&baseline=1.0.0&candidate=2.0.0',
  );
  await expect(page).toHaveURL(
    /\/apm\/deployments\?service=checkout&baseline=1.0.0&candidate=2.0.0$/,
  );

  await page.goto('/services/checkout?environment=prod');
  await expect(page).toHaveURL(/\/apm\/services\/checkout\?environment=prod$/);
  await expect(page.getByRole('heading', { name: 'checkout', exact: true })).toBeVisible();
});

test('RUM keeps an independent analysis navigation and guides SDK activation', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  await page.unroute('**/api/v1/query');
  await page.route('**/api/v1/query', (route) =>
    route.fulfill({
      json: { columns: [], rows: [], scanned_rows: 0, took_ms: 1 },
    }),
  );

  await page.goto('/rum/overview');
  const main = page.locator('main');
  await expect(
    main.getByRole('heading', { name: 'User Experience', exact: true }),
  ).toBeVisible();
  await expect(
    main.getByRole('heading', {
      name: 'Start collecting real user experience data',
    }),
  ).toBeVisible();
  await expect(main.getByRole('link', { name: 'Applications' })).toBeVisible();
  await expect(main.getByRole('link', { name: 'Pages' })).toBeVisible();
  await expect(main.getByRole('link', { name: 'Session replay' })).toBeVisible();
  await expect(main.getByText('Install SDK', { exact: true })).toBeVisible();
  await expect(
    main.getByRole('link', { name: 'Web application' }),
  ).toBeVisible();
  await expect(
    main.getByRole('link', { name: 'Source Maps' }),
  ).toHaveCount(0);
});

test('APM keyboard navigation opens the canonical service catalog', async ({
  page,
  mockServer,
}) => {
  await boot(page, mockServer.port);
  await page.goto('/investigate');
  await page.keyboard.press('Meta+Alt+S');
  await expect(page).toHaveURL(/\/apm\/services$/);
  await expect(page.getByRole('heading', { name: 'Services', exact: true })).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  test(`APM overview is accessible in ${theme} theme`, async ({
    page,
    mockServer,
  }) => {
    await boot(page, mockServer.port, theme);
    await page.goto('/apm/overview');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.getByRole('heading', { name: 'Application performance' })).toBeVisible();
    const results = await new AxeBuilder({ page })
      .exclude('[role="status"]')
      .exclude('[aria-live]')
      .analyze();
    const critical = results.violations.filter(
      (violation) => violation.impact === 'critical',
    );
    expect(critical, JSON.stringify(critical, null, 2)).toEqual([]);
  });
}
