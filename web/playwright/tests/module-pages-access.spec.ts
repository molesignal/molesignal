import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

// A role that holds the permission of one page of a module gets in, on that page, and is offered no
// tab that could only say "no permission".
const MODULES = [
  {
    name: 'Alerts',
    permissions: ['schedules.read'],
    enters: '/alerts',
    opens: /\/alerts\/schedules$/,
    tabs: '[data-testid="alerts-subnav"]',
    offered: ['On-call schedules'],
    absent: ['Incidents', 'Rules', 'Routing'],
  },
  {
    name: 'Synthetics',
    permissions: ['synthetics.locations.read'],
    enters: '/synthetics',
    opens: /\/synthetics\/locations$/,
    tabs: '[data-testid="synthetics-subnav"]',
    offered: ['Locations', 'Agents'],
    absent: ['Overview', 'Checks', 'Results', 'Variables'],
  },
  {
    name: 'APM',
    permissions: ['apm.errors.read'],
    enters: '/apm',
    opens: /\/apm\/errors$/,
    tabs: '[data-apm-navigation="surface"]',
    offered: ['Errors'],
    absent: ['Overview', 'Services', 'SLOs'],
  },
  {
    name: 'RUM',
    permissions: ['rum.playlists.read'],
    enters: '/rum',
    opens: /\/rum\/playlists$/,
    tabs: '[data-rum-navigation="surface"]',
    offered: ['Playlists'],
    absent: ['Overview', 'Explorer', 'Applications'],
  },
] as const;

for (const module of MODULES) {
  test(`${module.name} opens on the page a role may read`, async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { capabilityPermissions: [...module.permissions] });

    await page.goto(module.enters);

    await expect(page).toHaveURL(module.opens);
    const tabs = page.locator(module.tabs);

    for (const tab of module.offered) {
      await expect(tabs.getByRole('link', { name: tab, exact: true })).toBeVisible();
    }

    for (const tab of module.absent) {
      await expect(tabs.getByRole('link', { name: tab, exact: true })).toHaveCount(0);
    }
  });
}

test('a page without the permission still says so at its own address', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { capabilityPermissions: ['schedules.read'] });

  await page.goto('/alerts/routing');

  await expect(page.getByText("You don't have access to this page")).toBeVisible();
  await expect(page).toHaveURL(/\/alerts\/routing$/);
});

test('a role holding every permission is offered every tab', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.goto('/alerts');

  await expect(page).toHaveURL(/\/alerts\/incidents$/);
  const tabs = page.getByTestId('alerts-subnav');

  for (const tab of ['Insights', 'Incidents', 'Rules', 'Routing', 'On-call schedules', 'Tickets', 'Groups']) {
    await expect(tabs.getByRole('link', { name: tab, exact: true })).toBeVisible();
  }
});
