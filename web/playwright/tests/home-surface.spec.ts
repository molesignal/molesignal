import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Home surface hierarchy', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('uses canvas gutters and borderless functional surfaces', async ({
    page,
  }) => {
    await page.route('**/api/v1/home/overview**', (route) =>
      route.fulfill({
        json: {
          generated_at_micros: 200,
          window: {
            start_micros: 100,
            end_micros: 200,
            window_secs: 100,
          },
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
        },
      }),
    );
    await page.route('**/api/v1/audit**', (route) =>
      route.fulfill({ json: [] }),
    );
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

    const canvas = page.getByTestId('home-operations-canvas');
    await expect(canvas).toHaveClass(/space-y-\[12px\]/);
    await expect(canvas).not.toHaveClass(/border|divide/);

    const kpis = page.locator('[data-home-surface="kpi"]');
    await expect(kpis).toHaveCount(6);
    for (const kpi of await kpis.all()) {
      const appearance = await kpi.evaluate((element) => {
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
    await expect(surfaceGrids).toHaveCount(2);
    for (const grid of await surfaceGrids.all()) {
      const appearance = await grid.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          rowGap: styles.rowGap,
          columnGap: styles.columnGap,
          borderTop: styles.borderTopWidth,
        };
      });
      expect(appearance.rowGap).toBe('12px');
      expect(appearance.columnGap).toBe('12px');
      expect(appearance.borderTop).toBe('0px');
    }

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

    const operationalContext = page.getByTestId(
      'home-primary-operational-context',
    );
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
});
