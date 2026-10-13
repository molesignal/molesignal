import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('a report is scheduled in any time zone, chosen with the zone field every form has', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/reports');

  await page
    .getByRole('row', { name: /Weekly SLO/ })
    .getByRole('button', { name: 'Edit', exact: true })
    .click();
  // By its name: the list of zones opens as a dialog of its own.
  const workbench = page.getByRole('dialog', { name: 'Edit report' });
  await expect(workbench.getByRole('heading', { name: 'Edit report' })).toBeVisible();

  await workbench.getByText('Generation schedule', { exact: true }).first().click();

  // Five zones written into the form left most of the world out.
  const zone = workbench.getByRole('combobox', { name: 'Report timezone' });
  await expect(zone).toBeVisible();
  await zone.click();
  const search = page.getByPlaceholder('Search timezones…');
  await search.fill('Sydney');
  const sydney = page.getByRole('option', { name: /Australia\/Sydney/ });
  await expect(sydney).toBeVisible();
  await sydney.click();

  await expect(zone).toContainText('Australia/Sydney');
  // What the schedule reads as follows the zone that was picked.
  await expect(workbench).toContainText('Australia/Sydney');
});
