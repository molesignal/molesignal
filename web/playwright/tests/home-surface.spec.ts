import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const DAY_MICROS = 86_400_000_000;

const EMPTY_OVERVIEW = {
  generated_at_micros: 200,
  window: { start_micros: 100, end_micros: 200, window_secs: 100 },
  intake_status: 'no_data',
  probe_reason: null,
  intake_bytes: 0,
  stored_bytes: 0,
  rows: 0,
  compression_savings_ratio: null,
  active_streams: 0,
  total_streams: 0,
  attention_streams: 0,
  last_received_at_micros: null,
  stats_probe: { succeeded: 1, total: 1 },
  buckets: [],
  signals: [],
  streams: [],
};

/** Two streams that stopped sending six days ago: the "everything is quiet" state. */
function staleFleet() {
  const now = Date.now() * 1000;
  const lastReceived = now - 6 * DAY_MICROS;
  const streams = [
    { id: 'logs-1', name: 'app_logs', stream_type: 'logs' },
    { id: 'metrics-1', name: 'http_requests_total', stream_type: 'metrics' },
  ];
  return {
    overview: {
      ...EMPTY_OVERVIEW,
      generated_at_micros: now,
      window: {
        start_micros: now - DAY_MICROS,
        end_micros: now,
        window_secs: 86_400,
      },
      total_streams: streams.length,
      attention_streams: streams.length,
      last_received_at_micros: lastReceived,
      streams: streams.map((stream) => ({
        ...stream,
        status: 'degraded',
        rows: 0,
        stored_bytes: 0,
        first_received_at_micros: lastReceived - DAY_MICROS,
        last_received_at_micros: lastReceived,
      })),
    },
    runtime: {
      generated_at_micros: now,
      window_start_micros: now - DAY_MICROS,
      window_end_micros: now,
      window_secs: 86_400,
      streams: streams.map((stream) => ({
        ...stream,
        status: 'interrupted',
        rows: 0,
        stored_bytes: 0,
        current_stored_bytes: 0,
        first_received_at_micros: lastReceived - DAY_MICROS,
        last_received_at_micros: lastReceived,
        stats_available: true,
        buckets: [],
      })),
    },
  };
}

/** `count` recent audit events, newest first. */
function auditEvents(count: number) {
  const now = Date.now() * 1000;
  return Array.from({ length: count }, (_, index) => ({
    id: `event-${index}`,
    org_id: 'org-1',
    actor_kind: 'user',
    actor_id: 'user-1',
    action: 'agent.tool.called',
    target_kind: 'chat',
    target_id: `chat-${index}`,
    payload: {},
    ts_micros: now - index * 60_000_000,
  }));
}

async function mockHome(
  page: Page,
  { overview, runtime }: { overview: unknown; runtime: unknown },
  { audit = 0 }: { audit?: number } = {},
) {
  await page.route('**/api/v1/home/overview**', (route) =>
    route.fulfill({ json: overview }),
  );
  await page.route('**/api/v1/streams/runtime**', (route) =>
    route.fulfill({ json: runtime }),
  );
  await page.route('**/api/v1/audit**', (route) =>
    route.fulfill({ json: { items: auditEvents(audit), next_cursor: null } }),
  );
}

