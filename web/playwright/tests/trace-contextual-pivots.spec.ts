import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Trace contextual pivots', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/traces/demo-trace');
    await expect(page.getByTestId('trace-span-bar')).toHaveCount(3);
  });

  test('opens an exact Span logs pivot and keeps the source Trace visible', async ({ page }) => {
    await page.getByRole('button', { name: 'GET /users', exact: true }).click();

    const spanLogs = page.getByRole('link', { name: /View span logs/ });
    await expect(spanLogs).toContainText('Exact match');
    const href = await spanLogs.getAttribute('href');
    expect(href).toContain('/logs?');
    expect(href).toContain('trace_id=demo-trace');
    expect(href).toContain('span_id=s2');
    expect(href).toContain('source=trace');
    expect(href).toContain('source_id=demo-trace');

    await spanLogs.click();
    await expect(page).toHaveURL(/\/logs\?/);
    await expect(page.getByText('From Trace demo-trace', { exact: false })).toBeVisible();
    await expect(page.getByRole('link', { name: /Back to trace details/ })).toHaveAttribute(
      'href',
      '/traces/demo-trace',
    );
  });

  test('offers service-scoped metrics, traces, and logs', async ({ page }) => {
    await page.getByRole('button', { name: 'api', exact: true }).click();

    const metrics = page.getByRole('link', { name: /View service metrics/ });
    const traces = page.getByRole('link', { name: /View traces for this service/ });
    const logs = page.getByRole('link', { name: /View logs for this service/ });
    await expect(metrics).toContainText('Context match');
    await expect(traces).toBeVisible();
    await expect(logs).toBeVisible();

    expect(await metrics.getAttribute('href')).toContain('service=api');
    expect(await traces.getAttribute('href')).toContain('service_name');
    expect(await logs.getAttribute('href')).toContain('service');

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

  test('renders each duration after its waterfall bar without a hover tooltip', async ({
    page,
  }) => {
    const bars = page.getByTestId('trace-span-bar');
    const durations = page.getByTestId('trace-span-duration');
    await expect(durations).toHaveCount(3);

    for (let index = 0; index < 3; index += 1) {
      const [barBox, durationBox] = await Promise.all([
        bars.nth(index).boundingBox(),
        durations.nth(index).boundingBox(),
      ]);
      expect(barBox).not.toBeNull();
      expect(durationBox).not.toBeNull();
      expect(durationBox!.x).toBeGreaterThanOrEqual(barBox!.x + barBox!.width + 4);
      expect(durationBox!.x).toBeLessThanOrEqual(barBox!.x + barBox!.width + 9);
    }

    await bars.first().hover();
    await expect(page.getByRole('tooltip')).toHaveCount(0);
  });
});
