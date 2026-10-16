import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('business services', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lists the services in trouble first, each with what is behind it', async ({ page }) => {
    await page.goto('/business-services');

    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Business services');
    const rows = page.getByRole('list', { name: 'Business services' }).getByRole('listitem');
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toContainText('Public API');
    await expect(rows.nth(0)).toContainText('Partial outage');
    await expect(rows.nth(0)).toContainText('2 dependencies in trouble: Public API 5xx rate, Checkout API');
    await expect(rows.nth(1)).toContainText('Checkout');
    await expect(rows.nth(2)).toContainText('Sign-in');
    await expect(rows.nth(2)).toContainText('Operational');
  });

  test('opens a service with its causes, dependencies and status page components', async ({ page }) => {
    await page.goto('/business-services');
    await page.getByRole('link', { name: /^Public API/ }).click();

    await expect(page).toHaveURL(/\/business-services\/bs-public-api$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Public API');

    const health = page.getByRole('region', { name: 'Health' });
    await expect(health.getByRole('link', { name: 'Public API 5xx rate' }).first()).toBeVisible();
    await expect(health.getByRole('link', { name: 'Public API 5xx rate above 2%' })).toHaveAttribute(
      'href',
      '/alerts/incidents/inc-api-5xx',
    );
    await expect(health.getByText('Check failing')).toBeVisible();

    const dependencies = page.getByRole('region', { name: 'Dependencies' });
    await expect(dependencies.getByRole('listitem')).toHaveCount(3);
    await expect(dependencies.getByRole('link', { name: 'api-gateway' })).toHaveAttribute('href', '/apm/services/api-gateway');

    const components = page.getByRole('region', { name: 'Status page components' });
    await expect(components.getByRole('link', { name: 'Acme Cloud / Public API' })).toHaveAttribute(
      'href',
      '/status-pages/sp-acme-cloud/components/cmp-api',
    );
  });

  test('edits a service with its dependencies named', async ({ page }) => {
    await page.goto('/business-services/bs-checkout');
    await page.getByRole('button', { name: 'Edit' }).click();

    const drawer = page.getByRole('dialog', { name: 'Edit business service' });
    await expect(drawer.getByRole('textbox', { name: 'Name' })).toHaveValue('Checkout');
    // Every dependency keeps what it names once the lists arrive.
    await expect(drawer.getByRole('combobox', { name: 'Dependency', exact: true }).nth(0)).toContainText('Public API');
    await expect(drawer.getByRole('combobox', { name: 'Dependency', exact: true }).nth(1)).toContainText('Payments gateway');
    await expect(drawer.getByRole('combobox', { name: 'Dependency', exact: true }).nth(2)).toContainText('Checkout p95 latency');
  });
});
