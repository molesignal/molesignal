/**
 * Activation / Home e2e.
 *
 * The Home page derives an ActivationState from how much the org has set up
 * (streams / dashboards / alerts / schedules / connectors / model providers /
 * pipelines) and from what the reader may read and do. Previously only vitest
 * unit tests covered this; these specs assert it in a real browser:
 *   - an empty OSS org → activation prompt with all eight steps
 *   - an active org    → populated streams + the response steps still to do
 *   - real rows finish the steps, rows that could not deliver do not
 *   - sample data is for an org with nothing to look at, and never holds back
 *     one with streams of its own
 *   - a reader without a step's permission is neither reminded of it nor sends
 *     the request the backend would refuse
 *
 * Data is overridden per-test (rather than via the shared fixture) because the
 * fixture serves the `{ items }` envelope while `streamsApi.list` /
 * `dashboardsApi.list` expect bare arrays — overriding keeps this spec
 * self-contained and deterministic.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Page, Route } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const DATA = join(dirname(fileURLToPath(import.meta.url)), '../fixtures/data');
const readItems = (name: string): unknown[] =>
  (JSON.parse(readFileSync(join(DATA, name), 'utf8')) as { items: unknown[] }).items;

const STREAM_ITEMS = readItems('streams.json');
const DASHBOARD_ITEMS = readItems('dashboards.json');

type Json = unknown;

/** Everything a list behind a step answers. A list left out keeps the shared mock's answer. */
async function overrideHomeData(
  page: Page,
  data: {
    streams: Json[];
    dashboards: Json[];
    rules: Json[];
    schedules?: Json[];
    connectors?: Json[];
    providers?: Json[];
    pipelines?: Json[];
    sampleLoaded?: boolean;
  },
): Promise<void> {
  const getJson = (body: Json) => async (route: Route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({ json: body });
      return;
    }
    await route.fallback();
  };
  await page.route('**/api/v1/streams', getJson(data.streams));
  await page.route('**/api/v1/dashboards', getJson(data.dashboards));
  await page.route('**/api/v1/alerts/rules', getJson(data.rules));
  await page.route('**/api/v1/scheduled_pipelines', getJson(data.pipelines ?? []));
  if (data.schedules) await page.route('**/api/v1/schedules', getJson(data.schedules));
  await page.route('**/api/v1/notify/connectors', getJson(data.connectors ?? []));
  await page.route('**/api/v1/agent/settings/model-providers', getJson(data.providers ?? []));
  await page.route('**/api/v1/onboarding/sample-data', getJson({ loaded: data.sampleLoaded ?? false }));
  const overviewStreams = data.streams.map((stream, index) => {
    const item = stream as { id?: unknown; name?: unknown };
    const id = String(item.id ?? `stream-${index + 1}`);
    return {
      id,
      name: String(item.name ?? id),
      streamType: 'logs',
      status: 'healthy',
      rows: 1,
      storedBytes: 1,
      firstReceivedAt: 100,
      lastReceivedAt: 200,
    };
  });
  await page.route(
    '**/api/v1/home/overview**',
    getJson({
      generatedAt: 200,
      window: {
        startTime: 100,
        endTime: 200,
        windowSecs: 100,
      },
      intakeStatus: overviewStreams.length > 0 ? 'healthy' : 'no_data',
      probeReason: null,
      intakeBytes: overviewStreams.length,
      storedBytes: overviewStreams.length,
      rows: overviewStreams.length,
      compressionSavingsRatio: null,
      activeStreams: overviewStreams.length,
      totalStreams: overviewStreams.length,
      attentionStreams: 0,
      lastReceivedAt: overviewStreams.length > 0 ? 200 : null,
      statsProbe: { succeeded: 1, total: 1 },
      buckets: [],
      signals: [],
      streams: overviewStreams,
    }),
  );
  // Home maps the recent-activity feed; the shared catch-all returns `{}` (not
  // an array), which crashes `activity.map`. Serve an empty list explicitly.
  await page.route('**/api/v1/audit**', getJson([]));
}

const NOTHING_SET_UP = { streams: [], dashboards: [], rules: [], schedules: [] };

