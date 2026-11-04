import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const IAM_USERS = [
  {
    id: 'user-owner',
    email: 'owner@example.com',
    displayName: 'Owner',
    isRoot: true,
    disabled: false,
    status: 'active',
    displayRole: 'Owner',
    roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
    teamNames: [],
    loginMethod: 'password',
    lastActiveAt: 1_783_800_000_000_000,
    createdAt: 1_783_700_000_000_000,
  },
  {
    id: 'user-viewer',
    email: 'viewer@example.com',
    displayName: 'Viewer',
    isRoot: false,
    disabled: false,
    status: 'active',
    displayRole: 'Viewer',
    roles: [{ id: 'role-viewer', key: 'viewer', name: 'Viewer', builtin: true }],
    teamNames: ['Support'],
    loginMethod: 'password',
    lastActiveAt: 1_783_700_000_000_000,
    createdAt: 1_783_600_000_000_000,
  },
];

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/users', (route) =>
    route.fulfill({ json: { items: IAM_USERS } }),
  );
});

test('uses tonal IAM surfaces and alternating row backgrounds', async ({
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
  // The head is the band every table head is, a step off the card, with a rule under it.
  const cardBackground = await table.evaluate((element) => {
    for (let node = element.parentElement; node; node = node.parentElement) {
      const fill = getComputedStyle(node).backgroundColor;
      if (fill !== 'rgba(0, 0, 0, 0)') return fill;
    }
    return '';
  });
  const band = await page.evaluate(() => {
    const probe = document.createElement('span');
    probe.style.backgroundColor = 'var(--table-head-surface)';
    document.body.append(probe);
    const fill = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return fill;
  });
  expect(headerBackground).toBe(band);
  expect(headerBackground).not.toBe(cardBackground);
  await expect(headerCell).toHaveCSS('box-shadow', /inset/);

  const rowBackgrounds = await rows.evaluateAll((elements) =>
    elements.map((element) => getComputedStyle(element).backgroundColor),
  );
  expect(rowBackgrounds[0]).not.toBe(rowBackgrounds[1]);
});
