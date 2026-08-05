/**
 * Focus-ring visual baseline
 * (web-a11y-baseline).
 *
 * For every viz demo route × every (theme, density) combo, navigates the
 * page, forces focus on a representative canvas container, then snapshots
 * the focused element. 16 PNG baselines (4 viz × 4 combos) are committed
 * under `a11y-focus-ring.spec.ts-snapshots/`.
 *
 * Demo routes are used (not the in-frame viz) because they render the
 * canvas without auth + investigation-stack setup, so the focus target is
 * a stable DOM node across runs.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

// QUARANTINE (follow-up: P2-T6): the committed focus-ring baselines are
// `*-chromium-darwin.png` only, so this suite cannot pass on the linux CI
// runner. Skip on linux until linux baselines are regenerated; still runs
// locally on darwin.
test.beforeEach(() => {
  test.skip(
    process.platform === 'linux',
    'Focus-ring baselines are darwin-only; linux baselines are a follow-up (P2-T6).',
  );
});

type Theme = 'dark' | 'light';
type Density = 'comfortable' | 'compact';

const VIZ: Array<{ slug: string; path: string; focusTarget: string }> = [
  { slug: 'timeseries', path: '/_demo/timeseries?n=10000', focusTarget: 'canvas' },
  { slug: 'trace', path: '/_demo/trace?spans=1000', focusTarget: 'canvas' },
  { slug: 'topology', path: '/_demo/topology?nodes=24', focusTarget: '.react-flow__node' },
  { slug: 'log', path: '/_demo/log?rows=200', focusTarget: '[aria-label="Log stream"]' },
];

const COMBOS: Array<{ theme: Theme; density: Density }> = [
  { theme: 'dark', density: 'comfortable' },
  { theme: 'dark', density: 'compact' },
  { theme: 'light', density: 'comfortable' },
  { theme: 'light', density: 'compact' },
];

test.describe('a11y: focus ring baselines', () => {
  for (const viz of VIZ) {
    for (const combo of COMBOS) {
      const name = `${viz.slug}-focus-${combo.theme}-${combo.density}`;
      test(name, async ({ page, mockServer }) => {
        await mountMockRoutes(page, mockServer.port, { theme: combo.theme, density: combo.density });
        await page.goto(viz.path);
        const target = page.locator(viz.focusTarget).first();
        await target.waitFor({ state: 'visible', timeout: 30_000 });
        // Force keyboard focus so :focus-visible fires (Tab order varies
        // by viz internals; assigning tabIndex=0 + .focus() gives a stable
        // focused target across the four combos).
        await target.evaluate((node) => {
          (node as HTMLElement).tabIndex = 0;
          (node as HTMLElement).focus();
        });
        await page.waitForTimeout(80);
        await expect(target).toHaveScreenshot(`${name}.png`, {
          animations: 'disabled',
          maxDiffPixelRatio: 0.005,
        });
      });
    }
  }
});
