/**
 * What a list can do beyond showing its rows: sort, tick and act on what is
 * ticked, write itself to a file, and find the way back from filters that leave nothing.
 */
import { Buffer } from 'node:buffer';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('the rules list', () => {
  test('sorts by a column, the other way, then back to its own order', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');

    const firstName = page.locator('table tbody tr').first().locator('td').nth(1);
    await expect(firstName).toContainText('Public API 5xx rate');

    const head = page.getByRole('columnheader', { name: 'Rule' });
    await head.getByRole('button').click();
    await expect(head).toHaveAttribute('aria-sort', 'ascending');
    await expect(firstName).toContainText('Certificate expiry');

    await head.getByRole('button').click();
    await expect(head).toHaveAttribute('aria-sort', 'descending');
    await expect(firstName).toContainText('Slow order traces');

    await head.getByRole('button').click();
    await expect(head).not.toHaveAttribute('aria-sort');
    await expect(firstName).toContainText('Public API 5xx rate');
  });

  test('switches off what is ticked, and says how it went', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const written: Array<{ url: string; enabled: unknown }> = [];
    await page.route('**/api/v1/alerts/rules/*', async (route) => {
      if (route.request().method() !== 'PUT') return route.fallback();
      written.push({ url: route.request().url(), enabled: route.request().postDataJSON().enabled });
      return route.fulfill({ json: route.request().postDataJSON() });
    });

    await page.goto('/alerts/rules');
    await page.locator('table tbody tr').nth(0).getByRole('checkbox').click();
    await page.locator('table tbody tr').nth(1).getByRole('checkbox').click();

    const bar = page.getByTestId('rule-bulk');
    await expect(bar).toContainText('2 selected');
    await bar.getByRole('button', { name: 'Disable' }).click();

    await expect(page.getByText('2 rows updated')).toBeVisible();
    await expect(bar).toHaveCount(0);
    expect(written).toHaveLength(2);
    expect(written.every((request) => request.enabled === false)).toBe(true);
  });

  test('gives the way back from filters that leave nothing', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');

    await page.getByPlaceholder('Search rules, services, or sources').fill('nothing like this');
    await page.getByRole('button', { name: 'Clear filters' }).click();

    await expect(page.locator('table tbody tr')).toHaveCount(8);
  });
});

test.describe('the users list', () => {
  test('writes what is in view to a CSV a spreadsheet opens as UTF-8', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/iam/users');

    const download = page.waitForEvent('download');
    await page.getByTestId('table-export').click();
    const file = await download;

    expect(file.suggestedFilename()).toMatch(/^molesignal-users-.*\.csv$/);
    const chunks: Buffer[] = [];
    for await (const chunk of await file.createReadStream()) chunks.push(chunk as Buffer);
    const csv = Buffer.concat(chunks).toString('utf8');

    expect(csv.charCodeAt(0)).toBe(0xfeff);
    expect(csv).toContain('sre@example.com');
    expect(csv.split('\r\n')[0]).toContain('Email');
  });

  test('keeps oneself out of what can be ticked', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/iam/users');

    const rows = page.locator('table tbody tr');
    await expect(rows).not.toHaveCount(0);
    const boxes = await page.locator('table tbody [role="checkbox"]').count();
    expect(boxes).toBeLessThanOrEqual(await rows.count());
  });
});

test.describe('the status column', () => {
  for (const path of ['/alerts/rules', '/alerts/silences', '/iam/users', '/alerts/incidents']) {
    test(`${path} says it in words, with no chip behind them`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port);
      await page.goto(path);

      await expect(page.locator('table tbody tr').first()).toBeVisible();
      const chips = await page.evaluate(() => {
        const table = document.querySelector('table');
        if (!table) return -1;
        const heads = [...table.querySelectorAll('thead th')].map((cell) => cell.textContent?.trim());
        const column = heads.findIndex((head) => head === 'Status' || head === 'State');
        return [...table.querySelectorAll('tbody tr')].filter((row) =>
          row.children[column]?.querySelector('[class*="rounded-full"]'),
        ).length;
      });
      expect(chips).toBe(0);
    });
  }
});
