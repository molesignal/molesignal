import type { Page, Route } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Where a page is: its path and query, without the range the timed tabs carry in `time`. */
const place = (page: Page) => () => {
  const url = new URL(page.url());
  url.searchParams.delete('time');
  return `${url.pathname}${url.search}`;
};

const TABS = [
  ['Overview', '/synthetics/overview'],
  ['Checks', '/synthetics/checks'],
  ['Results', '/synthetics/results'],
  ['Locations', '/synthetics/locations'],
  ['Agents', '/synthetics/agents'],
  ['Variables', '/synthetics/variables'],
] as const;

test.describe('synthetics', () => {
  // The timed tabs open the module on a day, and the shell writes the range into the address about
  // 80 ms after the address last changed, on the path it had then. Links lead to addresses without
  // the range, so that write follows every step, and it can undo a click made before it. A step
  // that moves on waits for `?time=` first.
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps one title over six tabs, each with at most one action', async ({ page }) => {
    await page.goto('/synthetics');
    await expect(page).toHaveURL(/\/synthetics\/overview\?time=now-24h\.\.now$/);

    const tabs = page.getByTestId('synthetics-subnav');
    await expect(tabs.getByRole('link')).toHaveCount(TABS.length);

    for (const [name, path] of TABS) {
      await tabs.getByRole('link', { name, exact: true }).click();
      // The range goes with the reader, to the tabs that do not read it as well.
      await expect(page).toHaveURL(new RegExp(`${path}\\?time=now-24h\\.\\.now$`));
      // The module has one name; the tab that is open is said by the tabs.
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('Synthetics');
      await expect(tabs.getByRole('link', { name, exact: true })).toHaveAttribute(
        'aria-current',
        'page',
      );
      // The data follows the checks on its own: nothing asks to be refreshed by hand.
      await expect(page.getByRole('button', { name: 'Refresh' })).toHaveCount(0);
      const filled = await page
        .getByTestId('page-header')
        .locator('button')
        .evaluateAll((buttons) => buttons.length);
      expect(filled, `${name} offers more than one action`).toBeLessThanOrEqual(1);
    }
  });

  test('opens with what the checks say, then the checks that need attention', async ({ page }) => {
    await page.goto('/synthetics/overview');

    const standing = page.getByRole('region', { name: 'Synthetic checks now' });
    await expect(standing.getByRole('heading')).toHaveText('1 check is failing');
    await expect(standing).toContainText('10 checks running · 1 paused');
    // One statement, not a row of number cards.
    await expect(page.locator('[data-statement]')).toHaveCount(1);

    const attention = page.getByRole('region', { name: 'Needs attention' });
    await expect(attention.getByRole('link')).toHaveCount(3);
    // The worst first.
    await expect(attention.getByRole('link').first()).toContainText('Sign-in journey');

    await expect(page).toHaveURL(/\/synthetics\/overview\?time=now-24h\.\.now$/);
    await attention.getByRole('link', { name: 'Open Sign-in journey' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey(\?time=now-24h\.\.now)?$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Sign-in journey');
  });

  test('gives a check a page of its own, with its own tabs', async ({ page }) => {
    await page.goto('/synthetics/checks/mon-login-journey');

    // The module's tabs belong to the module's pages.
    await expect(page.getByTestId('synthetics-subnav')).toHaveCount(0);
    const tabs = page.getByTestId('synthetics-check-tabs');
    await expect(tabs.getByRole('link')).toHaveCount(4);

    const standing = page.getByRole('region', { name: 'The check now' });
    await expect(standing.getByRole('heading')).toHaveText('Failing');
    // The check runs from two places; each is one row, with how its runs went there.
    await expect(
      page.getByRole('region', { name: 'By location', exact: true }).locator('[data-check-location]'),
    ).toHaveCount(2);
    await expect(page.getByRole('button', { name: 'Run now' })).toBeVisible();

    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey\?time=now-24h\.\.now$/);
    await tabs.getByRole('link', { name: 'Results' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey\/results\?time=now-24h\.\.now$/);
    await page.getByRole('row').nth(1).click();
    await expect(page).toHaveURL(/\/results\/[^/?]+\?time=now-24h\.\.now$/);
    await expect(page.getByRole('dialog')).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey\/results\?time=now-24h\.\.now$/);
    await tabs.getByRole('link', { name: 'Configuration' }).click();
    await expect(page.getByRole('region', { name: 'Configuration' })).toContainText('Location policy');
  });

  test('offers the kinds of check as views of one list', async ({ page }) => {
    await page.goto('/synthetics/checks');

    const views = page.getByRole('group', { name: 'Kinds of check' });
    await expect(views.getByRole('button', { name: /^All/ })).toContainText('11');
    await expect(page.getByRole('row')).toHaveCount(12);
    // Each row draws its latest runs.
    await expect(page.locator('[data-run-strip]').first()).toBeVisible();

    await expect(page).toHaveURL(/\/synthetics\/checks\?time=now-24h\.\.now$/);
    await views.getByRole('button', { name: /^Browser/ }).click();
    await expect.poll(place(page)).toBe('/synthetics/checks?kind=browser');
    await expect(page.getByRole('row')).toHaveCount(2);
    await expect(page.getByRole('row').nth(1)).toContainText('Sign-in journey');
  });

  // One test for each address: a page load apiece, so no address waits on the
  // time the others took.
  for (const [gone, now] of [
    ['/synthetics/browser-tests', '/synthetics/checks?kind=browser'],
    ['/synthetics/network', '/synthetics/checks?kind=network'],
    ['/synthetics/schedules', '/synthetics/checks'],
    ['/synthetics/settings', '/synthetics/overview'],
    ['/synthetics/assertions', '/synthetics/checks/new'],
  ] as const) {
    test(`sends ${gone}, the address of a tab that is gone, to where its content is`, async ({
      page,
    }) => {
      await page.goto(gone);
      await expect.poll(place(page)).toBe(now);

      if (gone === '/synthetics/browser-tests') {
        await expect(page.getByRole('row')).toHaveCount(2);
      }
      if (gone === '/synthetics/assertions') {
        await expect(page.getByRole('dialog')).toBeVisible();
      }
    });
  }

  test('reads how the checks did over the range beside the tabs, a day unless told', async ({ page }) => {
    await page.goto('/synthetics/overview');

    const tabs = page.getByTestId('synthetics-subnav');
    await expect(tabs.getByRole('button', { name: /Last 24 hours/ })).toBeVisible();
    await expect.poll(() => new URL(page.url()).searchParams.get('time')).toBe('now-24h..now');

    const standing = page.getByRole('region', { name: 'Synthetic checks now' });
    await expect(standing).toContainText(/Availability\s*\d+(\.\d+)?%/);
    await expect(standing).toContainText(/of \d+ runs in range/);

    // The list of checks says how each did over the same range.
    await tabs.getByRole('link', { name: 'Checks', exact: true }).click();
    await expect(page.getByRole('columnheader', { name: 'Availability' })).toBeVisible();
    await expect(page.getByRole('row').nth(1)).toContainText(/\d+(\.\d+)?%/);
  });

  test('says whether a location is online, not whether it passed', async ({ page }) => {
    await page.goto('/synthetics/locations');

    const rows = page.getByRole('row');
    await expect(rows).toHaveCount(5);
    await expect(rows.nth(1)).toContainText('Online');
    await expect(rows.nth(3)).toContainText('Degraded');
    await expect(page.getByText('Passed', { exact: true })).toHaveCount(0);
  });

  test('asks each kind of check only about what its reply carries', async ({ page }) => {
    await page.goto('/synthetics/checks/new');

    const editor = page.getByRole('dialog');
    const sources = editor.getByRole('combobox', { name: 'Source' });

    // An HTTP check starts with a status code assertion.
    await expect(sources).toHaveCount(1);
    await expect(sources.first()).toHaveText(/Status code/);

    // HTTP reads its reply and the time each phase of the request took; a JSONPath names its path.
    await sources.first().click();
    await expect(page.getByRole('option')).toHaveText([
      'Status code',
      'Body',
      'JSONPath',
      'Header',
      'Duration (ms)',
      'Response size (bytes)',
      'DNS time (ms)',
      'Connect time (ms)',
      'TLS time (ms)',
      'First byte time (ms)',
    ]);
    await page.getByRole('option', { name: 'JSONPath', exact: true }).click();
    await expect(editor.getByRole('textbox', { name: 'JSONPath', exact: true })).toBeVisible();
    await expect(editor.getByRole('combobox', { name: 'HTTP version' })).toHaveText(/Negotiate/);

    // A DNS check has no status code: the assertion goes, and DNS offers its own values.
    await editor.getByRole('combobox', { name: 'Check type' }).click();
    await page.getByRole('option', { name: 'DNS', exact: true }).click();
    await expect(sources).toHaveCount(0);

    await editor.getByRole('button', { name: 'Add assertion' }).click();
    await sources.first().click();
    await expect(page.getByRole('option')).toHaveText(['Answers', 'Answer count', 'Response code', 'Duration (ms)']);
    await page.keyboard.press('Escape');

    // The Shanghai office has no Agent online that runs gRPC checks, and says so.
    await editor.getByRole('combobox', { name: 'Check type' }).click();
    await page.getByRole('option', { name: 'gRPC', exact: true }).click();
    await expect(editor.getByText('cn-sha-office · Private · No online Agent here runs this kind of check')).toBeVisible();
  });

  test('shows the trace a run carried and the route a failed run traced', async ({ page }) => {
    await page.goto('/synthetics/results?outcome=failing');
    await expect(page).toHaveURL(/\/synthetics\/results\?outcome=failing&time=now-24h\.\.now$/);
    await page.getByRole('row').filter({ hasText: 'Payments gateway' }).first().click();

    const drawer = page.getByRole('dialog');
    const trace = drawer.getByRole('link', { name: 'View trace' });

    await expect(trace).toHaveAttribute('href', /^\/traces\/4bf92f3577b34da6a3ce929d0e0e[0-9a-f]{4}\?from=\d+&to=\d+$/);
    await expect(drawer.locator('[data-traceroute-hops]')).toHaveAttribute('data-traceroute-hops', '5');
    await expect(drawer.getByText('The target did not answer the traceroute')).toBeVisible();
  });

  test('leads from a run of a browser check to the RUM session its page recorded', async ({ page, mockServer }) => {
    await page.route(/\/api\/v1\/synthetics\/results(\?|$)/, async (route) => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `http://127.0.0.1:${mockServer.port}${url.pathname}${url.search}` });
      const answer = (await response.json()) as { items: Array<{ attempts: Array<{ metadata?: Record<string, string> }> }> };

      for (const result of answer.items) {
        for (const attempt of result.attempts) attempt.metadata = { ...attempt.metadata, 'rum.session_id': 'session-of-the-run' };
      }

      await route.fulfill({ response, json: answer });
    });

    await page.goto('/synthetics/results?outcome=failing');
    await page.getByRole('row').filter({ hasText: 'Payments gateway' }).first().click();

    const link = page.getByRole('dialog').getByRole('link', { name: 'Open the RUM session of this run' }).first();

    await expect(link).toHaveAttribute('href', /^\/rum\/sessions\/view\/session-of-the-run\?from=/);
  });

  test('says where a check gets no results, and why', async ({ page, mockServer }) => {
    const fromMock = (route: Route) => {
      const url = new URL(route.request().url());
      return route.fetch({ url: `http://127.0.0.1:${mockServer.port}${url.pathname}${url.search}` });
    };

    // Frankfurt has missed the last three runs of the search check: no Agent there was online.
    await page.route(/\/api\/v1\/synthetics\/monitors\/mon-search-api$/, async (route) => {
      const detail = await (await fromMock(route)).json();
      await route.fulfill({
        json: {
          ...detail,
          missingRuns: { noData: true, locations: [{ locationId: 'loc-eu-central', missedRuns: 3 }] },
        },
      });
    });
    await page.route(/\/api\/v1\/synthetics\/monitors\/mon-search-api\/results(\?.*)?$/, async (route) => {
      const runs = await (await fromMock(route)).json();
      const now = Date.now() * 1000;
      const skipped = {
        ...runs[0],
        id: 'res-search-skipped',
        locationId: 'loc-eu-central',
        agentId: undefined,
        scheduledAt: now - 60_000_000,
        startedAt: now,
        finishedAt: now,
        receivedAt: now,
        outcome: 'skipped',
        attempts: [],
        assertions: [],
        metadata: { skip_reason: 'no_agent_online' },
      };
      await route.fulfill({ json: [skipped, ...runs] });
    });

    await page.goto('/synthetics/checks/mon-search-api');

    const standing = page.locator('[data-check-standing="no_data"]');
    await expect(standing.getByText('No results', { exact: true })).toBeVisible();
    await expect(standing.getByText('Frankfurt missed the last 3 runs')).toBeVisible();
    await expect(standing.getByText('Not run: no Agent at this location was online.')).toBeVisible();

    const frankfurt = page.locator('[data-check-location="loc-eu-central"]');
    await expect(frankfurt.getByText('Missed the last 3 runs')).toBeVisible();
    await expect(frankfurt.getByText('No results')).toBeVisible();

    // The list says so too, in place of the state its last runs left.
    await page.goto('/synthetics/checks');
    await expect(page.getByRole('row').filter({ hasText: 'Search API' }).getByText('No results')).toBeVisible();
  });

  test('says how to begin when there is no check', async ({ page }) => {
    await page.route(/\/api\/v1\/synthetics\/monitors(?:\?.*)?$/, (route) =>
      route.fulfill({ json: [] }),
    );
    await page.goto('/synthetics/overview');

    const firstRun = page.getByTestId('synthetics-first-run');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText(
      'Check your services from the outside',
    );
    // The first-run page carries the one way in; the header does not repeat it.
    await expect(page.getByRole('button', { name: 'Create check' })).toHaveCount(1);
    await expect(page).toHaveURL(/\/synthetics\/overview\?time=now-24h\.\.now$/);
    await firstRun.getByRole('link', { name: 'Certificate validity' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/new\?preset=tls(&time=now-24h\.\.now)?$/);
  });
});
