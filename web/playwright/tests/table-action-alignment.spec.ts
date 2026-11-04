import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const ROUTES = [
  { path: '/streams', alignment: 'center' },
  { path: '/reports', alignment: 'right' },
  { path: '/alerts/rules', alignment: 'right' },
  { path: '/settings/notify/connectors', alignment: 'right' },
  { path: '/settings/notify/templates', alignment: 'right' },
  { path: '/alerts/escalations', alignment: 'right' },
  { path: '/iam/users', alignment: 'center' },
] as const;

const ALERT_RULE = {
  id: 'rule-alignment',
  orgId: 'acme-prod',
  name: 'Checkout error rate',
  description: 'Alignment fixture',
  enabled: true,
  kind: 'scheduled',
  query: {
    language: 'promql',
    statement: 'http_requests_total',
    periodSecs: 300,
    stream: { name: 'metrics', streamType: 'metrics' },
  },
  trigger: {
    operator: 'gt',
    threshold: 1,
    forPeriods: 1,
    silenceSecs: 300,
  },
  escalationPolicyId: 'policy-default',
  labels: { service: 'checkout' },
  annotations: {},
  lastState: { kind: 'healthy' },
};

const NOTIFY_TEMPLATE = {
  id: 'template-alignment',
  organizationId: 'acme-prod',
  name: 'Critical incident',
  body: '{{severity}} {{summary}}',
  format: 'text',
  createdAt: 1_783_700_000_000_000,
  updatedAt: 1_783_800_000_000_000,
};

const NOTIFY_CONNECTOR = {
  id: 'connector-alignment',
  organizationId: 'acme-prod',
  name: 'Primary email',
  connectorType: 'email_smtp',
  config: {
    host: 'smtp.example.com',
    port: 587,
    from: 'alerts@example.com',
  },
  capabilities: {
    directUser: true,
    group: false,
    richText: true,
    interactive: false,
    acknowledgement: false,
    attachments: true,
  },
  enabled: true,
  status: 'connected',
  lastTestedAt: null,
  lastTestStatus: null,
  lastTestError: null,
  createdAt: 1_783_700_000_000_000,
  updatedAt: 1_783_800_000_000_000,
};

const ESCALATION_POLICY = {
  id: 'escalation-alignment',
  orgId: 'acme-prod',
  name: 'Primary responders',
  steps: [
    {
      targets: [
        {
          kind: 'user',
          userId: 'user-primary',
        },
      ],
      ackTimeoutSecs: 300,
      minSeverity: null,
    },
  ],
  repeat: false,
  maxLoops: 1,
};

const EXTEND_TABLE = {
  tableName: 'customers',
  description: 'Customer ownership metadata',
  keyField: 'key',
  valueFields: [
    {
      name: 'owner',
      fieldType: 'string',
      required: false,
      description: 'Owning team',
    },
  ],
  rowCount: 3,
  updatedAt: 1_783_800_000_000_000,
  usageLocations: [],
};

const IAM_USER = {
  id: 'u1',
  email: 'admin@example.com',
  displayName: 'Root',
  isRoot: true,
  disabled: false,
  status: 'active',
  displayRole: 'Owner',
  roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
  teamNames: [],
  loginMethod: 'password',
  lastActiveAt: 1_783_800_000_000_000,
  createdAt: 1_783_700_000_000_000,
};

