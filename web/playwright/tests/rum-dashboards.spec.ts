import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';
import { installRumDashboards } from '../fixtures/mockRumDashboards';

const WEB = MOCK_RUM.applications.web;
const APP = MOCK_RUM.applications.app;
const BROWSER = '/dashboards/rum-browser-acme-prod';

async function boot(page: Page, port: number) {
  await mountMockRoutes(page, port);

  return installRumDashboards(page);
}

/** The control of a variable of the dashboard, by the label it has. */
const control = (page: Page, label: string) => page.locator('label', { hasText: label }).locator('select');

test.describe('RUM dashboards', () => {
  test('are linked from the overview with the application and the environment of the page', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);

    const link = page.locator('[data-rum-dashboard-link]');

    await expect(link).toBeVisible();
    await expect(link).toContainText('Open in dashboard');
    await expect(link).toHaveAttribute('href', `${BROWSER}?var-application=${WEB}&var-environment=prod`);
  });

  test('are the mobile one for an application that runs on a phone, whatever the environment', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${APP}`);

    await expect(page.locator('[data-rum-dashboard-link]')).toHaveAttribute(
      'href',
      `/dashboards/rum-mobile-acme-prod?var-application=${APP}`,
    );
  });

  test('open on a day when nothing else chose the range, as they were made for', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(BROWSER);

    await expect(page.getByRole('button', { name: 'Time range: Last 24 hours' })).toBeVisible();
  });

  test('open on what the link chose, and every panel asks about it', async ({ page, mockServer }) => {
    const captured = await boot(page, mockServer.port);

    await page.goto(`${BROWSER}?var-application=${WEB}&var-environment=prod`);

    await expect(control(page, '应用')).toHaveValue(WEB);
    await expect(control(page, '环境')).toHaveValue('prod');

    await expect
      .poll(() => captured.statements.some((sql) => sql.includes(`"application.id" = '${WEB}'`) && sql.includes(`env = 'prod'`)))
      .toBe(true);

    // The statement is the one of the panel, with the variables put in.
    expect(captured.statements.every((sql) => !sql.includes('${'))).toBe(true);
  });
});

test.describe('a built-in dashboard', () => {
  test('is copied by a button of its head, and the copy is the person\'s own to change', async ({ page, mockServer }) => {
    const captured = await boot(page, mockServer.port);

    await page.goto(BROWSER);
    // The dashboard writes the day it opens on into the address a moment after it opens: a click made
    // before that would be written over by it.
    await expect(page).toHaveURL(/[?&]time=/);
    await page.getByRole('button', { name: 'Create copy' }).click();

    await expect(page).toHaveURL(/\/dashboards\/dash-\d+/);
    expect(captured.created).toHaveLength(1);

    const model = captured.created[0]!.model;

    expect(model.editable).toBe(true);
    expect(model.uid).toBe('');
    expect(model.title).toBe('RUM · 浏览器应用 copy');
    expect(model.extensions).toEqual({});
    expect(model.tags).toEqual(['内置', 'rum']);
  });
});

test.describe('the variables of a dashboard', () => {
  test('go into the address when they are chosen, and are chosen again from it', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    // The address names the day the dashboard opens on, so the shell has no range to write into it: a
    // write of its own, on the address it saw before, could take back the variable the page puts there.
    await page.goto(`${BROWSER}?time=now-24h..now`);

    // Nobody has chosen an application: the first one is, and the address says so.
    await expect(control(page, '应用')).toHaveValue(WEB);
    await expect(page).toHaveURL(new RegExp(`var-application=${WEB}`));

    await control(page, '环境').selectOption('staging');
    await expect(page).toHaveURL(/var-environment=staging/);

    await page.reload();

    await expect(control(page, '应用')).toHaveValue(WEB);
    await expect(control(page, '环境')).toHaveValue('staging');
  });

  test('keep a value of the address that the range holds no data for', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`${BROWSER}?var-application=an-older-app`);

    // It is not among the applications of the range, and it is still the one the page is about.
    await expect(control(page, '应用')).toHaveValue('an-older-app');
  });
});
