import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Trace contextual pivots', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/traces/demo-trace');
    await expect(page.getByTestId('trace-span-bar')).toHaveCount(3);
  });

  test('opens an exact Span logs pivot and keeps the source Trace visible', async ({ page }) => {
    // A left click on a span's name selects the span; its menu is under the right button.
    await page.getByTestId('trace-span-row').filter({ hasText: 'GET /users' }).getByRole('button', { name: 'GET /users', exact: true }).click({ button: 'right' });

    const spanLogs = page.getByRole('link', { name: /View span logs/ });
    await expect(spanLogs).toContainText('Exact match');
    const href = await spanLogs.getAttribute('href');
    expect(href).toContain('/logs?');
    expect(href).toContain('traceId=demo-trace');
    expect(href).toContain('spanId=s2');
    expect(href).toContain('source=trace');
    expect(href).toContain('sourceId=demo-trace');

    await spanLogs.click();
    await expect(page).toHaveURL(/\/logs\?/);
  });

  // A pick in the span's menu is the menu's, not a pick of the span: the trace page, which keeps
  // the span picked in its address, would otherwise write it over the address the link opened.
  test('keeps the source Trace in sight on the Logs page it opened', async ({ page }) => {
    await page.getByTestId('trace-span-row').filter({ hasText: 'GET /users' }).getByRole('button', { name: 'GET /users', exact: true }).click({ button: 'right' });
    await page.getByRole('link', { name: /View span logs/ }).click();
    await expect(page.getByText('From Trace demo-trace', { exact: false })).toBeVisible();
    await expect(page.getByRole('link', { name: /Back to trace details/ })).toHaveAttribute('href', '/traces/demo-trace');

    const params = new URL(page.url()).searchParams;
    expect(params.get('traceId')).toBe('demo-trace');
    expect(params.get('spanId')).toBe('s2');
    expect(params.get('source')).toBe('trace');
    expect(params.get('sourceId')).toBe('demo-trace');
  });

  test('offers service-scoped metrics, traces, and logs', async ({ page }) => {
    // The span's service is a reference at the head of its detail.
    await page.getByTestId('trace-span-row').filter({ hasText: 'GET /users' }).getByRole('button', { name: 'GET /users', exact: true }).click();
    await page.getByTestId('trace-span-panel').getByRole('button', { name: 'api', exact: true }).click();

    const metrics = page.getByRole('link', { name: /View service metrics/ });
    const traces = page.getByRole('link', { name: /View traces for this service/ });
    const logs = page.getByRole('link', { name: /View logs for this service/ });
    await expect(metrics).toContainText('Context match');
    await expect(traces).toBeVisible();
    await expect(logs).toBeVisible();

    expect(await metrics.getAttribute('href')).toContain('service=api');
    // Each names the service as its page takes it: the traces query by the span's field,
    // the logs by meaning, for the Logs page to put on the field of the stream it opens.
    expect(await traces.getAttribute('href')).toContain('service.name');
    expect(await logs.getAttribute('href')).toContain('service=api');

    await metrics.click();
    await expect(page).toHaveURL(/\/metrics\?/);

    const contextBar = page.getByTestId('investigation-context-bar');
    const topbar = page.getByRole('banner');
    const pageHeader = page.locator(
      '#main > [data-testid="investigation-context-bar"] + div',
    );
    await expect(contextBar).toBeVisible();
    await expect(topbar).toBeVisible();
    await expect(pageHeader).toBeVisible();

    const [contextBox, topbarBox, headerBox] = await Promise.all([
      contextBar.boundingBox(),
      topbar.boundingBox(),
      pageHeader.boundingBox(),
    ]);
    expect(contextBox).not.toBeNull();
    expect(topbarBox).not.toBeNull();
    expect(headerBox).not.toBeNull();
    expect(contextBox!.y).toBeGreaterThanOrEqual(topbarBox!.y + topbarBox!.height);
    expect(contextBox!.y + contextBox!.height).toBeLessThanOrEqual(headerBox!.y);
    await expect
      .poll(() =>
        page.evaluate(() =>
          getComputedStyle(document.documentElement).getPropertyValue('--contextbar-h').trim(),
        ),
      )
      .toBe(`${contextBox!.height}px`);
  });

  test('draws rows of one height, each bar in the colour of its service, as the legend says', async ({ page }) => {
    const bars = page.getByTestId('trace-span-bar');
    // Three services, three colours; red is a failed span's and none of them.
    await expect(bars.nth(0)).toHaveAttribute('data-tone', 'service');
    const fills = await bars.evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).backgroundColor));
    expect(new Set(fills).size).toBe(3);
    const errorRed = await page.evaluate(() => {
      const probe = document.createElement('span');
      probe.style.color = 'var(--red)';
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    });
    expect(fills).not.toContain(errorRed);

    // The legend names each service in its bar's colour, in the order the trace meets them.
    const legend = page.getByTestId('trace-legend');
    await expect(legend.locator('li')).toHaveCount(3);
    await expect(legend.locator('li').nth(0)).toContainText('web');
    const swatches = await legend
      .locator('li > span[aria-hidden="true"]')
      .evaluateAll((nodes) => nodes.map((node) => getComputedStyle(node).backgroundColor));
    expect(swatches).toEqual(fills);

    // A name set as an HTTP method and a plain one take the same room.
    const rows = page.getByTestId('trace-span-row');
    await expect(rows).toHaveCount(3);
    const heights = await rows.evaluateAll((nodes) => nodes.map((node) => node.getBoundingClientRect().height));
    expect(heights).toEqual([32, 32, 32]);
    const barHeights = await bars.evaluateAll((nodes) => nodes.map((node) => node.getBoundingClientRect().height));
    expect(barHeights).toEqual([12, 12, 12]);
  });

  test('says each duration past the end of its bar, clear of it, the bar that reaches the end of the axis too', async ({
    page,
  }) => {
    const bars = page.getByTestId('trace-span-bar');
    const durations = page.getByTestId('trace-span-duration');
    await expect(durations).toHaveCount(3);

    const boxes = async (index: number) => {
      const [bar, duration] = await Promise.all([bars.nth(index).boundingBox(), durations.nth(index).boundingBox()]);
      expect(bar).not.toBeNull();
      expect(duration).not.toBeNull();
      return { bar: bar!, duration: duration! };
    };

    // Every duration begins where its bar ends. The root takes the whole axis: the room right of the axis holds its duration.
    for (const index of [0, 1, 2]) {
      const { bar, duration } = await boxes(index);
      expect(Math.abs(duration.x - (bar.x + bar.width))).toBeLessThanOrEqual(1.5);
    }
    const root = await boxes(0);
    const row = await page.getByTestId('trace-span-row').first().boundingBox();
    expect(root.duration.x + root.duration.width).toBeLessThanOrEqual(row!.x + row!.width);

    await bars.first().hover();
    await expect(page.getByRole('tooltip')).toHaveCount(0);
  });
});
