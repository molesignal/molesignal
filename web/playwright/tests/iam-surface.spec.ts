import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const IAM_USERS = [
  {
    id: 'user-owner',
    email: 'owner@example.com',
    display_name: 'Owner',
    is_root: true,
    disabled: false,
    status: 'active',
    display_role: 'Owner',
    roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
    team_names: [],
    login_method: 'password',
    last_active_at_micros: 1_783_800_000_000_000,
    created_at_micros: 1_783_700_000_000_000,
  },
  {
    id: 'user-viewer',
    email: 'viewer@example.com',
    display_name: 'Viewer',
    is_root: false,
    disabled: false,
    status: 'active',
    display_role: 'Viewer',
    roles: [{ id: 'role-viewer', key: 'viewer', name: 'Viewer', builtin: true }],
    team_names: ['Support'],
    login_method: 'password',
    last_active_at_micros: 1_783_700_000_000_000,
    created_at_micros: 1_783_600_000_000_000,
  },
];

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/users', (route) =>
    route.fulfill({ json: { items: IAM_USERS } }),
  );
});

test('uses tonal IAM surfaces instead of page and table dividers', async ({
  page,
}) => {
  await page.goto('/iam/users');

  const workspace = page.locator('[data-iam-surface-workspace]');
  const filterSurface = page.locator('[data-iam-filter-surface]');
  const table = page.locator('[data-data-table]');
  const headerCell = table.locator('thead th').first();
  const rows = table.locator('tbody tr');

  await expect(workspace).toBeVisible();
  await expect(filterSurface).toBeVisible();
  await expect(rows).toHaveCount(2);

  for (const element of [
    filterSurface,
    table.locator('thead tr'),
    ...(await rows.all()),
  ]) {
    for (const side of ['top', 'right', 'bottom', 'left'] as const) {
      await expect(element).toHaveCSS(`border-${side}-width`, '0px');
    }
  }

  const [workspaceBackground, filterBackground, headerBackground] =
    await Promise.all([
      workspace.evaluate((element) => getComputedStyle(element).backgroundColor),
      filterSurface.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      ),
      headerCell.evaluate((element) => getComputedStyle(element).backgroundColor),
    ]);
  expect(filterBackground).not.toBe(workspaceBackground);
  expect(headerBackground).toBe(filterBackground);

  const rowBackgrounds = await rows.evaluateAll((elements) =>
    elements.map((element) => getComputedStyle(element).backgroundColor),
  );
  expect(rowBackgrounds[0]).not.toBe(rowBackgrounds[1]);
});
