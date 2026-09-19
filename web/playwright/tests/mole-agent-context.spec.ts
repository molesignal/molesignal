import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('Mole Agent sends the investigation context shown in the shell', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  const filters = encodeURIComponent(
    JSON.stringify([['service', '=', 'checkout']]),
  );
  await page.goto(
    `/logs?time=2026-08-14T05%3A00%3A00.000Z..2026-08-14T06%3A00%3A00.000Z&filters=${filters}`,
  );

  const panel = page.locator('aside[aria-label="Mole Agent"]');
  await expect(panel).toHaveAttribute('inert', '');
  await page.getByTestId('mole-agent-trigger').click();
  await expect(panel).not.toHaveAttribute('inert', '');
  await expect(panel).toContainText('service:checkout');
  await expect(panel.getByTestId('composer-context')).toContainText(
    'Service: checkout',
  );
  await expect(panel.getByRole('combobox', { name: 'Time' })).toContainText(
    '05:00:00 - 06:00:00 UTC',
  );

  const outbound = page.waitForRequest(
    (request) =>
      request.method() === 'POST' &&
      /\/api\/v1\/agent\/chat\/[^/]+\/messages$/.test(request.url()),
  );
  await panel
    .getByLabel('Ask about incidents, services, or operational tasks…')
    .fill('Investigate checkout');
  await panel.getByRole('button', { name: 'Send' }).click();

  const body = (await outbound).postDataJSON() as {
    time_range: { start_micros: number; end_micros: number };
    stream_hints: string[];
  };
  expect(body.time_range).toEqual({
    start_micros: Date.parse('2026-08-14T05:00:00.000Z') * 1_000,
    end_micros: Date.parse('2026-08-14T06:00:00.000Z') * 1_000,
  });
  expect(body.stream_hints).toContain('service:checkout');

  await panel.getByRole('button', { name: 'Close' }).click();
  await expect(panel).toHaveAttribute('inert', '');
});
