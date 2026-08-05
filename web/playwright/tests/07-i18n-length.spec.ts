/**
 * M3.2 i18n length audit — capture every key page in both locales so the
 * designer can eyeball the ~1.6× zh / en length difference for overflow,
 * truncation, and toolbar wrapping.
 *
 * Not a regression suite — these screenshots are diagnostic. They are
 * written to `.design/molesignal-redesign/screenshots/m3/i18n/` using
 * `page.screenshot({ path })` instead of
 * Playwright's snapshot baselines so a re-run overwrites the diff folder
 * rather than failing on pixel mismatch.
 *
 * Run with:
 *   pnpm playwright tests/07-i18n-length.spec.ts
 *
 * After the run, open the two PNGs side by side (e.g. `open
 * .design/molesignal-redesign/screenshots/m3/i18n/sidebar-en.png
 * .design/molesignal-redesign/screenshots/m3/i18n/sidebar-zh.png`) and
 * walk down the column looking for: overflowed text in the sidebar group
 * headings, wrapped pills in the alerts toolbar, truncated KPI labels,
 * and toolbar buttons that now wrap onto two lines.
 */
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { test } from '@playwright/test';

import type { Locale } from '../../src/i18n';
import { installMockSession } from '../fixtures/mockSession';

const __dirname = dirname(fileURLToPath(import.meta.url));

const PREFS_KEY = 'molesignal-ui-prefs';
const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';
const EXPLICIT_THEME_KEY = 'molesignal-theme-explicit';

const OUT_DIR = resolve(__dirname, '../../../.design/molesignal-redesign/screenshots/m3/i18n');

const LOCALES: Locale[] = ['en-us', 'zh-cn'];

// Pages most likely to surface the 1.6× length differential: dense
// toolbars, multi-column tables, KPI strips, sidebar group headings.
const PAGES: Array<{ slug: string; route: string; requiresAuth: boolean }> = [
  { slug: 'login', route: '/login', requiresAuth: false },
  { slug: 'home', route: '/home', requiresAuth: true },
  { slug: 'sidebar', route: '/home', requiresAuth: true }, // same route, captured for sidebar comparison
  { slug: 'logs', route: '/logs', requiresAuth: true },
  { slug: 'metrics', route: '/metrics', requiresAuth: true },
  { slug: 'traces', route: '/traces', requiresAuth: true },
  { slug: 'alerts', route: '/alerts', requiresAuth: true },
  { slug: 'streams', route: '/streams', requiresAuth: true },
  { slug: 'dashboards', route: '/dashboards', requiresAuth: true },
  { slug: 'iam-users', route: '/iam/users', requiresAuth: true },
  { slug: 'settings-general', route: '/settings/general', requiresAuth: true },
  { slug: 'datasource', route: '/datasource', requiresAuth: true },
  { slug: 'pipelines', route: '/pipelines', requiresAuth: true },
  { slug: 'apm-overview', route: '/apm/overview', requiresAuth: true },
  { slug: 'rum-overview', route: '/rum/overview', requiresAuth: true },
  { slug: 'rum-sessions', route: '/rum/sessions', requiresAuth: true },
];

for (const locale of LOCALES) {
  const localeTag = locale === 'zh-cn' ? 'zh' : 'en';
  test.describe(`i18n length @ ${locale}`, () => {
    test.beforeEach(async ({ page }) => {
      // Pin the clock so any "now"-relative chrome is identical between
      // en and zh runs — otherwise zh-side runs a few seconds later read
      // a fresher window summary, which looks like a layout diff.
      await page.clock.install({ time: new Date('2026-05-23T10:00:00.000Z') });
      await installMockSession(page);

      await page.addInitScript(
        ({
          prefsKey,
          prefs,
          themeKey,
          densityKey,
          explicitKey,
        }) => {
          // useThemeStore is persisted by zustand `persist` under
          // `molesignal-ui-prefs` as `{state, version}` — seeding this
          // before mount lets `ThemeBootstrap` pick up the language on
          // the very first paint and `i18n.changeLanguage()` fires before
          // any route renders user-visible copy in the wrong locale.
          localStorage.setItem(prefsKey, JSON.stringify(prefs));
          localStorage.setItem(themeKey, 'dark');
          localStorage.setItem(densityKey, 'normal');
          localStorage.setItem(explicitKey, '1');
        },
        {
          prefsKey: PREFS_KEY,
          prefs: { state: { palette: 'default', language: locale }, version: 0 },
          themeKey: THEME_KEY,
          densityKey: DENSITY_KEY,
          explicitKey: EXPLICIT_THEME_KEY,
        },
      );

      // Stub /api/v1/* calls so the screenshot captures the rendered shell
      // without depending on a live backend. Most list-style endpoints (alerts,
      // dashboards, rum, iam, ...) return a top-level array — defaulting to
      // `[]` keeps `data.map(...)` from throwing and turning the page into a
      // generic ErrorBoundary (which would look identical between en and zh
      // and defeat the purpose of the audit). The few endpoints that return
      // an object (`/streams`, `/topology`, ...) get a specific shape.
      // Playwright route handlers are last-registered-wins, so register the
      // catch-all FIRST and let specific endpoints override afterwards.
      await page.route('**/api/v1/**', (route) => route.fulfill({ json: [] }));
      await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: { items: [] } }));
      await page.route('**/api/v1/web/topology**', (route) =>
        route.fulfill({ json: { nodes: [], edges: [] } }),
      );
      await page.route('**/api/v1/web/search**', (route) =>
        route.fulfill({ json: { items: [] } }),
      );
      await page.route('**/api/v1/web/traces**', (route) =>
        route.fulfill({ json: { items: [] } }),
      );
      await page.route('**/api/v1/web/trace/**', (route) =>
        route.fulfill({ status: 404, body: 'not found' }),
      );
      // `/query` is the universal PromQL / SQL endpoint — returns QueryResult.
      await page.route('**/api/v1/query**', (route) =>
        route.fulfill({ json: { columns: [], rows: [], scanned_rows: 0, took_ms: 0 } }),
      );
      // `/metrics/catalog` returns `{ metrics: MetricInfo[] }`.
      await page.route('**/api/v1/metrics/catalog**', (route) =>
        route.fulfill({ json: { metrics: [] } }),
      );
    });

    for (const pageDef of PAGES) {
      test(`${pageDef.slug}`, async ({ page }) => {
        await page.goto(pageDef.route);
        // Let lazy-loaded chunks settle (Monaco, reactflow, uPlot are
        // all import()'d). Without this the screenshot can capture a
        // skeleton frame and the en / zh comparison becomes noise.
        await page.waitForLoadState('networkidle');
        // Wait for `ThemeBootstrap` to flush its language effect — without
        // this the screenshot can land between the first render (browser-
        // detected `en-us`) and the persisted-store hydration that flips
        // i18next to `zh-cn`, producing en-content under a zh filename.
        await page.waitForFunction(
          (expected) => document.documentElement.getAttribute('lang') === expected,
          locale,
          { timeout: 5_000 },
        );

        await page.screenshot({
          path: `${OUT_DIR}/${pageDef.slug}-${localeTag}.png`,
          fullPage: true,
          animations: 'disabled',
        });
      });
    }
  });
}
