/**
 * i18n length audit — capture every key page in both locales so the
 * designer can eyeball the ~1.6× zh / en length difference for overflow,
 * truncation, and toolbar wrapping.
 *
 * These diagnostic screenshots are written to `test-results/i18n-length/`
 * with `page.screenshot({ path })`, so each run refreshes the same artifacts.
 *
 * Run with:
 *   pnpm playwright tests/07-i18n-length.spec.ts
 *
 * After the run, open the two PNGs side by side (e.g. `open
 * test-results/i18n-length/sidebar-en.png
 * test-results/i18n-length/sidebar-zh.png`) and
 * walk down the column looking for: overflowed text in the sidebar group
 * headings, wrapped pills in the alerts toolbar, truncated KPI labels,
 * and toolbar buttons that now wrap onto two lines.
 *
 * The shared mock gives the Logs and Traces explorers nothing to list, so
 * `logs` and `traces` are captures of their empty state. `logs-results` and
 * `traces-results` are the same two pages with results from `mockExplorers`:
 * the copy of the result head, the histogram, the list and its paging is on
 * the page only there.
 */
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Locale } from '../../src/i18n';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults, type Explorer } from '../fixtures/mockExplorers';

const __dirname = dirname(fileURLToPath(import.meta.url));

const PREFS_KEY = 'molesignal-ui-prefs';

const OUT_DIR = resolve(__dirname, '../../test-results/i18n-length');

const LOCALES: Locale[] = ['en-us', 'zh-cn'];

// Pages most likely to surface the 1.6× length differential: dense
// toolbars, multi-column tables, KPI strips, sidebar group headings.
const PAGES: Array<{
  slug: string;
  route: string;
  signedIn: boolean;
  /** The explorer the page is, where it is captured with results on it. */
  results?: Explorer;
}> = [
  { slug: 'signin', route: '/signin', signedIn: false },
  { slug: 'home', route: '/home', signedIn: true },
  { slug: 'sidebar', route: '/home', signedIn: true }, // same route, captured for sidebar comparison
  { slug: 'logs', route: '/logs', signedIn: true },
  { slug: 'logs-results', route: '/logs', signedIn: true, results: 'logs' },
  { slug: 'metrics', route: '/metrics', signedIn: true },
  { slug: 'traces', route: '/traces', signedIn: true },
  { slug: 'traces-results', route: '/traces', signedIn: true, results: 'traces' },
  { slug: 'alerts', route: '/alerts', signedIn: true },
  { slug: 'streams', route: '/streams', signedIn: true },
  { slug: 'dashboards', route: '/dashboards', signedIn: true },
  { slug: 'iam-users', route: '/iam/users', signedIn: true },
  { slug: 'settings-general', route: '/settings/general', signedIn: true },
  { slug: 'datasource', route: '/datasource', signedIn: true },
  { slug: 'pipelines', route: '/pipelines', signedIn: true },
  { slug: 'apm-overview', route: '/apm/overview', signedIn: true },
  { slug: 'rum-overview', route: '/rum/overview', signedIn: true },
  { slug: 'rum-sessions', route: '/rum/sessions', signedIn: true },
];

for (const locale of LOCALES) {
  const localeTag = locale === 'zh-cn' ? 'zh' : 'en';
  test.describe(`i18n length @ ${locale}`, () => {
    for (const pageDef of PAGES) {
      test(`${pageDef.slug}`, async ({ page, mockServer }) => {
        // The shared mock backend serves every page its data and pins the
        // clock, so the en and zh captures differ in copy and nothing else.
        // A signed-in page takes its language from the saved preferences: the
        // shell applies what the backend returns over whatever the browser
        // remembered, so the language is saved there, not only seeded.
        await mountMockRoutes(page, mockServer.port, {
          theme: 'dark',
          density: 'normal',
          ...(pageDef.signedIn
            ? { preferences: { theme: 'dark', density: 'normal', language: locale } }
            : { signedOut: true }),
        });
        if (pageDef.results) await serveExplorerResults(page);
        // useThemeStore is persisted by zustand `persist` under
        // `molesignal-ui-prefs` as `{state, version}`. Seeding it puts the
        // first paint in the right language, and it is all the sign-in page
        // has to go by.
        await page.addInitScript(
          ({ prefsKey, prefs }) => {
            localStorage.setItem(prefsKey, JSON.stringify(prefs));
          },
          {
            prefsKey: PREFS_KEY,
            prefs: { state: { palette: 'default', language: locale }, version: 0 },
          },
        );

        await page.goto(pageDef.route);
        // Let lazy-loaded chunks settle (Monaco, reactflow, uPlot are
        // all import()'d). Without this the screenshot can capture a
        // skeleton frame and the en / zh comparison becomes noise.
        await page.waitForLoadState('networkidle');
        // Wait for `ThemeBootstrap` to flush its language effect — without
        // this the screenshot can land between the first render and the
        // moment the saved language reaches i18next, producing en-content
        // under a zh filename.
        await page.waitForFunction(
          (expected) => document.documentElement.getAttribute('lang') === expected,
          locale,
          { timeout: 5_000 },
        );
        // The capture is only worth comparing when the page itself rendered:
        // a shell that could not load its access, or a crashed route, looks
        // the same in both languages.
        await expect(page.getByText('Unexpected Application Error!')).toHaveCount(0);
        if (pageDef.signedIn) {
          await expect(
            page.getByTestId('primary-sidebar').getByRole('link').first(),
          ).toBeVisible();
        } else {
          await expect(page.getByTestId('signin-card')).toBeVisible();
        }
        if (pageDef.results) await expectExplorerResults(page, pageDef.results);

        await page.screenshot({
          path: `${OUT_DIR}/${pageDef.slug}-${localeTag}.png`,
          fullPage: true,
          animations: 'disabled',
        });
      });
    }
  });
}