test.describe('Home surface hierarchy', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('uses canvas gutters and functional-surface depth', async ({ page }) => {
    await mockHome(page, {
      overview: EMPTY_OVERVIEW,
      runtime: {
        generated_at_micros: 200,
        window_start_micros: 100,
        window_end_micros: 200,
        window_secs: 100,
        streams: [],
      },
    });
    await page.goto('/home');

    await expect(page.locator('[data-page-appearance="surface"]')).toBeVisible();

    const header = page.getByTestId('page-header');
    const headerAppearance = await header.evaluate((element) => {
      const styles = getComputedStyle(element);
      return {
        borderBottom: styles.borderBottomWidth,
        background: styles.backgroundColor,
      };
    });
    expect(headerAppearance.borderBottom).toBe('0px');

    // Bands (verdict, workspace, footer) sit 24px apart; cards inside a band sit 16px apart.
    const canvas = page.getByTestId('home-operations-canvas');
    await expect(canvas).not.toHaveClass(/border|divide/);
    expect(await canvas.evaluate((element) => getComputedStyle(element).rowGap)).toBe('24px');

    const hero = page.locator('[data-home-surface="hero"]');
    const metrics = page.locator('[data-home-surface="kpi"]');
    await expect(hero).toHaveCount(1);
    await expect(metrics).toHaveCount(3);
    for (const card of [hero, ...(await metrics.all())]) {
      const appearance = await card.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          borderTop: styles.borderTopWidth,
          borderRight: styles.borderRightWidth,
          borderBottom: styles.borderBottomWidth,
          borderLeft: styles.borderLeftWidth,
          radius: styles.borderRadius,
          shadow: styles.boxShadow,
        };
      });
      expect([
        appearance.borderTop,
        appearance.borderRight,
        appearance.borderBottom,
        appearance.borderLeft,
      ]).toEqual(['0px', '0px', '0px', '0px']);
      expect(appearance.radius).not.toBe('0px');
      expect(appearance.shadow).not.toBe('none');
    }

    const surfaceGrids = page.locator('[data-home-surface-grid]');
    await expect(surfaceGrids).toHaveCount(1);
    const gridAppearance = await surfaceGrids.first().evaluate((element) => {
      const styles = getComputedStyle(element);
      return {
        rowGap: styles.rowGap,
        columnGap: styles.columnGap,
        borderTop: styles.borderTopWidth,
      };
    });
    expect(gridAppearance.rowGap).toBe('16px');
    expect(gridAppearance.columnGap).toBe('16px');
    expect(gridAppearance.borderTop).toBe('0px');

    const sections = page.locator('[data-home-surface="section"]');
    await expect(sections).toHaveCount(4);
    for (const section of await sections.all()) {
      const appearance = await section.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          borderTop: styles.borderTopWidth,
          borderLeft: styles.borderLeftWidth,
          radius: styles.borderRadius,
        };
      });
      expect(appearance.borderTop).toBe('0px');
      expect(appearance.borderLeft).toBe('0px');
      expect(appearance.radius).not.toBe('0px');
    }

    const operationalContext = page.getByTestId('home-primary-operational-context');
    const contextAppearance = await operationalContext
      .locator(':scope > div')
      .evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          borderLeft: styles.borderLeftWidth,
          radius: styles.borderRadius,
          shadow: styles.boxShadow,
        };
      });
    expect(contextAppearance.borderLeft).toBe('0px');
    expect(contextAppearance.radius).not.toBe('0px');
    expect(contextAppearance.shadow).not.toBe('none');
  });

  test('leads with one verdict and its next step, then the evidence', async ({
    page,
  }) => {
    // A wide screen: the verdict strip should be a single short row there.
    await page.setViewportSize({ width: 1920, height: 1080 });
    await mockHome(page, staleFleet());
    await page.goto('/home');

    const hero = page.locator('[data-home-surface="hero"]');
    await expect(hero).toHaveAttribute('data-verdict', 'stale');
    await expect(hero.getByRole('heading', { name: 'Data is stale' })).toBeVisible();
    await expect(
      hero.getByText('All 2 streams have had no data for over 24 hours.'),
    ).toBeVisible();
    await expect(hero.getByRole('button', { name: 'Check data intake' })).toBeVisible();

    // A strip, not a banner: the verdict takes a glance, so it stays short.
    const heroBox = await hero.boundingBox();
    expect(heroBox).not.toBeNull();
    expect(heroBox!.height).toBeLessThanOrEqual(125);
    const metricBoxes = await Promise.all(
      (await page.locator('[data-home-surface="kpi"]').all()).map((card) => card.boundingBox()),
    );
    for (const box of metricBoxes) expect(box!.height).toBeLessThanOrEqual(125);

    // Reading order: verdict, then the workspace that backs it up.
    const workspaceBox = await page
      .locator('[data-home-surface-grid]')
      .first()
      .boundingBox();
    expect(heroBox).not.toBeNull();
    expect(workspaceBox).not.toBeNull();
    expect(heroBox!.y + heroBox!.height).toBeLessThanOrEqual(workspaceBox!.y);

    // The one primary action on the page is the verdict's; "New" is a quiet button.
    const heroCta = await hero
      .getByRole('button', { name: 'Check data intake' })
      .evaluate((element) => getComputedStyle(element).backgroundColor);
    const newButton = await page
      .getByRole('button', { name: 'New' })
      .first()
      .evaluate((element) => getComputedStyle(element).backgroundColor);
    expect(heroCta).not.toBe(newButton);
  });

  test('ends both workspace columns on the same line', async ({ page }) => {
    // No entrance animation, so nothing is mid-transform when the boxes are measured.
    await page.emulateMedia({ reducedMotion: 'reduce' });
    for (const viewport of [
      { width: 1920, height: 1080 },
      { width: 1280, height: 800 },
    ]) {
      await page.setViewportSize(viewport);
      // Few, some and more events than the card has room for: the columns line
      // up whatever the activity list (and the on-call card above it) holds.
      for (const audit of [0, 3, 8]) {
        await mockHome(page, staleFleet(), { audit });
        await page.goto('/home');
        await expect(page.locator('[data-home-surface="hero"]')).toBeVisible();
        await expect(page.getByTestId('home-primary-operational-context')).toBeVisible();

        const columns = await page
          .locator('[data-home-surface-grid] > *')
          .evaluateAll((elements) =>
            elements.map((element) => {
              const column = element.getBoundingClientRect();
              const last = element.lastElementChild?.getBoundingClientRect();
              return { bottom: column.bottom, lastBottom: last?.bottom ?? column.bottom };
            }),
          );
        expect(columns).toHaveLength(2);
        const label = `${viewport.width}px with ${audit} events`;
        expect(Math.abs(columns[0]!.bottom - columns[1]!.bottom), label).toBeLessThan(1);
        // The cards themselves reach the shared edge, not just their wrappers.
        expect(Math.abs(columns[0]!.lastBottom - columns[1]!.lastBottom), label).toBeLessThan(1);

        // Only whole rows are shown: none is cut by the edge of its card.
        const cut = await page.evaluate(() => {
          const rows = Array.from(document.querySelectorAll<HTMLElement>('[data-fit-row]'));
          return rows
            .filter((row) => getComputedStyle(row).visibility !== 'hidden')
            .filter((row) => {
              const edge = row.closest('section')!.getBoundingClientRect().bottom;
              return row.getBoundingClientRect().bottom > edge + 0.5;
            }).length;
        });
        expect(cut, label).toBe(0);
      }
    }
  });

  test('opens a stream from a real link and draws a single scrollbar', async ({
    page,
  }) => {
    await mockHome(page, staleFleet());
    await page.goto('/home');

    const link = page.getByRole('link', { name: 'app_logs' });
    await expect(link).toHaveAttribute('href', '/logs?stream=app_logs');

    // Identical conditions collapse to quiet text instead of a column of pills.
    const statusCell = page.locator('tbody tr').first().locator('td').nth(2);
    await expect(statusCell).toHaveText('Stale');

    // Rows are sized to fit, so the table never gains its own vertical scrollbar.
    const metrics = await page
      .getByTestId('home-top-streams-viewport')
      .locator(':scope > div')
      .evaluate((element) => ({
        overflowY: getComputedStyle(element).overflowY,
        overflow: element.scrollHeight - element.clientHeight,
      }));
    expect(metrics.overflowY).toBe('hidden');
    expect(metrics.overflow).toBeLessThanOrEqual(0);
  });

  test('degrades instead of crashing when the backend answers with empty bodies', async ({
    page,
  }) => {
    // The default mock answers the home and runtime endpoints with `{}`.
    const pageErrors: string[] = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto('/home');

    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await expect(page.locator('[data-home-surface="hero"]')).toBeVisible();
    expect(pageErrors).toEqual([]);
  });

  test('gives the page controls one filled button style', async ({ page }) => {
    await mockHome(page, {
      overview: EMPTY_OVERVIEW,
      runtime: {
        generated_at_micros: 200,
        window_start_micros: 100,
        window_end_micros: 200,
        window_secs: 100,
        streams: [],
      },
    });
    await page.goto('/home');

    // Colors transition over 120ms after load; sample the settled value.
    await page.addStyleTag({
      content: '*, *::before, *::after { transition: none !important; }',
    });
    const fill = (name: string | RegExp) =>
      page
        .getByRole('button', { name })
        .first()
        .evaluate((element) => getComputedStyle(element).backgroundColor);
    // A bare icon button next to filled ones reads as disabled, not as a button.
    // Polled: under load the toolbar can re-render between finding a button and
    // reading its style.
    await expect
      .poll(() => fill(/Last 24 hours/))
      .not.toMatch(/^(|rgba\(0, 0, 0, 0\))$/);
    const windowFill = await fill(/Last 24 hours/);
    await expect.poll(() => fill('Refresh')).toBe(windowFill);
    await expect.poll(() => fill('New')).toBe(windowFill);
  });

  test('gives icon-only chrome controls a 32px target', async ({ page }) => {
    await mockHome(page, {
      overview: EMPTY_OVERVIEW,
      runtime: {
        generated_at_micros: 200,
        window_start_micros: 100,
        window_end_micros: 200,
        window_secs: 100,
        streams: [],
      },
    });
    await page.goto('/home');

    for (const testId of ['sidebar-toggle', 'theme-toggle', 'user-menu-trigger']) {
      const box = await page.getByTestId(testId).boundingBox();
      expect(box, testId).not.toBeNull();
      expect(box!.width, testId).toBeGreaterThanOrEqual(32);
      expect(box!.height, testId).toBeGreaterThanOrEqual(32);
    }
  });
});
