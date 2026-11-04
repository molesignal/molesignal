/**
 * Shift swaps and calendar subscriptions: someone on call asks a member to take a stretch of their
 * shift, the person asked answers, a manager approves when the schedule needs it; anyone can put a
 * schedule's shifts, or their own, in a calendar app.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { scheduleCoverage, withCoverage } from '../fixtures/scheduleCoverage';

const HOUR = 3_600_000_000;
const DAY = 24 * HOUR;
/** The page's clock is frozen here. */
const NOW = Date.parse(FROZEN_NOW_ISO) * 1000;

/** A schedule the signed-in person (`dev`) is on call for through an override right now. */
function swapSchedule(now: number, swapNeedsApproval = false) {
  return {
    id: 'sch-swap',
    orgId: 'acme-prod',
    name: 'Search on-call',
    description: 'Indexing and the query path.',
    timezone: 'UTC',
    enabled: true,
    rotations: [
      { id: 'rot-swap', name: 'Primary', members: ['dev', 'u2', 'u3'], kind: 'weekly' as const, activeWindows: [], startAt: now - 30 * DAY },
    ],
    overrides: [{ id: 'ovr-dev', userId: 'dev', startAt: now - HOUR, endAt: now + 10 * HOUR, reason: 'Release week' }],
    swapNeedsApproval: swapNeedsApproval,
    createdBy: 'u1',
    updatedBy: 'u1',
    createdAt: now - 30 * DAY,
    updatedAt: now - DAY,
  };
}

function swap(now: number, id: string, fields: Record<string, unknown>) {
  return {
    id,
    scheduleId: 'sch-swap',
    scheduleName: 'Search on-call',
    requesterId: 'u2',
    targetId: 'dev',
    startAt: now + 2 * DAY,
    endAt: now + 2 * DAY + 8 * HOUR,
    note: '',
    status: 'pending',
    overrideId: null,
    decidedBy: null,
    decidedAt: null,
    createdAt: now - HOUR,
    updatedAt: now - HOUR,
    ...fields,
  };
}

async function routeSchedule(page: Page, schedule: ReturnType<typeof swapSchedule>, swaps: unknown[]) {
  await page.route(/\/api\/v1\/schedules\/sch-swap$/, (route) => route.fulfill({ json: withCoverage(schedule, NOW) }));
  await page.route(/\/api\/v1\/schedules\/sch-swap\/coverage(?:\?.*)?$/, (route) => {
    const params = new URL(route.request().url()).searchParams;
    return route.fulfill({ json: scheduleCoverage(schedule, Number(params.get('from')), Number(params.get('to'))) });
  });
  await page.route(/\/api\/v1\/schedules\/sch-swap\/swaps$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { swaps } }) : route.fallback(),
  );
}

test('someone on call asks a member to take their shift', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = NOW;
  await routeSchedule(page, swapSchedule(now), []);

  let sent: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/schedules\/sch-swap\/swaps$/, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    sent = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: swap(now, 'sw-new', { requesterId: 'dev', targetId: 'u2', ...sent }) });
  });

  await page.goto('/alerts/schedules/sch-swap');
  await page.getByRole('button', { name: 'Ask for a swap' }).click();

  const dialog = page.getByRole('dialog', { name: 'Ask for a swap' });
  await expect(dialog).toContainText('It takes effect when they accept.');
  await dialog.getByRole('combobox', { name: 'Taken by' }).click();
  await page.getByRole('option', { name: 'Kenji Watanabe' }).click();
  await dialog.getByRole('textbox', { name: 'Note' }).fill('Travelling on Friday');
  await dialog.getByRole('button', { name: 'Send request' }).click();

  await expect(page.getByText('Swap request sent')).toBeVisible();
  expect(sent).toMatchObject({ targetId: 'u2', note: 'Travelling on Friday' });
  // The coming shift is the override still running: from now to its end.
  const body = sent as unknown as { startAt: number; endAt: number };
  expect(body.endAt).toBe(now + 10 * HOUR);
  expect(Math.abs(body.startAt - now)).toBeLessThan(2 * 60_000_000);
});

