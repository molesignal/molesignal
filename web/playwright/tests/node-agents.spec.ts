import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('node agents', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('opens on the nodes and keeps one title over three tabs', async ({ page }) => {
    await page.goto('/node-agents');
    await expect(page).toHaveURL(/\/node-agents\/nodes$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Node agents');

    const tabs = page.getByTestId('node-agents-subnav');
    await expect(tabs.getByRole('link')).toHaveCount(3);

    for (const [name, path] of [
      ['Tasks', '/node-agents/tasks'],
      ['Enrollment', '/node-agents/enrollment'],
      ['Nodes', '/node-agents/nodes'],
    ] as const) {
      await tabs.getByRole('link', { name, exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('Node agents');
    }
  });

  test('lists nodes by status and narrows them', async ({ page }) => {
    await page.goto('/node-agents/nodes');
    await expect(page.getByRole('row')).toHaveCount(4);
    // The node that wants attention comes first.
    await expect(page.getByRole('row').nth(1)).toContainText('db-1.prod');
    await expect(page.locator('[data-node-status="offline"]')).toHaveCount(1);

    await page.getByPlaceholder('Search by host, version or label').fill('rack=r1');
    await expect(page.getByRole('row')).toHaveCount(2);
    await expect(page.getByRole('row').nth(1)).toContainText('web-1.prod');
  });

  test('drains a node and revokes another with a reason', async ({ page }) => {
    await page.goto('/node-agents/nodes/node-web-1');
    const drawer = page.getByRole('dialog');
    await expect(drawer).toContainText('web-1.prod');
    await expect(drawer).toContainText('Applied r2 · desired r2');

    await drawer.getByRole('button', { name: 'Drain' }).click();
    const drain = page.getByRole('dialog').filter({ hasText: 'Drain web-1.prod' });
    await drain.getByRole('button', { name: 'Drain' }).click();
    await expect(page.getByRole('dialog').filter({ hasText: 'web-1.prod' }).first()).toContainText('Draining');

    await page.goto('/node-agents/nodes/node-db-1');
    await page.getByRole('dialog').getByRole('button', { name: 'Revoke' }).click();
    const revoke = page.getByRole('dialog').filter({ hasText: 'Revoke db-1.prod' });
    await expect(revoke.getByRole('button', { name: 'Revoke' })).toBeDisabled();
    await revoke.getByLabel('Reason').fill('Decommissioned');
    await revoke.getByRole('button', { name: 'Revoke' }).click();
    await expect(page.locator('[data-node-status="revoked"]').first()).toBeVisible();
  });

  test('edits a node configuration as a form', async ({ page }) => {
    await page.goto('/node-agents/nodes/node-web-1');
    const drawer = page.getByRole('dialog');
    await expect(drawer).toContainText('Collectors off');
    await expect(drawer).toContainText('ras');

    await drawer.getByRole('button', { name: 'Edit configuration' }).click();
    const form = page.getByRole('dialog').filter({ hasText: 'Configuration of web-1.prod' });
    await expect(form.getByRole('checkbox', { name: 'ras', exact: true })).not.toBeChecked();
    await form.getByRole('checkbox', { name: 'oom', exact: true }).click();
    await form.getByRole('switch', { name: 'Off-CPU tracing' }).click();
    await form.getByLabel('Container profiles per window').fill('2000');
    await expect(form.getByRole('button', { name: 'Save revision' })).toBeDisabled();
    await form.getByLabel('Container profiles per window').fill('16');

    const saved = page.waitForRequest(
      (request) => request.method() === 'PUT' && request.url().endsWith('/node-agents/nodes/node-web-1/config'),
    );
    await form.getByRole('button', { name: 'Save revision' }).click();
    expect((await saved).postDataJSON()).toEqual({
      config: {
        collectors: { deny: ['oom', 'ras'] },
        profiling: { oncpu: true, offcpu: true, max_workloads: 16 },
      },
    });
  });

  test('runs a task on a node and cancels a running one', async ({ page }) => {
    await page.goto('/node-agents/tasks');
    await expect(page.getByRole('row')).toHaveCount(3);
    await expect(page.getByTestId('node-task-quota')).toContainText('Active per node: 4');

    await page.getByRole('button', { name: 'Run a task' }).first().click();
    const run = page.getByRole('dialog').filter({ hasText: 'Run a task' });
    // The spec is a form: a tool and its own options, written as the JSON the node reads.
    await run.getByLabel('Kind').click();
    await page.getByRole('option', { name: 'Tool', exact: true }).click();
    await run.getByLabel(/^Tool/).click();
    await page.getByRole('option', { name: 'tcpshark' }).click();
    await run.getByRole('switch', { name: 'Count tail loss probes' }).click();
    await run.getByLabel('Window (seconds)').fill('5000');
    await expect(run.getByText('Between 1 and 3600 seconds.')).toBeVisible();
    await expect(run.getByRole('button', { name: 'Run a task' })).toBeDisabled();
    await run.getByLabel('Window (seconds)').fill('30');

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/node-agents/tasks'),
    );
    await run.getByRole('button', { name: 'Run a task' }).click();
    expect((await created).postDataJSON()).toMatchObject({
      nodeId: 'node-web-1',
      kind: 'tool',
      spec: { tool: 'tcpshark', duration_secs: 30, mode: 'retransmit', enable_tlp: true, max_events_per_second: 100 },
    });
    await expect(page).toHaveURL(/\/node-agents\/tasks\/9e8d7c6b-/);
    await expect(page.getByRole('dialog')).toContainText('Queued');
    await page.keyboard.press('Escape');
    await expect(page.getByRole('row')).toHaveCount(4);

    await page.getByRole('row').filter({ hasText: 'Running' }).click();
    const task = page.getByRole('dialog');
    await task.getByRole('button', { name: 'Cancel task' }).click();
    await expect(task).toContainText('cancel requested');
  });

  test('issues an enrollment token once and revokes it', async ({ page }) => {
    await page.goto('/node-agents/enrollment');
    await expect(page.getByRole('row')).toHaveCount(2);

    await page.getByRole('button', { name: 'Issue token' }).click();
    const create = page.getByRole('dialog').filter({ hasText: 'Issue an enrollment token' });
    await create.getByLabel('Description').fill('Staging hosts');
    await create.getByLabel('Labels').fill('env=staging');
    await create.getByRole('button', { name: 'Issue token' }).click();

    const issued = page.getByRole('dialog').filter({ hasText: 'Enrollment token' });
    await expect(issued).toContainText('msnode_');
    await expect(issued).toContainText('https://localhost:5086');
    await page.keyboard.press('Escape');

    const row = page.getByRole('row').filter({ hasText: 'Staging hosts' });
    await expect(row).toContainText('Active');
    await row.getByRole('button', { name: 'Revoke' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Revoke' }).click();
    await expect(row).toContainText('Revoked');
  });
});
