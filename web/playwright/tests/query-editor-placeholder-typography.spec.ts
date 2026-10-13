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
  // The editor first paints a plain stand-in and swaps in the real one once
  // its chunk has loaded. Only the real one hides its hint from assistive
  // technology, so that is the hint to wait for: the stand-in can be removed
  // between finding it and reading its style.
  const editorHint = '[data-code-editor-placeholder="true"][aria-hidden="true"]';

  await page.goto('/metrics');
  // Every explorer starts with its editor folded into the bar.
  await page.getByRole('button', { name: 'Expand query' }).click();
  const metricsPlaceholder = page
    .locator(editorHint)
    .filter({ hasText: 'rate(metric_name{...}[5m])' });
  await expect(metricsPlaceholder).toBeVisible();
  const metricsTypography = await readTypography(metricsPlaceholder);

  await page.goto('/traces');
  const expandQuery = page.getByRole('button', { name: 'Expand query' });
  if (await expandQuery.isVisible()) await expandQuery.click();
  const tracesPlaceholder = page
    .locator(editorHint)
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
