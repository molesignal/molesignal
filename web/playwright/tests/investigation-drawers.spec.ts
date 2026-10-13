/**
 * Investigation drawers
 *
 * A frame has one drawer from the moment it is pushed until it has left: the
 * drawer is mounted once, and when the frame closes that same drawer slides
 * out.
 *
 * The stack writes itself to the address a moment after every change, and the
 * address is read back into the stack. A frame that comes back from that trip
 * under a new identity gets a second drawer: its content is mounted again and,
 * outside the frozen clock of the mock, fetches its data again. So the checks
 * here record every drawer the page mounts instead of looking only at the
 * drawers that are there in the end.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface Drawer {
  id: string;
  index: string;
}

interface Leave {
  /** The drawer that left is the element the frame already had. */
  sameElement: boolean;
  height: number;
  /** It is on its way out, not just still there. */
  moving: boolean;
  /** Where the drawer beneath it stood at that moment. */
  beneathLeft: number;
}

interface DrawerLog {
  mounted: Drawer[];
  leaves: Leave[];
  top: Element | null;
}

const DRAWER = 'aside[data-frame-id]';

/** Records every drawer added to the page from now on, and every drawer that starts to leave. */
async function recordDrawers(page: Page): Promise<void> {
  await page.evaluate((selector) => {
    const log: DrawerLog = { mounted: [], leaves: [], top: null };
    (window as unknown as { __drawers: DrawerLog }).__drawers = log;

    new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes) {
          if (!(node instanceof HTMLElement)) continue;
          const drawers = node.matches(selector)
            ? [node]
            : [...node.querySelectorAll<HTMLElement>(selector)];
          for (const drawer of drawers) {
            log.mounted.push({
              id: drawer.dataset.frameId ?? '',
              index: drawer.dataset.frameIndex ?? '',
            });
          }
        }

        const drawer = record.target;
        if (record.type !== 'attributes' || !(drawer instanceof HTMLElement)) continue;
        if (!drawer.matches(`${selector}[data-leaving]`)) continue;
        const beneath = document.querySelector(`${selector}:not([data-leaving])`);
        log.leaves.push({
          sameElement: drawer === log.top,
          height: drawer.getBoundingClientRect().height,
          moving: drawer.getAnimations().length > 0,
          beneathLeft: beneath?.getBoundingClientRect().left ?? Number.NaN,
        });
      }
    }).observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['data-leaving'],
    });
  }, DRAWER);
}

async function stackInAddress(page: Page): Promise<string | null> {
  return new URL(page.url()).searchParams.get('stack');
}

/**
 * Every drawer mounted so far and the drawers on the page, read once the
 * stack has reached the address and the drawers have then kept still for half
 * a second. The address is read back into the stack as soon as it is written;
 * a drawer mounted because of that, and the 200 ms slide-out of the one it
 * replaced, are over well within that time.
 */
async function settledDrawers(
  page: Page,
  addressBefore: string | null,
): Promise<{ mounted: Drawer[]; open: Drawer[] }> {
  await expect.poll(() => stackInAddress(page), { timeout: 10_000 }).not.toBe(addressBefore);

  return page.evaluate(
    (selector) =>
      new Promise<{ mounted: Drawer[]; open: Drawer[] }>((resolve) => {
        const log = (window as unknown as { __drawers: DrawerLog }).__drawers;
        const read = () => ({
          mounted: [...log.mounted],
          open: [...document.querySelectorAll<HTMLElement>(selector)].map((drawer) => ({
            id: drawer.dataset.frameId ?? '',
            index: drawer.dataset.frameIndex ?? '',
          })),
        });

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
    DRAWER,
  );
}

/** Opens the `web` service frame from the palette: the first drawer. */
async function openWebService(page: Page): Promise<{ mounted: Drawer[]; open: Drawer[] }> {
  await page.keyboard.press('Meta+K');
  await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();
  await page.keyboard.type('web');
  // Click the service row directly — cmdk's default selection is unreliable
  // when the typed value fuzzy-matches several static actions.
  await page.locator('[cmdk-item][data-value="service:web:web"]').click();

  const first = page.locator(`${DRAWER}[data-frame-index="0"]`);
  await expect(first.getByTestId('topology-node-api')).toBeVisible({ timeout: 10_000 });
  return settledDrawers(page, null);
}

/** Opens the `api` service frame from the graph in the first drawer: a second drawer on top. */
async function drillIntoApi(page: Page): Promise<{ mounted: Drawer[]; open: Drawer[] }> {
  const address = await stackInAddress(page);
  await page.locator(`${DRAWER}[data-frame-index="0"]`).getByTestId('topology-node-api').click();

  const second = page.locator(`${DRAWER}[data-frame-index="1"]`);
  await expect(second.getByTestId('topology-node-api')).toBeVisible({ timeout: 10_000 });
  return settledDrawers(page, address);
}

test.describe('investigation drawers', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a frame keeps the one drawer it was pushed with', async ({ page }) => {
    await page.goto('/investigate');
    await recordDrawers(page);

    const opened = await openWebService(page);
    expect(opened.mounted, 'drawers mounted for the first frame').toHaveLength(1);
    expect(opened.open).toEqual(opened.mounted);

    const pushed = await drillIntoApi(page);
    expect(
      pushed.mounted.map((drawer) => drawer.index),
      'drawers mounted for the two frames',
    ).toEqual(['0', '1']);
    expect(pushed.open).toEqual(pushed.mounted);
  });

  test('closing the top drawer slides that drawer out over the one beneath', async ({ page }) => {
    await page.goto('/investigate');
    await recordDrawers(page);
    await openWebService(page);
    await drillIntoApi(page);

    const top = page.locator(`${DRAWER}[data-frame-index="1"]`);
    const height = (await top.boundingBox())?.height;
    await top.evaluate((drawer) => {
      (window as unknown as { __drawers: DrawerLog }).__drawers.top = drawer;
    });
    await top.getByRole('button', { name: 'Close frame' }).click();

    // The drawer beneath is the only one left, and it ends where it stood.
    await expect(page.locator(DRAWER)).toHaveCount(1);
    const beneathLeft = (await page.locator(DRAWER).boundingBox())?.x;
    const log = await page.evaluate(() => {
      const { mounted, leaves } = (window as unknown as { __drawers: DrawerLog }).__drawers;
      return { mounted, leaves };
    });

    expect(log.leaves, 'drawers seen leaving').toEqual([
      { sameElement: true, height, moving: true, beneathLeft },
    ]);
    // No drawer was mounted to play the slide-out.
    expect(log.mounted).toHaveLength(2);
  });
});
