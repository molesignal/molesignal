import { readFileSync, mkdirSync } from 'node:fs';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
const market = '/Users/gagral/code/molesignal/molesignal-marketplace';
const output = '/private/tmp/molesignal-marketplace-seed-captures';
const resources: { id: string; name: string; kind: string; file: string }[] = JSON.parse(readFileSync(`${market}/data/capture-resources.json`, 'utf8'));
mkdirSync(output, { recursive: true });
test.use({ viewport: { width: 1280, height: 900 } });
for (const resource of resources) {
  test(`capture ${resource.id}`, async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light', preferences: { theme: 'light' } });
    if (resource.kind === 'Dashboard') {
      await page.goto('/signin');
      const model = JSON.parse(readFileSync(`${market}/public/resources/${resource.file}`, 'utf8'));
      await page.evaluate(async model => {
        const response = await fetch('/api/v1/dashboards/d1', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title: model.title, model }) });
        if (!response.ok) throw new Error('Cannot seed dashboard');
      }, model);
      await page.route('**/api/v1/query', route => {
        const body = route.request().postDataJSON();
        const query = JSON.stringify(body);
        const start = Number(body.timeRange?.start || 1779526800000000), end = Number(body.timeRange?.end || 1779530400000000);
        let baseline = 30;
        if (/memory|resident|filesystem/.test(query) && !/100|0\.1/.test(query)) baseline = 1024 * 1024 * 256;
        if (/probe_success/.test(query)) baseline = 1;
        if (/probe_http_status_code/.test(query)) baseline = 200;
        if (/probe_duration/.test(query)) baseline = 0.18;
        if (/failures_total|accepted.*handled/.test(query)) baseline = 0;
        const constant = /probe_success|probe_http_status_code/.test(query) || baseline === 0;
        return route.fulfill({ json: { columns: ['_timestamp', 'value'], rows: Array.from({length: 61}, (_, i) => [start + (end-start)*i/60, constant ? baseline : baseline * (1 + Math.sin(i/5)*0.12)]), scannedRows: 61, tookMs: 5 } });
      });
      await page.goto('/dashboards/d1?reportRender=1');
      await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'ready', { timeout: 20000 });
      await expect(page.locator('[data-printed-panel]')).toHaveCount(model.elements.length);
      await page.evaluate(() => document.fonts.ready);
      await page.locator('[data-report-document-body]').screenshot({ path: `${output}/${resource.id}.png`, animations: 'disabled' });
    } else if (resource.kind === '告警规则') {
      await page.goto('/alerts/rules');
      await page.getByTestId('rule-transfer').click();
      await page.getByTestId('rule-import').click();
      const drawer = page.getByRole('dialog', { name: 'Import rules' });
      await drawer.getByRole('textbox', { name: 'Content' }).fill(readFileSync(`${market}/public/resources/${resource.file}`, 'utf8'));
      await drawer.getByRole('textbox', { name: 'Content' }).evaluate(node => { node.scrollTop = 0; });
      await page.evaluate(() => document.fonts.ready);
      await drawer.screenshot({ path: `${output}/${resource.id}.png`, animations: 'disabled' });
    } else {
      await page.route('**/api/v1/notify/templates', route => route.fulfill({ json: [] }));
      await page.goto('/settings/notify/templates');
      await page.getByRole('button', { name: 'New template' }).click();
      const drawer = page.getByRole('dialog');
      await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill(resource.name);
      await drawer.getByRole('textbox', { name: /^Message body/ }).fill(readFileSync(`${market}/public/resources/${resource.file}`, 'utf8'));
      await page.evaluate(() => document.fonts.ready);
      await drawer.screenshot({ path: `${output}/${resource.id}.png`, animations: 'disabled' });
    }
  });
}
