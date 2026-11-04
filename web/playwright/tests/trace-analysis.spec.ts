/**
 * What a trace is worked with beyond reading it: its critical path, a
 * comparison with another trace, the trace as a file, the RUM session it
 * served and what the profiler sampled in its spans. On the trace's page, in
 * the drawer over the trace list, and in the drawer an APM page opens.
 */
import { readFile } from 'node:fs/promises';

import type { Locator, Page, Request } from '@playwright/test';

import { installApmRoutes } from '../fixtures/mockApm';
import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

/** Room for a page to load: a full load from the dev server takes several seconds when the suite runs. */
const LOADED = { timeout: 15_000 };

const START_NS = (Date.parse(FROZEN_NOW_ISO) - 10 * 60 * 1000) * 1_000_000;

type Shape = Array<[id: string, parent: string | null, service: string, operation: string, fromMs: number, toMs: number, attributes?: Record<string, unknown>]>;

/**
 * A checkout: two queries of its own, an order call that looks up stock, and an
 * audit message sent without waiting for it while the order call runs.
 */
const CHECKOUT: Shape = [
  ['request', null, 'web', 'GET /checkout', 0, 120, { 'http.response.status_code': 200 }],
  ['carts', 'request', 'web', 'SELECT carts', 4, 14],
  ['users', 'request', 'web', 'SELECT users', 16, 22],
  ['audit', 'request', 'audit', 'publish audit', 25, 60],
  ['orders', 'request', 'orders-api', 'POST /orders', 30, 100],
  ['order-rows', 'orders', 'orders-api', 'SELECT orders', 34, 50],
  ['stock', 'orders', 'inventory', 'GET /stock', 40, 90],
];

/** The same checkout, slower: the stock lookup takes longer, a fraud check follows it, and no audit or users query. */
const SLOWER: Shape = [
  ['request', null, 'web', 'GET /checkout', 0, 260, { 'http.response.status_code': 200 }],
  ['carts', 'request', 'web', 'SELECT carts', 4, 14],
  ['orders', 'request', 'orders-api', 'POST /orders', 30, 240],
  ['order-rows', 'orders', 'orders-api', 'SELECT orders', 34, 50],
  ['stock', 'orders', 'inventory', 'GET /stock', 40, 200],
  ['fraud', 'orders', 'fraud', 'POST /score', 200, 236],
  ['rules', 'fraud', 'fraud', 'load rules', 202, 210],
];

/** The trace of a shape. `calledBy` is a span outside it that its root names as its parent: a browser's request, say. */
function trace(traceId: string, shape: Shape, rootAttributes: Record<string, unknown> = {}, calledBy?: string) {
  return {
    traceId: traceId,
    rootSpanId: shape[0]![0],
    truncated: false,
    spans: shape.map(([spanId, parent, service, operation, from, to, attributes], index) => {
      const parentId = parent ?? (index === 0 ? calledBy : undefined);

      return {
        spanId: spanId,
        ...(parentId ? { parentSpanId: parentId } : {}),
        service,
        operation,
        startNs: START_NS + from * 1_000_000,
        endNs: START_NS + to * 1_000_000,
        durationNs: (to - from) * 1_000_000,
        kind: index === 0 ? 2 : 3,
        status: 'OK',
        resource: { attributes: { 'service.name': service }, droppedAttributesCount: 0 },
        scope: { name: `${service}-instrumentation`, version: '1.0.0', attributes: {}, droppedAttributesCount: 0 },
        attributes: { ...(attributes ?? {}), ...(index === 0 ? rootAttributes : {}) },
        events: [],
        links: [],
      };
    }),
  };
}

/** Serves every trace: those whose id begins with `slow` as the slower checkout, the rest as the checkout. */
async function serveTraces(page: Page, rootAttributes: Record<string, unknown> = {}, calledBy?: string) {
  await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) => {
    const traceId = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    return route.fulfill({ json: trace(traceId, traceId.startsWith('slow') ? SLOWER : CHECKOUT, rootAttributes, calledBy) });
  });
}