const CONNECTED = { id: 'c-1', name: 'Ops mail', connectorType: 'email_smtp', enabled: true, status: 'connected' };
const WITH_KEY = { id: 'p-1', name: 'OpenAI', provider: 'openai', defaultModel: 'gpt-5', enabled: true, keySet: true };
const SAMPLE_STREAMS = ['sample_app_logs', 'sample_app_metrics', 'sample_app_traces'].map((name) => ({ id: name, name }));

function setupDrawer(page: Page) {
  return page.getByRole('dialog', { name: 'Quick start' });
}

test.describe('Activation — empty OSS org', () => {
  test('Home prompts activation when nothing is set up', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');

    // No streams → the streams panel shows its empty state…
    await expect(page.getByText('No streams registered yet.')).toBeVisible({ timeout: 10_000 });
    // …and the compact activation strip reports every remaining setup step.
    await expect(page.getByText('8 setup steps remaining')).toBeVisible();

    // The list reads from data coming in to someone answering an alert.
    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('listitem')).toHaveText([
      /Connect a datasource/,
      /Load sample data/,
      /Create a dashboard/,
      /Create an alert/,
      /Set up an on-call schedule/,
      /Add a notification channel/,
      /Connect an AI model/,
      /Automate a pipeline/,
    ]);
  });
});

test.describe('Activation — active org', () => {
  test('Home shows populated streams and the response steps still to do', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, {
      streams: STREAM_ITEMS,
      dashboards: DASHBOARD_ITEMS,
      rules: [{ id: 'rule-1', name: 'p95', enabled: true }],
      schedules: [],
    });

    await page.goto('/home');

    // Streams render (so the empty state is gone) and the three core steps are
    // complete (streams + dashboards + alerts) → the strip leads on to on-call.
    await expect(page.getByText('No streams registered yet.')).toHaveCount(0);
    const strip = page.locator('[data-home-surface="activation"]');
    await expect(strip.getByText('4 setup steps remaining')).toBeVisible({ timeout: 10_000 });
    await expect(strip.getByText('Set up an on-call schedule')).toBeVisible();

    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByText('Core workflows are active')).toBeVisible();
  });
});

