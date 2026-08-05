/**
 * 03 — topology drill
 *
 * Opens a service frame via palette → topology renders mocked nodes → click
 * the `web` node → verify a second frame is pushed (drill-down).
 *
 * Uses the `topology-node-<id>` testid (added in ServiceNode) so the spec
 * is robust to ReactFlow class-name churn across versions.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('topology drill', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('click unhealthy node pushes a service drawer', async ({ page }) => {
    await page.goto('/investigate');
    await page.keyboard.press('Meta+K');
    await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();
    await page.keyboard.type('web');
    // Click the service row directly — cmdk's default selection is unreliable
    // when the typed value fuzzy-matches several static actions.
    await page.locator('[cmdk-item][data-value="service:web:web"]').click();

    // Topology canvas mounts with mocked nodes (web, api, db). `.first()`
    // guards against the brief window after click where two `web` nodes
    // co-exist (original drawer + just-pushed drawer) — the strict locator
    // would otherwise complain on Playwright's internal action retry.
    const webNode = page.getByTestId('topology-node-web').first();
    await webNode.waitFor({ state: 'visible', timeout: 10_000 });
    await webNode.click();

    // A second `service` frame should be pushed onto the stack — verify by
    // counting topology nodes (each ServiceFrame renders its own topology so
    // `web` appears twice once the second frame mounts).
    await expect(page.getByTestId('topology-node-web')).toHaveCount(2, { timeout: 10_000 });
  });

  test('service nodes can be dragged to a new canvas position', async ({ page }) => {
    await page.goto('/service-graph');

    // Use the central node so the drag motion remains inside compact canvases.
    const serviceNode = page.getByTestId('topology-node-api').first();
    await serviceNode.waitFor({ state: 'visible', timeout: 10_000 });
    const reactFlowNode = serviceNode.locator('..');
    await expect(reactFlowNode).toHaveClass(/react-flow__node/);

    const before = await reactFlowNode.boundingBox();
    expect(before).not.toBeNull();
    if (!before) return;

    // Start on the explicit icon handle rather than the clickable service-name
    // reference, which intentionally remains a non-drag affordance.
    const dragHandle = serviceNode.locator('[data-topology-drag-handle]');
    const handleBox = await dragHandle.boundingBox();
    expect(handleBox).not.toBeNull();
    if (!handleBox) return;
    const startX = handleBox.x + handleBox.width / 2;
    const startY = handleBox.y + handleBox.height / 2;
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 160, startY + 80, { steps: 8 });
    await page.mouse.up();

    const after = await reactFlowNode.boundingBox();
    expect(after).not.toBeNull();
    expect((after?.x ?? before.x) - before.x).toBeGreaterThan(100);
    expect((after?.y ?? before.y) - before.y).toBeGreaterThan(40);
  });

  test('trace service graph defaults to a searchable horizontal tree', async ({ page }) => {
    await page.goto('/service-graph');

    await expect(page.getByTestId('service-graph-canvas')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Tree view' })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: 'Horizontal' })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByText('3 services', { exact: true })).toBeVisible();

    await page.getByRole('textbox', { name: 'Search services in the graph' }).fill('api');
    await expect(page.getByTestId('topology-node-api')).toHaveAttribute('data-search-match', 'true');
    await expect(page.getByTestId('topology-node-web')).toHaveAttribute('data-search-match', 'false');
    await expect(page.getByText('1 / 3 services', { exact: true })).toBeVisible();

    await page.getByRole('button', { name: 'Vertical' }).click();
    await expect(page.getByRole('button', { name: 'Vertical' })).toHaveAttribute('aria-pressed', 'true');
  });
});