function profile(id: string, traceId: string, spanId: string) {
  return { id, service: 'inventory', profileType: 'cpu', timestamp: START_NS / 1_000, totalValue: 100, sampleCount: 420, durationNanos: 10_000_000_000, unsymbolized: false, traceId: traceId, spanId: spanId };
}

const FLAMEBEARER = {
  names: ['total', 'main', 'handleStock', 'encodeJSON', 'queryDB'],
  levels: [[0, 100, 0, 0], [0, 100, 5, 1], [0, 95, 15, 2], [5, 50, 50, 3, 0, 30, 30, 4]],
  numTicks: 100,
  maxSelf: 50,
  units: 'samples',
};

/** One CPU profile of the trace asked for, for each span named, and the flame graph any of them is drawn as. Returns what was asked. */
async function serveProfiles(page: Page, spans: readonly string[]) {
  const asked: URL[] = [];
  await page.route(/\/api\/v1\/profiles(\?.*)?$/, (route) => {
    const url = new URL(route.request().url());
    asked.push(url);
    const traceId = url.searchParams.get('traceId') ?? '';
    return route.fulfill({ json: { profiles: spans.map((spanId) => profile(`p-${spanId}`, traceId, spanId)) } });
  });
  await page.route(/\/api\/v1\/profiles\/flamegraph(\?.*)?$/, (route) => {
    asked.push(new URL(route.request().url()));
    return route.fulfill({ json: { flamebearer: FLAMEBEARER, truncated: false, profileCount: 1 } });
  });
  return asked;
}

const row = (scope: Page | Locator, spanId: string) => scope.locator(`[data-testid="trace-span-row"][data-span-id="${spanId}"]`);

async function openTrace(page: Page, path: string) {
  await page.goto(path);
  await expect(page.getByTestId('trace-span-row')).toHaveCount(CHECKOUT.length, LOADED);
}

