import { readFileSync, mkdirSync } from 'node:fs';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
const market = '/Users/gagral/code/molesignal/molesignal-marketplace';
const output = '/private/tmp/molesignal-marketplace-captures';
mkdirSync(output, { recursive: true });
test.use({ viewport: { width: 1280, height: 900 } });
for (const id of ['kubernetes-overview', 'postgresql-performance', 'service-golden-signals']) {
  test(`capture ${id}`, async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light', preferences: { theme: 'light' } });
    await page.goto('/signin');
    const model = JSON.parse(readFileSync(`${market}/public/resources/${id}.json`, 'utf8'));
    await page.evaluate(async model => {
      const response = await fetch('/api/v1/dashboards/d1', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title: model.title, model }) });
      if (!response.ok) throw new Error('Cannot seed dashboard');
    }, model);
    await page.route('**/api/v1/query', route => {
      const body = route.request().postDataJSON();
      const start = Number(body.timeRange?.start || 1779526800000000), end = Number(body.timeRange?.end || 1779530400000000);
      return route.fulfill({ json: { columns: ['_timestamp', 'value'], rows: Array.from({length: 61}, (_, i) => [start + (end-start)*i/60, Math.round((20 + Math.sin(i/5)*3)*100)/100]), scannedRows: 61, tookMs: 5 } });
    });
    await page.goto('/dashboards/d1?reportRender=1');
    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'ready', { timeout: 20000 });
    await expect(page.locator('[data-printed-panel]')).toHaveCount(3);
    await page.evaluate(() => document.fonts.ready);
    await page.locator('[data-report-document-body]').screenshot({ path: `${output}/${id}.png`, animations: 'disabled' });
  });
}
for (const id of ['kubernetes-restarts', 'service-error-rate']) {
  test(`capture ${id}`, async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light', preferences: { theme: 'light' } });
    await page.goto('/alerts/rules');
    await page.getByTestId('rule-transfer').click();
    await page.getByTestId('rule-import').click();
    const drawer = page.getByRole('dialog', { name: 'Import rules' });
    await drawer.getByRole('textbox', { name: 'Content' }).fill(readFileSync(`${market}/public/resources/${id}.yaml`, 'utf8'));
    await drawer.getByRole('textbox', { name: 'Content' }).evaluate(node => { node.scrollTop = 0; });
    await page.evaluate(() => document.fonts.ready);
    await drawer.screenshot({ path: `${output}/${id}.png`, animations: 'disabled' });
  });
}
test('capture alert-notification', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { theme: 'light', preferences: { theme: 'light' } });
  await page.route('**/api/v1/notify/templates', route => route.fulfill({ json: [] }));
  await page.goto('/settings/notify/templates');
  await page.getByRole('button', { name: 'New template' }).click();
  const drawer = page.getByRole('dialog');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('告警通知正文');
  await drawer.getByRole('textbox', { name: /^Message body/ }).fill(readFileSync(`${market}/public/resources/alert-notification.txt`, 'utf8'));
  await page.evaluate(() => document.fonts.ready);
  await drawer.screenshot({ path: `${output}/alert-notification.png`, animations: 'disabled' });
});
