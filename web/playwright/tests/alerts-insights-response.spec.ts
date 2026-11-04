/**
 * How people respond to incidents and how much on-call weighs on them: the response part of the alert
 * insights (time to acknowledge, escalation, steps, ways, broken down) and each person's on-call load,
 * there and on a schedule.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('the insights say how soon people responded, compared with the window before', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/alerts/insights');

  const response = page.locator('[data-insights-response]');
  await expect(response.getByRole('heading', { name: 'Response', exact: true })).toBeVisible();
  await expect(response.locator('[data-readout]').first()).toContainText('4m');
  await expect(response).toContainText('P50 3m · P90 10m');
  // 25% now against 40% before.
  await expect(response).toContainText('38% lower than before');

  const steps = response.locator('[data-insights-steps]');
  await expect(steps).toContainText('Step 1');
  await expect(steps).toContainText('Step 2');
  await expect(steps).toContainText('1 of 2 · 50%');

  const via = response.locator('[data-insights-via]');
  await expect(via).toContainText('Web');
  await expect(via).toContainText('SMS reply');

  // Broken down by team, service and person, with names where there are names.
  const breakdown = response.locator('[data-insights-breakdown]');
  await expect(breakdown.locator('[data-breakdown-row="team-platform"]')).toContainText('Platform');
  await breakdown.getByRole('button', { name: /People/ }).click();
  await expect(breakdown.locator('[data-breakdown-row="u2"]')).toContainText('Kenji Watanabe');
  await breakdown.getByRole('button', { name: /Services/ }).click();
  await expect(breakdown.locator('[data-breakdown-row="api-gateway"]')).toContainText('3m');
});

test('each person’s on-call load counts their shifts and the pages that came at night', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let asked: URL | null = null;
  await page.route(/\/api\/v1\/alerts\/insights\/oncall-load/, async (route) => {
    asked = new URL(route.request().url());
    await route.fallback();
  });

  await page.goto('/alerts/insights');

  const load = page.locator('[data-insights-oncall-load]');
  await expect(load).toContainText('night is 22:00 to 8:00 in the timezone of their schedule');
  const kenji = load.locator('[data-load-row="u2"]');
  await expect(kenji).toContainText('Kenji Watanabe');
  await expect(kenji).toContainText('168h');
  await expect(kenji).toContainText('3 · mean 2m 30s');
  await expect(kenji).toContainText('Asia/Shanghai');
  expect(asked!.searchParams.get('windowSecs')).toBe('604800');
});

test('a schedule shows the load of its own members', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let scheduleId: string | null = null;
  await page.route(/\/api\/v1\/alerts\/insights\/oncall-load/, async (route) => {
    scheduleId = new URL(route.request().url()).searchParams.get('scheduleId');
    await route.fallback();
  });

  await page.goto('/alerts/schedules/sch-payments');
  await expect(page.getByText('Member load (last 30 days)')).toBeVisible();
  await expect(page.locator('[data-oncall-load] [data-load-row="u2"]')).toContainText('Kenji Watanabe');
  expect(scheduleId).toBe('sch-payments');
});
