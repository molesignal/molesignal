/**
 * Topology fit
 *
 * The service graph has to end up centred in its canvas: on the first load,
 * when the graph is laid out for a canvas size it has only just measured, and
 * again after every later re-layout. A fit that runs before the new nodes are
 * measured leaves the view where the previous layout was, so each check waits
 * for the nodes to stop moving and then looks at where they are.
 *
 * The zoom controls share the canvas with the minimap and must stay clickable.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface Margins {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/**
 * Free space between the canvas edges and the box around all nodes, read once
 * the nodes have kept still for half a second. That outlasts a re-layout and
 * the 180 ms fit animation.
 */
async function settledNodeMargins(page: Page): Promise<Margins> {
  await page.locator('.react-flow__node').first().waitFor({ state: 'visible', timeout: 30_000 });
  return page.locator('.react-flow').first().evaluate(
    (flow) =>
      new Promise<Margins>((resolve) => {
        const read = (): Margins => {
          const canvas = flow.getBoundingClientRect();
          const boxes = [...flow.querySelectorAll('.react-flow__node')].map((node) =>
            node.getBoundingClientRect(),
          );
          return {
            left: Math.min(...boxes.map((box) => box.left)) - canvas.left,
            right: canvas.right - Math.max(...boxes.map((box) => box.right)),
            top: Math.min(...boxes.map((box) => box.top)) - canvas.top,
            bottom: canvas.bottom - Math.max(...boxes.map((box) => box.bottom)),
          };
        };

        let last = read();
        let stillSince = performance.now();
        const check = () => {
          const current = read();
          if (JSON.stringify(current) !== JSON.stringify(last)) {
            last = current;
            stillSince = performance.now();
          } else if (performance.now() - stillSince >= 500) {
            resolve(current);
            return;
          }
          setTimeout(check, 50);
        };
        check();
      }),
  );
}

/** The settled graph is centred to the pixel on both axes. */
async function expectSettledGraphCentred(page: Page): Promise<void> {
  const margins = await settledNodeMargins(page);
  expect(Math.abs(margins.left - margins.right), 'left and right margins').toBeLessThanOrEqual(1);
  expect(Math.abs(margins.top - margins.bottom), 'top and bottom margins').toBeLessThanOrEqual(1);
}

test.describe('topology fit', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the graph is centred once the first load settles', async ({ page }) => {
    await page.goto('/_demo/topology?nodes=24');

    await expectSettledGraphCentred(page);
  });

  test('the graph is centred again after every re-layout', async ({ page }) => {
    await page.goto('/traces?tab=service-graph');
    await expectSettledGraphCentred(page);

    // Each of these lays the three services out differently.
    for (const name of ['Graph view', 'Tree view', 'Vertical', 'Horizontal']) {
      await page.getByRole('button', { name }).click();
      await expect(page.getByRole('button', { name })).toHaveAttribute('aria-pressed', 'true');
      await expectSettledGraphCentred(page);
    }
  });

  test('the zoom controls are not covered by the minimap', async ({ page }) => {
    await page.goto('/_demo/topology?nodes=24');
    await expect(page.locator('.react-flow__minimap')).toBeVisible({ timeout: 30_000 });

    // A trial click runs the hit-target check without changing the view.
    for (const control of ['zoomin', 'zoomout', 'fitview']) {
      await page.locator(`.react-flow__controls-${control}`).click({ trial: true, timeout: 5_000 });
    }
  });
});
