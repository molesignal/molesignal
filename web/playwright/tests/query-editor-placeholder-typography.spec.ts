import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface PlaceholderTypography {
  fontFamily: string;
  fontSize: string;
  fontStyle: string;
  fontWeight: string;
  lineHeight: string;
}

async function readTypography(locator: Locator): Promise<PlaceholderTypography> {
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      fontFamily: style.fontFamily,
      fontSize: style.fontSize,
      fontStyle: style.fontStyle,
      fontWeight: style.fontWeight,
      lineHeight: style.lineHeight,
    };
  });
}

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
});

test('keeps Metrics and Traces query hints on the shared editor typography', async ({
  page,
}) => {
  await page.goto('/metrics');
  const metricsPlaceholder = page
    .locator('[data-code-editor-placeholder="true"]')
    .filter({ hasText: 'rate(metric_name{...}[5m])' });
  await expect(metricsPlaceholder).toBeVisible();
  const metricsTypography = await readTypography(metricsPlaceholder);

  await page.goto('/traces');
  const expandQuery = page.getByRole('button', { name: 'Expand query' });
  if (await expandQuery.isVisible()) await expandQuery.click();
  const tracesPlaceholder = page
    .locator('[data-code-editor-placeholder="true"]')
    .filter({ hasText: 'trace_id = "..." / service_name contains "checkout"' });
  await expect(tracesPlaceholder).toBeVisible();
  const tracesTypography = await readTypography(tracesPlaceholder);

  expect(metricsTypography).toEqual(tracesTypography);
  expect(metricsTypography).toMatchObject({
    fontSize: '12px',
    fontStyle: 'italic',
    fontWeight: '400',
    lineHeight: '20px',
  });
  expect(metricsTypography.fontFamily).toContain('JetBrains Mono');
});