test.describe('Activation — sample data', () => {
  test('an org with streams of its own is not held back once the other seven are done', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    // The shared mock has schedules of its own.
    await overrideHomeData(page, {
      streams: STREAM_ITEMS,
      dashboards: DASHBOARD_ITEMS,
      rules: [{ id: 'rule-1', name: 'p95', enabled: true }],
      providers: [WITH_KEY],
      pipelines: [{ id: 'pipeline-1', name: 'Backfill' }],
    });

    await page.goto('/home');

    const strip = page.locator('[data-home-surface="activation"]');
    await expect(strip.getByText('1 setup step remaining')).toBeVisible({ timeout: 10_000 });
    await expect(strip.getByText('Add a notification channel')).toBeVisible();

    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('button', { name: /Load sample data/ })).toHaveCount(0);
    await page.keyboard.press('Escape');

    await page.route('**/api/v1/notify/connectors', (route) => route.fulfill({ json: [CONNECTED] }));
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(strip).toHaveCount(0);
  });

  test('a stream the platform writes neither connects a datasource nor takes it away', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, { ...NOTHING_SET_UP, streams: [{ id: 'audit', name: '_audit' }] });

    await page.goto('/home');

    const strip = page.locator('[data-home-surface="activation"]');
    await expect(strip.getByText('8 setup steps remaining')).toBeVisible({ timeout: 10_000 });
    await expect(strip.getByText('Connect a datasource')).toBeVisible();

    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('button', { name: /Load sample data/ })).toBeVisible();
  });

  test('an empty org loads it from the list, and the step then reads as done', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);
    let loaded = false;
    await page.route('**/api/v1/onboarding/sample-data', (route) => {
      if (route.request().method() === 'POST') loaded = true;
      return route.fulfill({ json: { loaded, totalRows: 104, streams: [] } });
    });
    await page.route('**/api/v1/streams', (route) => route.fulfill({ json: loaded ? SAMPLE_STREAMS : [] }));

    await page.goto('/home');

    await expect(page.getByText('8 setup steps remaining')).toBeVisible({ timeout: 10_000 });
    await page.getByRole('button', { name: 'Complete setup' }).click();
    const step = setupDrawer(page).getByRole('button', { name: /Load sample data/ });
    await step.click();

    await expect(step).toContainText('Done');
    // The sample streams are streams, so they take the datasource step along.
    await expect(setupDrawer(page).getByRole('button', { name: /Connect a datasource/ })).toContainText('Done');
    expect(loaded).toBe(true);
  });

  test('an org that loaded it sees it done beside streams of its own', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, {
      ...NOTHING_SET_UP,
      streams: [...STREAM_ITEMS, ...SAMPLE_STREAMS],
      sampleLoaded: true,
    });

    await page.goto('/home');
    await page.getByRole('button', { name: 'Complete setup' }).click({ timeout: 10_000 });

    await expect(setupDrawer(page).getByRole('button', { name: /Load sample data/ })).toContainText('Done');
  });

  test('sample data loaded since the page opened finishes its step on refresh', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');
    await expect(page.getByText('8 setup steps remaining')).toBeVisible({ timeout: 10_000 });

    await page.route('**/api/v1/onboarding/sample-data', (route) => route.fulfill({ json: { loaded: true } }));
    await page.route('**/api/v1/streams', (route) => route.fulfill({ json: SAMPLE_STREAMS }));
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('6 setup steps remaining')).toBeVisible();
  });

  test('a status that does not answer leaves the steps uncounted', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);
    await page.route('**/api/v1/onboarding/sample-data', (route) =>
      route.fulfill({ status: 500, json: { message: 'unavailable' } }),
    );

    await page.goto('/home');

    await expect(page.getByText('No streams registered yet.')).toBeVisible({ timeout: 10_000 });
    await page.waitForLoadState('networkidle');
    await expect(page.locator('[data-home-surface="activation"]')).toHaveCount(0);
  });

  test('a reader who may not read the stream list is not offered it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'Editor',
      capabilityPermissions: ['streams.query', 'logs.write', 'schedules.read', 'schedules.create'],
    });
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');
    await page.getByRole('button', { name: 'Complete setup' }).click({ timeout: 10_000 });

    await expect(setupDrawer(page).getByRole('listitem')).toHaveText([/Set up an on-call schedule/]);
  });
});

test.describe('Activation — the response steps follow real data', () => {
  test('a schedule, a usable connector and a provider with a key finish them', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    // The shared mock has schedules of its own.
    await overrideHomeData(page, {
      streams: [],
      dashboards: [],
      rules: [],
      connectors: [CONNECTED],
      providers: [WITH_KEY],
    });

    await page.goto('/home');

    await expect(page.getByText('5 setup steps remaining')).toBeVisible({ timeout: 10_000 });

    await page.getByRole('button', { name: 'Complete setup' }).click();
    for (const step of ['Set up an on-call schedule', 'Add a notification channel', 'Connect an AI model']) {
      await expect(setupDrawer(page).getByRole('button', { name: new RegExp(step) })).toContainText('Done');
    }
  });

  test('a connector that could not deliver and a provider without a key do not', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, {
      ...NOTHING_SET_UP,
      connectors: [
        { ...CONNECTED, id: 'c-1', enabled: false },
        { ...CONNECTED, id: 'c-2', status: 'error' },
      ],
      providers: [
        { ...WITH_KEY, id: 'p-1', enabled: false },
        { ...WITH_KEY, id: 'p-2', keySet: false },
      ],
    });

    await page.goto('/home');

    await expect(page.getByText('8 setup steps remaining')).toBeVisible({ timeout: 10_000 });
  });

  test('a connector added since the page opened finishes its step on refresh', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');
    await expect(page.getByText('8 setup steps remaining')).toBeVisible({ timeout: 10_000 });

    await page.route('**/api/v1/notify/connectors', (route) => route.fulfill({ json: [CONNECTED] }));
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();

    await expect(page.getByText('7 setup steps remaining')).toBeVisible();
  });

  test('one list that does not answer leaves the steps uncounted', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, NOTHING_SET_UP);
    await page.route('**/api/v1/agent/settings/model-providers', (route) =>
      route.fulfill({ status: 500, json: { message: 'unavailable' } }),
    );

    await page.goto('/home');

    await expect(page.getByText('No streams registered yet.')).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('[data-home-surface="activation"]')).toHaveCount(0);
  });

  for (const [step, path] of [
    ['Set up an on-call schedule', '/alerts/schedules'],
    ['Add a notification channel', '/settings/notify/connectors'],
    ['Connect an AI model', '/agent/settings/models'],
  ] as const) {
    test(`"${step}" opens ${path}`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port);
      await overrideHomeData(page, NOTHING_SET_UP);

      await page.goto('/home');
      await page.getByRole('button', { name: 'Complete setup' }).click();
      await setupDrawer(page).getByRole('button', { name: new RegExp(step) }).click();

      await expect(page).toHaveURL(new RegExp(`${path}(?:[?#]|$)`));
      await expect(page.getByRole('alert')).toHaveCount(0);
    });
  }
});

