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
  org_id: 'acme-prod',
  name: 'Checkout error rate',
  description: 'Alignment fixture',
  enabled: true,
  kind: 'scheduled',
  query: {
    language: 'promql',
    statement: 'http_requests_total',
    period_secs: 300,
    stream: { name: 'metrics', stream_type: 'metrics' },
  },
  trigger: {
    operator: 'gt',
    threshold: 1,
    for_periods: 1,
    silence_secs: 300,
  },
  escalation_policy_id: 'policy-default',
  labels: { service: 'checkout' },
  annotations: {},
  last_state: { kind: 'healthy' },
};

const NOTIFY_TEMPLATE = {
  id: 'template-alignment',
  organization_id: 'acme-prod',
  name: 'Critical incident',
  body: '{{severity}} {{summary}}',
  format: 'text',
  created_at: 1_783_700_000_000_000,
  updated_at: 1_783_800_000_000_000,
};

const NOTIFY_CONNECTOR = {
  id: 'connector-alignment',
  organization_id: 'acme-prod',
  name: 'Primary email',
  connector_type: 'email_smtp',
  config: {
    host: 'smtp.example.com',
    port: 587,
    from: 'alerts@example.com',
  },
  capabilities: {
    direct_user: true,
    group: false,
    rich_text: true,
    interactive: false,
    acknowledgement: false,
    attachments: true,
  },
  enabled: true,
  status: 'connected',
  last_tested_at: null,
  last_test_status: null,
  last_test_error: null,
  created_at: 1_783_700_000_000_000,
  updated_at: 1_783_800_000_000_000,
};

const ESCALATION_POLICY = {
  id: 'escalation-alignment',
  org_id: 'acme-prod',
  name: 'Primary responders',
  steps: [
    {
      targets: [
        {
          kind: 'user',
          user_id: 'user-primary',
        },
      ],
      ack_timeout_secs: 300,
      min_severity: null,
    },
  ],
  repeat: false,
  max_loops: 1,
};

const EXTEND_TABLE = {
  table_name: 'customers',
  description: 'Customer ownership metadata',
  key_field: 'key',
  value_fields: [
    {
      name: 'owner',
      field_type: 'string',
      required: false,
      description: 'Owning team',
    },
  ],
  row_count: 3,
  updated_at: 1_783_800_000_000_000,
  usage_locations: [],
};

const IAM_USER = {
  id: 'u1',
  email: 'admin@example.com',
  display_name: 'Root',
  is_root: true,
  disabled: false,
  status: 'active',
  display_role: 'Owner',
  roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
  team_names: [],
  login_method: 'password',
  last_active_at_micros: 1_783_800_000_000_000,
  created_at_micros: 1_783_700_000_000_000,
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
