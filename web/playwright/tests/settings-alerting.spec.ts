/**
 * Settings → Alerting: holiday calendars (made from an ICS file, kept while something counts by
 * them) and the organization's priority and urgency levels.
 */
import { Buffer } from 'node:buffer';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const CALENDAR = {
  id: 'cal-cn',
  orgId: 'acme-prod',
  name: 'CN 2026',
  description: '',
  days: [
    { date: '2026-10-01', kind: 'holiday', name: 'National Day' },
    { date: '2026-10-10', kind: 'workday', name: 'Make-up workday' },
  ],
  createdBy: null,
  createdAt: 1,
  updatedAt: 1,
};

const ICS = [
  'BEGIN:VCALENDAR',
  'BEGIN:VEVENT',
  'DTSTART;VALUE=DATE:20271001',
  'DTEND;VALUE=DATE:20271003',
  'SUMMARY:National Day',
  'END:VEVENT',
  'END:VCALENDAR',
  '',
].join('\r\n');

test('a holiday calendar is read from an ICS file, and one in use cannot be deleted', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  let parsed: string | null = null;
  await page.route(/\/api\/v1\/alerts\/holiday-calendars$/, (route) => {
    if (route.request().method() !== 'POST') return route.fulfill({ json: [CALENDAR] });
    created = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ status: 201, json: { ...CALENDAR, ...created, id: 'cal-new' } });
  });
  await page.route(/\/api\/v1\/alerts\/holiday-calendars\/parse$/, (route) => {
    parsed = (route.request().postDataJSON() as { content: string }).content;
    return route.fulfill({
      json: {
        days: [
          { date: '2027-10-01', kind: 'holiday', name: 'National Day' },
          { date: '2027-10-02', kind: 'holiday', name: 'National Day' },
        ],
      },
    });
  });
  await page.route(/\/api\/v1\/alerts\/holiday-calendars\/cal-cn\/references$/, (route) =>
    route.fulfill({ json: [{ kind: 'routing_rule', id: 'rr-1', name: 'Checkout on workdays' }] }),
  );

  await page.goto('/settings/alerting/holidays');
  const row = page.getByRole('row', { name: /CN 2026/ });
  await expect(row).toBeVisible();

  // Something counts by it: the dialog names it and keeps the calendar.
  await row.getByRole('button', { name: 'Delete' }).click();
  const confirm = page.getByRole('dialog', { name: 'Delete CN 2026?' });
  await expect(confirm).toContainText('Checkout on workdays');
  await expect(confirm.getByRole('button', { name: 'Delete' })).toBeDisabled();
  await confirm.getByRole('button', { name: 'Cancel' }).click();

  await page.getByTestId('holiday-new').click();
  const drawer = page.getByRole('dialog', { name: 'New holiday calendar' });
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('CN 2027');
  await drawer.locator('input[type="file"]').setInputFiles({
    name: 'holidays.ics',
    mimeType: 'text/calendar',
    buffer: Buffer.from(ICS),
  });

  await expect.poll(() => parsed).toBe(ICS);
  await expect(drawer.locator('[data-holiday-day="2027-10-01"]')).toBeVisible();
  await expect(drawer.locator('[data-holiday-day="2027-10-02"]')).toBeVisible();

  await drawer.getByRole('button', { name: 'Create calendar' }).click();
  await expect.poll(() => created).toMatchObject({
    name: 'CN 2027',
    days: [
      { date: '2027-10-01', kind: 'holiday', name: 'National Day' },
      { date: '2027-10-02', kind: 'holiday', name: 'National Day' },
    ],
  });
});

const BUILT_IN_LEVELS = {
  priorities: ['P1', 'P2', 'P3', 'P4', 'P5'].map((priority) => ({ priority, name: '', description: '' })),
  urgencies: [
    { key: 'high', name: '', escalates: true, quietHours: 'switch', steps: 'high', severities: ['critical', 'error'] },
    { key: 'low', name: '', escalates: false, quietHours: 'block', steps: 'low', severities: ['warning', 'info'] },
  ],
};

test('the organization adds a priority and an urgency level, and moves a severity to it', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/importance$/, (route) => {
    if (route.request().method() !== 'PUT') {
      return route.fulfill({ json: { levels: BUILT_IN_LEVELS, updatedBy: null, updatedAt: 0 } });
    }
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { levels: saved, updatedBy: 'u1', updatedAt: 2 } });
  });

  await page.goto('/settings/alerting/importance');
  await expect(page.locator('[data-priority-levels="5"]')).toBeVisible();
  await page.getByTestId('priority-add').click();
  await expect(page.locator('[data-priority-levels="6"]')).toBeVisible();

  await page.getByRole('textbox', { name: 'Key of a new level' }).fill('urgent');
  await page.getByTestId('urgency-add').click();
  const urgent = page.locator('[data-urgency-level="urgent"]');
  await urgent.getByRole('textbox', { name: 'Name of urgent' }).fill('Urgent');
  await urgent.getByRole('button', { name: 'Critical' }).click();
  await expect(urgent.getByRole('button', { name: 'Critical' })).toHaveAttribute('aria-pressed', 'true');

  await page.getByTestId('importance-save').click();
  await expect.poll(() => saved).toMatchObject({
    priorities: Array.from({ length: 6 }, (_, index) => ({ priority: `P${index + 1}` })),
    // A new level goes in above low, the least urgent.
    urgencies: [
      { key: 'high', severities: ['error'] },
      { key: 'urgent', name: 'Urgent', severities: ['critical'] },
      { key: 'low', severities: ['warning', 'info'] },
    ],
  });
});