test.describe('Activation — what the reader may read and do', () => {
  const REFUSED = [
    '/api/v1/schedules',
    '/api/v1/notify/connectors',
    '/api/v1/agent/settings/model-providers',
    '/api/v1/alerts/escalations',
    '/api/v1/teams',
  ];

  test('a reader without the permissions is not asked about the steps nor sends their requests', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'Editor',
      capabilityPermissions: [
        'streams.read',
        'streams.query',
        'logs.write',
        'dashboards.read',
        'dashboards.create',
        'alerts.rules.read',
        'alerts.rules.manage',
        'pipelines.read',
        'pipelines.create',
      ],
    });
    await overrideHomeData(page, { streams: [], dashboards: [], rules: [] });
    const asked: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (REFUSED.includes(pathname)) asked.push(pathname);
    });

    await page.goto('/home');

    await expect(page.getByText('5 setup steps remaining')).toBeVisible({ timeout: 10_000 });
    // The on-call card has nothing to say to someone who may not read schedules.
    await expect(page.getByText('Could not load the schedules')).toHaveCount(0);
    await expect(page.getByText('No on-call schedule configured')).toHaveCount(0);

    // A refresh runs a query whatever its `enabled` says, so it has to leave these out itself.
    await Promise.all([
      page.waitForRequest('**/api/v1/home/overview**'),
      page.getByRole('button', { name: 'Refresh', exact: true }).click(),
    ]);
    await page.waitForLoadState('networkidle');
    expect(asked).toEqual([]);

    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('listitem')).toHaveText([
      /Connect a datasource/,
      /Load sample data/,
      /Create a dashboard/,
      /Create an alert/,
      /Automate a pipeline/,
    ]);
  });

  test('a role made of the model permissions is led to add a provider, and can', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'Viewer',
      // `streams.query` is what the route of Home itself asks for on a real backend.
      capabilityPermissions: ['streams.query', 'agent.use', 'agent.models.read', 'agent.models.manage'],
    });
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');

    await expect(page.getByText('1 setup step remaining')).toBeVisible({ timeout: 10_000 });
    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('listitem')).toHaveText([/Connect an AI model/]);

    await setupDrawer(page).getByRole('button', { name: /Connect an AI model/ }).click();

    await expect(page).toHaveURL(/\/agent\/settings\/models(?:[?#]|$)/);
    await expect(page.getByRole('button', { name: 'Add model provider' })).toBeEnabled();
  });

  test('a role without the permission that adds a provider is not led there', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'Viewer',
      capabilityPermissions: [
        'streams.read',
        'streams.query',
        'logs.write',
        'agent.use',
        'agent.models.read',
        'agent.profiles.manage',
      ],
    });
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');

    await expect(page.getByText('2 setup steps remaining')).toBeVisible({ timeout: 10_000 });
    await page.getByRole('button', { name: 'Complete setup' }).click();
    await expect(setupDrawer(page).getByRole('listitem')).toHaveText([
      /Connect a datasource/,
      /Load sample data/,
    ]);
  });

  test('a read-only reader is not reminded of steps only someone else can do', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });
    await overrideHomeData(page, NOTHING_SET_UP);

    await page.goto('/home');

    await expect(page.getByText('No streams registered yet.')).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('[data-home-surface="activation"]')).toHaveCount(0);
  });
});