test.describe('table action column alignment', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  for (const { path, alignment } of ROUTES) {
    test(`${path} aligns the action header with row controls`, async ({
      page,
    }) => {
      if (path === '/alerts/rules') {
        await page.route('**/api/v1/alerts/rules', (route) =>
          route.fulfill({ json: [ALERT_RULE] }),
        );
      }
      if (path === '/settings/notify/connectors') {
        await page.route('**/api/v1/notify/connectors', (route) =>
          route.fulfill({ json: [NOTIFY_CONNECTOR] }),
        );
        await page.route('**/api/v1/notify/connector-types', (route) =>
          route.fulfill({ json: [] }),
        );
      }
      if (path === '/settings/notify/templates') {
        await page.route('**/api/v1/notify/templates', (route) =>
          route.fulfill({ json: [NOTIFY_TEMPLATE] }),
        );
      }
      if (path === '/alerts/escalations') {
        await page.route('**/api/v1/alerts/escalations', (route) =>
          route.fulfill({ json: [ESCALATION_POLICY] }),
        );
        await page.route('**/api/v1/schedules', (route) =>
          route.fulfill({ json: [] }),
        );
      }
      if (path === '/iam/users') {
        await page.route('**/api/v1/users', (route) =>
          route.fulfill({ json: { items: [IAM_USER] } }),
        );
      }
      await page.goto(path);

      const header = page.getByRole('columnheader', {
        name: 'Actions',
        exact: true,
      });
      await expect(header).toBeVisible({ timeout: 10_000 });
      await expect
        .poll(() =>
          header.evaluate((element) => getComputedStyle(element).textAlign),
        )
        .toBe(alignment);

      const actionCell = page.locator('tbody tr').first().getByRole('cell').last();
      const lastAction = actionCell.getByRole('button').last();
      await expect(lastAction).toBeVisible();

      if (
        path === '/settings/notify/connectors' ||
        path === '/alerts/escalations'
      ) {
        await expect(
          actionCell.getByRole('button', { name: 'Edit', exact: true }),
        ).toBeVisible();
      }

      if (alignment === 'center') {
        const firstAction = actionCell.getByRole('button').first();
        const [headerBox, firstBox, lastBox] = await Promise.all([
          header.boundingBox(),
          firstAction.boundingBox(),
          lastAction.boundingBox(),
        ]);
        expect(headerBox).not.toBeNull();
        expect(firstBox).not.toBeNull();
        expect(lastBox).not.toBeNull();
        const headerCenter = headerBox!.x + headerBox!.width / 2;
        const actionsCenter = (firstBox!.x + lastBox!.x + lastBox!.width) / 2;
        expect(Math.abs(headerCenter - actionsCenter)).toBeLessThanOrEqual(1);
      } else {
        const [cellContentRight, actionRight] = await Promise.all([
          actionCell.evaluate((element) => {
            const styles = getComputedStyle(element);
            return (
              element.getBoundingClientRect().right -
              Number.parseFloat(styles.paddingRight)
            );
          }),
          lastAction.evaluate((element) => element.getBoundingClientRect().right),
        ]);
        expect(Math.abs(cellContentRight - actionRight)).toBeLessThanOrEqual(1);
      }

      if (
        path === '/settings/notify/connectors' ||
        path === '/alerts/escalations'
      ) {
        await actionCell
          .getByRole('button', { name: 'Edit', exact: true })
          .click();
        await expect(page.getByRole('dialog')).toBeVisible();
      }
      if (path === '/settings/notify/templates') {
        await page.locator('tbody tr').first().click();
        await expect(page.getByRole('dialog')).toBeVisible();
      }
    });
  }

  test('/extend-tables aligns the status header with the status pill', async ({
    page,
  }) => {
    await page.route('**/api/v1/extend_tables', (route) =>
      route.fulfill({ json: [EXTEND_TABLE] }),
    );
    await page.goto('/extend-tables');

    const header = page.getByTestId('extend-table-status-header');
    const statusCell = page.getByTestId('extend-table-status-cell').first();
    const statusPill = statusCell.locator(':scope > span').last();
    await expect(header).toBeVisible({ timeout: 10_000 });
    await expect(statusPill).toBeVisible();
    await expect
      .poll(() =>
        header.evaluate((element) => getComputedStyle(element).textAlign),
      )
      .toBe('center');

    const [headerBox, pillBox] = await Promise.all([
      header.boundingBox(),
      statusPill.boundingBox(),
    ]);
    expect(headerBox).not.toBeNull();
    expect(pillBox).not.toBeNull();
    expect(
      Math.abs(
        headerBox!.x +
          headerBox!.width / 2 -
          (pillBox!.x + pillBox!.width / 2),
      ),
    ).toBeLessThanOrEqual(1);

    await expect(statusCell.getByRole('button')).toHaveCSS(
      'position',
      'absolute',
    );
  });
});