test('the person asked answers, and a manager approves when the schedule needs it', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = NOW;
  await routeSchedule(page, swapSchedule(now, true), [
    swap(now, 'sw-ask', { note: 'Dentist on Thursday' }),
    swap(now, 'sw-wait', { requesterId: 'u3', targetId: 'u2', status: 'awaiting_approval' }),
    swap(now, 'sw-old', { status: 'declined', decidedBy: 'dev', decidedAt: now - DAY }),
  ]);

  const answered: string[] = [];
  await page.route(/\/api\/v1\/schedules\/sch-swap\/swaps\/[^/]+\/(accept|approve)$/, (route) => {
    const [, id, answer] = /swaps\/([^/]+)\/(\w+)$/.exec(route.request().url()) ?? [];
    answered.push(`${id}:${answer}`);
    return route.fulfill({
      json: swap(now, id ?? '', { status: answer === 'accept' ? 'awaiting_approval' : 'accepted', overrideId: answer === 'approve' ? 'ovr-new' : null }),
    });
  });

  await page.goto('/alerts/schedules/sch-swap');

  const asked = page.locator('[data-swap="sw-ask"]');
  await expect(asked).toContainText('Kenji Watanabe asks');
  await expect(asked).toContainText('Waiting for an answer');
  await expect(asked).toContainText('Dentist on Thursday');
  // Settled swaps stay out of the way until asked for.
  await expect(page.locator('[data-swap="sw-old"]')).toHaveCount(0);

  await asked.getByRole('button', { name: 'Accept' }).click();
  await expect(page.getByText('Accepted; waiting for approval')).toBeVisible();

  const waiting = page.locator('[data-swap="sw-wait"]');
  await expect(waiting).toContainText('Amara Okafor asks Kenji Watanabe to take over');
  await waiting.getByRole('button', { name: 'Approve' }).click();
  await expect(page.getByText('Approved; the override is in effect')).toBeVisible();
  expect(answered).toEqual(['sw-ask:accept', 'sw-wait:approve']);

  await page.getByRole('button', { name: 'Show settled' }).click();
  await expect(page.locator('[data-swap="sw-old"]')).toContainText('Declined');
});

test('a schedule asks for approval of swaps from its editor', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = NOW;
  await routeSchedule(page, swapSchedule(now), []);

  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/schedules\/sch-swap$/, async (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ...swapSchedule(now), ...saved } });
  });

  await page.goto('/alerts/schedules/sch-swap');
  await page.getByTestId('page-header').getByRole('button', { name: 'Edit schedule' }).click();
  await page.getByRole('switch', { name: 'Swaps need approval' }).click();
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved).toMatchObject({ swapNeedsApproval: true });
});

test('my swaps and my own calendar from the schedules list', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = NOW;

  await page.route(/\/api\/v1\/schedules\/swaps$/, (route) =>
    route.fulfill({ json: { swaps: [swap(now, 'sw-mine', {}), swap(now, 'sw-sent', { requesterId: 'dev', targetId: 'u3' })] } }),
  );
  let feeds: unknown[] = [];
  await page.route(/\/api\/v1\/schedules\/calendar-feeds$/, (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: { feeds } });
    const feed = { id: 'feed-1', scheduleId: null, scheduleName: null, createdAt: now, lastUsedAt: null };
    feeds = [feed];
    return route.fulfill({ json: { feed, path: '/api/v1/public/calendar/0123abcd.ics', url: null } });
  });

  await page.goto('/alerts/schedules');

  const swapsButton = page.locator('[data-my-swaps-button]');
  await expect(swapsButton).toContainText('1');
  await swapsButton.click();
  const mine = page.getByRole('dialog', { name: 'My shift swaps' });
  await expect(mine.locator('[data-swap="sw-mine"]').getByRole('link', { name: 'Search on-call' })).toHaveAttribute(
    'href',
    '/alerts/schedules/sch-swap',
  );
  await expect(mine.locator('[data-swap="sw-mine"]').getByRole('button', { name: 'Accept' })).toBeVisible();
  await expect(mine.locator('[data-swap="sw-sent"]').getByRole('button', { name: 'Withdraw' })).toBeVisible();
  await page.keyboard.press('Escape');

  await page.getByRole('button', { name: 'Subscribe to my shifts' }).click();
  const subscribe = page.getByRole('dialog', { name: 'Subscribe to my shifts' });
  await subscribe.getByRole('button', { name: 'Create address' }).click();
  const origin = new URL(page.url()).origin;
  await expect(subscribe.locator('[data-calendar-subscription]')).toContainText(
    `${origin}/api/v1/public/calendar/0123abcd.ics`,
  );
  await expect(subscribe).toContainText('No external address is set for this instance');
  await expect(subscribe.getByRole('link', { name: 'Open in calendar app' })).toHaveAttribute(
    'href',
    `${origin.replace(/^https?:\/\//, 'webcal://')}/api/v1/public/calendar/0123abcd.ics`,
  );
});