test.describe('trace analysis', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the critical path is drawn over the waterfall and the flame graph, and says each service’s part of it', async ({ page }) => {
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    const toggle = page.getByRole('button', { name: 'Critical path' });
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expect(row(page, 'request')).not.toHaveAttribute('data-critical');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-pressed', 'true');

    // The order call ended last, and in it the stock lookup; the audit message and the order rows ran alongside.
    for (const spanId of ['request', 'carts', 'users', 'orders', 'stock']) {
      await expect(row(page, spanId)).toHaveAttribute('data-critical', 'true');
    }
    for (const spanId of ['audit', 'order-rows']) await expect(row(page, spanId)).toHaveAttribute('data-critical', 'false');
    await expect(row(page, 'stock').getByTestId('trace-critical-segment')).toHaveCount(1);
    // The request's own time is on the path in four stretches: before, between and after its calls.
    await expect(row(page, 'request').getByTestId('trace-critical-segment')).toHaveCount(4);
    await expect(row(page, 'audit').getByTestId('trace-critical-segment')).toHaveCount(0);

    const legend = page.getByTestId('trace-legend');
    await expect(legend).toHaveAttribute('data-legend', 'critical');
    await expect(legend.getByTestId('trace-critical-total')).toHaveText('Critical path 120 ms');
    await expect(legend.locator('[data-legend-service]')).toHaveText(['inventory42%', 'web42%', 'orders-api17%']);

    await page.getByRole('button', { name: 'Flame graph' }).click();
    await expect(page.locator('[data-testid="trace-flame-block"][data-span-id="audit"]')).toHaveAttribute('data-critical', 'false');
    await expect(page.locator('[data-testid="trace-flame-block"][data-span-id="stock"]')).toHaveAttribute('data-critical', 'true');

    await toggle.click();
    await expect(legend).toHaveAttribute('data-legend', 'work');
    await expect(page.locator('[data-testid="trace-flame-block"][data-span-id="stock"]')).not.toHaveAttribute('data-critical');
  });

  test('the trace downloads as the trace API returned it, and as OTLP JSON', async ({ page }) => {
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');
    const menu = page.getByTestId('trace-export');

    await menu.click();
    const [json] = await Promise.all([page.waitForEvent('download'), page.getByTestId('trace-export-json').click()]);
    expect(json.suggestedFilename()).toBe('trace-trace-a.json');
    expect(JSON.parse(await readFile((await json.path())!, 'utf8'))).toEqual(trace('trace-a', CHECKOUT));
    // The menu hands the focus back to its button a moment after it closes.
    await expect(menu).toBeFocused();

    await menu.click();
    const [otlp] = await Promise.all([page.waitForEvent('download'), page.getByTestId('trace-export-otlp').click()]);
    expect(otlp.suggestedFilename()).toBe('trace-trace-a.otlp.json');
    const exported = JSON.parse(await readFile((await otlp.path())!, 'utf8')) as {
      resourceSpans: Array<{
        resource: { attributes: Array<{ key: string; value: { stringValue?: string } }> };
        scopeSpans: Array<{ scope: { name: string }; spans: Array<Record<string, unknown>> }>;
      }>;
    };

    // One resource a service, its spans under the scope that recorded them.
    expect(exported.resourceSpans.map((resource) => resource.resource.attributes.find((item) => item.key === 'service.name')?.value.stringValue)).toEqual([
      'web',
      'audit',
      'orders-api',
      'inventory',
    ]);
    const inventory = exported.resourceSpans[3]!.scopeSpans[0]!;
    expect(inventory.scope.name).toBe('inventory-instrumentation');
    expect(inventory.spans[0]).toMatchObject({
      traceId: 'trace-a',
      spanId: 'stock',
      parentSpanId: 'orders',
      name: 'GET /stock',
      kind: 3,
      status: { code: 1 },
    });
    const stock = inventory.spans[0]!;
    expect(BigInt(stock.endTimeUnixNano as string) - BigInt(stock.startTimeUnixNano as string)).toBe(50_000_000n);
  });

  test('two traces picked in the list are compared, the first against the second', async ({ page }) => {
    await serveExplorerResults(page);
    await page.goto('/traces?tab=traces');
    await expectExplorerResults(page, 'traces');
    const rows = page.locator('[data-trace-row]');
    const [first, second] = [await rows.nth(1).getAttribute('data-trace-id'), await rows.nth(3).getAttribute('data-trace-id')];
    // The second trace picked is served as the slower checkout.
    await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) => {
      const traceId = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
      return route.fulfill({ json: trace(traceId, traceId === second ? SLOWER : CHECKOUT) });
    });

    await page.getByRole('button', { name: 'Pick two traces to compare' }).click();
    const bar = page.getByTestId('trace-compare-pick');
    await expect(bar).toContainText('Pick two traces to compare.');
    await expect(page.getByTestId('trace-compare-picked')).toBeDisabled();

    // A row picks its trace instead of opening it; picked again, it is put back.
    await rows.nth(1).click();
    await rows.nth(2).click();
    await rows.nth(2).click();
    await rows.nth(3).click();
    await expect(page.locator('[data-trace-drawer]')).toHaveCount(0);
    await expect(rows.nth(1)).toHaveAttribute('data-pick', 'A');
    await expect(rows.nth(3)).toHaveAttribute('data-pick', 'B');
    await expect(rows.nth(2)).not.toHaveAttribute('data-pick');
    await expect(rows.nth(3)).toHaveAttribute('aria-pressed', 'true');
    await expect(bar.locator('[data-picked-trace]')).toHaveCount(2);

    await page.getByTestId('trace-compare-picked').click();
    // The page goes on to keep its time range in the address as well.
    await expect(page).toHaveURL(new RegExp(`/traces/${first}\\?compare=${second}&from=\\d+&to=\\d+&compareFrom=\\d+&compareTo=\\d+(&time=|$)`));
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Trace comparison');

    const comparison = page.locator('[data-trace-compare]');
    await expect(comparison.getByTestId('trace-compare-duration-a')).toHaveText('120 ms', LOADED);
    await expect(comparison.getByTestId('trace-compare-duration-b')).toHaveText('260 ms');
    await expect(comparison.getByTestId('trace-compare-duration')).toContainText('B took longer than A:');
    await expect(comparison.getByTestId('trace-compare-duration')).toContainText('+140 ms');

    // By service, the biggest change first: the stock lookup took 110 ms more.
    const services = comparison.getByTestId('trace-compare-services');
    await expect(services.locator('[data-compare-row]').first()).toHaveAttribute('data-compare-row', 'inventory');
    await expect(services.locator('[data-compare-row="inventory"]')).toContainText('+110 ms');
    await expect(services.locator('[data-compare-row="fraud"]')).toContainText('new');
    await expect(services.locator('[data-compare-row="audit"]')).toContainText('Not in B');

    await comparison.getByRole('button', { name: /^By operation/ }).click();
    await expect(comparison.getByTestId('trace-compare-operations').locator('[data-compare-row]')).toHaveCount(9);

    await comparison.getByRole('button', { name: /^In one trace only/ }).click();
    const onlyA = comparison.getByTestId('trace-compare-only-a');
    const onlyB = comparison.getByTestId('trace-compare-only-b');
    await expect(onlyA.locator('[data-missing-call]')).toHaveCount(2);
    await expect(onlyA).toContainText('publish audit');
    await expect(onlyA).toContainText('SELECT users');
    await expect(onlyB.locator('[data-missing-call]')).toHaveCount(1);
    await expect(onlyB.locator('[data-missing-call]')).toContainText('POST /score');
    await expect(onlyB.locator('[data-missing-call]')).toContainText('and 1 span it called');
  });

  test('a trace is compared with another named by its id; the two swap, and the comparison ends on the first', async ({ page }) => {
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    await page.getByTestId('trace-compare-with').click();
    const field = page.getByTestId('trace-compare-with-input');
    await field.fill('trace-a');
    await expect(page.getByTestId('trace-compare-with-submit')).toBeDisabled();
    await field.fill('slow-b');
    await field.press('Enter');

    await expect(page).toHaveURL(/\/traces\/trace-a\?compare=slow-b&from=\d+&to=\d+(&time=|$)/);
    const comparison = page.locator('[data-trace-compare]');
    await expect(comparison.getByTestId('trace-compare-duration-b')).toHaveText('260 ms', LOADED);
    await expect(page.getByRole('navigation', { name: 'Breadcrumb' }).getByRole('listitem').last()).toHaveText('Trace comparison');

    await page.getByTestId('trace-compare-swap').click();
    await expect(page).toHaveURL(/\/traces\/slow-b\?compare=trace-a&compareFrom=\d+&compareTo=\d+(&time=|$)/);
    await expect(comparison.getByTestId('trace-compare-duration-a')).toHaveText('260 ms', LOADED);
    await expect(comparison.getByTestId('trace-compare-duration')).toContainText('B took less time than A:');

    await page.getByTestId('trace-compare-stop').click();
    // The page alone again; it goes on to keep its span and its time range in the address.
    await expect(page).toHaveURL((url) => url.pathname === '/traces/slow-b' && !url.searchParams.has('compare'));
    await expect(page.getByTestId('trace-span-row')).toHaveCount(SLOWER.length, LOADED);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Trace detail');
  });

  test('a span naming its RUM session links the trace to that session, over the hours it can have run', async ({ page }) => {
    const lookups: Request[] = [];
    page.on('request', (request) => {
      if (request.url().includes('/api/v1/rum/traces/')) lookups.push(request);
    });
    await serveTraces(page, { 'session.id': 'sess-named' });
    await openTrace(page, '/traces/trace-a');

    const jump = page.getByTestId('trace-jump-session');
    await expect(jump).toHaveText('User session');
    const href = new URL((await jump.getAttribute('href'))!, 'http://localhost');
    expect(href.pathname).toBe('/rum/sessions/view/sess-named');
    const [from, to] = [Date.parse(href.searchParams.get('from')!), Date.parse(href.searchParams.get('to')!)];
    expect(from).toBe(START_NS / 1_000_000 - 4 * 3_600_000);
    expect(to).toBeGreaterThan(START_NS / 1_000_000 + 120);
    // A session the spans name is not looked for.
    expect(lookups).toHaveLength(0);
  });

  test('a trace no span ties to a session is linked to the session whose request carried it', async ({ page }) => {
    const asked: URL[] = [];
    await page.route(/\/api\/v1\/rum\/traces\/[^/]+\/session(\?.*)?$/, (route) => {
      asked.push(new URL(route.request().url()));
      return route.fulfill({
        json: { traceId: 'trace-a', session: { sessionId: 'sess-found', timestamp: START_NS / 1_000, actionType: 'xhr', page: '/checkout' } },
      });
    });
    // The browser's request is the root's parent: a span the trace does not hold.
    await serveTraces(page, {}, 'browser-fetch');
    await openTrace(page, '/traces/trace-a');

    const jump = page.getByTestId('trace-jump-session');
    await expect(jump).toBeVisible();
    expect(asked[0]?.pathname).toBe('/api/v1/rum/traces/trace-a/session');
    // Looked for around the trace, in microseconds.
    expect(Number(asked[0]?.searchParams.get('from'))).toBe(START_NS / 1_000 - 60_000_000);

    await jump.click();
    await expect(page).toHaveURL(/\/rum\/sessions\/view\/sess-found\?/);
  });

  test('a trace no session made has no session to open, and one begun inside the backend is not looked for', async ({ page }) => {
    const lookups: string[] = [];
    page.on('request', (request) => {
      if (request.url().includes('/api/v1/rum/traces/')) lookups.push(request.url());
    });
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    await expect(page.getByTestId('trace-jump-logs')).toBeVisible();
    await expect(page.getByTestId('trace-jump-session')).toHaveCount(0);
    expect(lookups).toHaveLength(0);

    // Called from outside, by a request no session sent: looked for, and still none.
    await serveTraces(page, {}, 'gateway-call');
    await openTrace(page, '/traces/trace-b');
    await expect.poll(() => lookups.length).toBe(1);
    await expect(page.getByTestId('trace-jump-session')).toHaveCount(0);
  });

  test('a span with profile samples shows the functions that took them, and opens their flame graph', async ({ page }) => {
    const asked = await serveProfiles(page, ['stock']);
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    // The waterfall marks the span the samples kept, and the trace's Profiles are those that kept it.
    await expect(row(page, 'stock').getByTestId('trace-span-profiled')).toBeVisible();
    await expect(row(page, 'orders').getByTestId('trace-span-profiled')).toHaveCount(0);
    await expect(page.getByTestId('trace-jump-profiles')).toHaveAttribute('href', /^\/profiles\?traceId=trace-a&from=/);

    await row(page, 'stock').click();
    const panel = page.getByTestId('trace-span-panel');
    await panel.getByRole('button', { name: /^Profile/ }).click();
    const tab = panel.getByTestId('trace-span-profile');
    await expect(tab).toHaveAttribute('data-profile', 'linked');
    await expect(tab).toContainText('1 profile kept this span.');
    await expect(tab.getByTestId('trace-span-profile-top').locator('li').first()).toContainText('encodeJSON');
    await expect(tab.getByTestId('trace-span-profile-top').locator('li').first()).toContainText('50%');

    const flame = asked.find((url) => url.pathname.endsWith('/flamegraph'))!;
    expect(Object.fromEntries(flame.searchParams)).toMatchObject({ traceId: 'trace-a', spanId: 'stock', type: 'cpu' });

    await tab.getByTestId('trace-span-profile-open').click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('Profile of GET /stock');
    await expect(dialog.locator('[data-profile-view]')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);

    const profiles = new URL((await tab.getByTestId('trace-span-profile-page').getAttribute('href'))!, 'http://localhost');
    expect(profiles.pathname).toBe('/profiles');
    expect(profiles.searchParams.get('traceId')).toBe('trace-a');
    expect(profiles.searchParams.get('spanId')).toBe('stock');
  });

  test('the Profiles page opened from a span analyzes that span’s profiles, not the rest of its trace', async ({ page }) => {
    await serveProfiles(page, ['stock', 'carts']);
    const selections: string[][] = [];
    await page.route(/\/api\/v1\/profiles\/flamegraph\/selection(\?.*)?$/, (route) => {
      selections.push((route.request().postDataJSON() as { profileIds: string[] }).profileIds);
      return route.fulfill({ json: { flamebearer: FLAMEBEARER, truncated: false, profileCount: 1 } });
    });
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    await row(page, 'stock').click();
    const panel = page.getByTestId('trace-span-panel');
    await panel.getByRole('button', { name: /^Profile/ }).click();
    await panel.getByTestId('trace-span-profile-page').click();

    await expect(page).toHaveURL(/\/profiles\?traceId=trace-a&spanId=stock&/);
    await expect.poll(() => selections.at(-1)).toEqual(['p-stock']);
  });

  test('a span without profile samples offers the profiles of the minutes around it', async ({ page }) => {
    await serveProfiles(page, []);
    await serveTraces(page);
    await openTrace(page, '/traces/trace-a');

    // No profile kept the trace: its Profiles are those of the time it ran.
    await expect(page.getByTestId('trace-jump-profiles')).toHaveAttribute('href', /^\/profiles\?from=/);
    await row(page, 'carts').click();
    const panel = page.getByTestId('trace-span-panel');
    await panel.getByRole('button', { name: /^Profile/ }).click();
    await expect(panel.getByTestId('trace-span-profile')).toHaveAttribute('data-profile', 'none');

    const around = new URL((await panel.getByTestId('trace-span-profile-window').getAttribute('href'))!, 'http://localhost');
    expect(around.searchParams.get('traceId')).toBeNull();
    expect(Date.parse(around.searchParams.get('from')!)).toBe(START_NS / 1_000_000 + 4 - 5 * 60_000);
    expect(Date.parse(around.searchParams.get('to')!)).toBe(START_NS / 1_000_000 + 14 + 5 * 60_000);
  });

  test('the drawer over the trace list holds the same tools, and the critical path stays on from trace to trace', async ({ page }) => {
    await serveExplorerResults(page);
    await serveTraces(page);
    await page.goto('/traces?tab=traces');
    await expectExplorerResults(page, 'traces');

    await page.locator('[data-trace-row]').first().click();
    const drawer = page.locator('[data-trace-drawer]');
    await expect(drawer.getByTestId('trace-span-row')).toHaveCount(CHECKOUT.length, LOADED);
    await expect(drawer.getByTestId('trace-export')).toBeVisible();
    await expect(drawer.getByTestId('trace-compare-with')).toBeVisible();

    await drawer.getByRole('button', { name: 'Critical path' }).click();
    await expect(row(drawer, 'stock')).toHaveAttribute('data-critical', 'true');

    // The next trace of the list opens anew, with the path still drawn.
    await drawer.getByRole('button', { name: 'Next in the list (J)' }).click();
    await expect(drawer.getByRole('button', { name: 'Critical path' })).toHaveAttribute('aria-pressed', 'true', LOADED);
    await expect(row(drawer, 'stock')).toHaveAttribute('data-critical', 'true');
    await expect(row(drawer, 'audit')).toHaveAttribute('data-critical', 'false');
  });

  test('the drawer an APM page opens a trace in holds the same tools', async ({ page }) => {
    await installApmRoutes(page);
    await serveTraces(page);
    await page.goto('/apm/services/checkout/endpoints?namespace=shop&environment=prod');

    await page.locator('[data-apm-endpoint-row="POST /orders"]').click();
    await page.locator('[data-apm-drawer="endpoint"] [data-apm-slow-request]').click();
    const drawer = page.locator('[data-trace-drawer]');
    await expect(drawer.getByTestId('trace-span-row')).toHaveCount(CHECKOUT.length, LOADED);

    await drawer.getByRole('button', { name: 'Critical path' }).click();
    await expect(row(drawer, 'orders')).toHaveAttribute('data-critical', 'true');
    await expect(drawer.getByTestId('trace-legend')).toHaveAttribute('data-legend', 'critical');
    await expect(drawer.getByTestId('trace-export')).toBeVisible();
    await expect(drawer.getByTestId('trace-compare-with')).toBeVisible();

    await row(drawer, 'stock').click();
    await expect(drawer.getByTestId('trace-span-panel').getByRole('button', { name: /^Profile/ })).toBeVisible();
  });
});
