/**
 * Colour contrast on the light theme.
 *
 * `a11y-routes` scans each route in the mock user's default theme, which is
 * dark. A status tone that reads on dark can fall short of AA on light, where
 * the soft tones are fills and a word in a tone has to be set in ink
 * (`scripts/check-no-soft-text.ts`). So the pages an operator works in and
 * every settings page are scanned again here on light, for contrast alone.
 *
 * The Logs and Traces explorers are scanned twice: as the shared mock leaves
 * them, with nothing to list, and with results from `mockExplorers`, where the
 * level of a log line, the error count of a trace and the type of a field are
 * words in a tone.
 */
import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import {
  expect,
  FROZEN_NOW_ISO,
  MOCK_IAM_PLATFORM_PERMISSIONS,
  mountMockRoutes,
  test,
} from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults, type Explorer } from '../fixtures/mockExplorers';

type Mount = NonNullable<Parameters<typeof mountMockRoutes>[2]>;

interface Scanned {
  path: string;
  /** What the page lists, for an endpoint the shared mock does not model: a page handed `{}` for a list does not render. */
  serve?: Record<string, unknown>;
  /** The explorer the page is, where it is scanned with results on it: the shared mock alone gives it nothing to list. */
  results?: Explorer;
  mount?: Mount;
}

const NOW = Date.parse(FROZEN_NOW_ISO);
const DAY = 86_400_000;
const micros = (ms: number) => ms * 1000;

/** Platform settings are reached from the platform organization. */
const PLATFORM: Mount = {
  scope: 'system',
  platformPermissions: [...MOCK_IAM_PLATFORM_PERMISSIONS],
};

const PAGES: Scanned[] = [
  { path: '/home' },
  { path: '/logs' },
  { path: '/logs', results: 'logs' },
  { path: '/traces' },
  { path: '/traces', results: 'traces' },
  { path: '/metrics' },
  { path: '/alerts' },
  { path: '/dashboards' },
  { path: '/synthetics/overview' },
  { path: '/streams' },

  { path: '/settings/general' },
  { path: '/settings/notify/connectors' },
  { path: '/settings/notify/users' },
  { path: '/settings/notify/policies' },
  { path: '/settings/notify/templates' },
  { path: '/settings/notify/defaults' },
  { path: '/settings/notify/deliveries' },
  { path: '/settings/regex_patterns' },
  { path: '/settings/audit' },
  {
    path: '/settings/client_ip',
    mount: PLATFORM,
    serve: {
      '/settings/client_ip': {
        mode: 'header',
        headerName: 'X-Forwarded-For',
        trustedProxyCidrs: ['10.0.0.0/8'],
        fallbackToPeer: true,
        allowPrivateClientIps: false,
        maxChainLength: 5,
      },
    },
  },
  { path: '/settings/organization_management', mount: PLATFORM },
  { path: '/settings/signin_limits', mount: PLATFORM },
  {
    path: '/settings/nodes',
    serve: {
      '/clusters': [
        {
          id: 'c1',
          name: 'eu-west',
          advertiseAddr: 'https://eu.example.com:5080',
          tokenSecretRef: 'secret/eu',
          tlsVerify: true,
          enabled: true,
          discovered: false,
          createdAt: micros(NOW - 30 * DAY),
          updatedAt: micros(NOW - DAY),
        },
        {
          id: 'c2',
          name: 'ap-south',
          advertiseAddr: 'https://ap.example.com:5080',
          tokenSecretRef: 'secret/ap',
          tlsVerify: false,
          enabled: false,
          discovered: true,
          createdAt: micros(NOW - 10 * DAY),
          updatedAt: micros(NOW - 2 * DAY),
        },
      ],
      '/settings/service_graph': { source: 'intake' },
      '/settings/federation': {
        clusterId: 'local',
        drainIntervalSecs: 5,
        pushBatchSize: 100,
        seenEventsTtlSecs: 3600,
        gossipIntervalSecs: 30,
      },
    },
  },
  {
    path: '/settings/cipher_keys',
    serve: {
      '/cipher_keys': [
        {
          id: 'ck-1',
          name: 'pii-key',
          alg: 'AES-256-GCM',
          version: 3,
          createdAt: micros(NOW - 30 * DAY),
          rotatedAt: micros(NOW - DAY),
        },
        { id: 'ck-2', name: 'billing-key', alg: 'AES-256-GCM', version: 1, createdAt: micros(NOW - 3 * DAY) },
      ],
    },
  },
  {
    path: '/settings/field_masking',
    serve: {
      '/field_masking/rules': [
        {
          id: 'r1',
          orgId: 'org',
          name: 'Card number',
          priority: 1,
          enabled: true,
          fieldPattern: 'card_*',
          streamPattern: null,
          streamType: null,
          algorithm: { kind: 'inner', prefixChars: 4, suffixChars: 4, replacement: '*' },
          createdAt: micros(NOW - 30 * DAY),
          updatedAt: micros(NOW - DAY),
        },
        {
          id: 'r2',
          orgId: 'org',
          name: 'Email',
          priority: 2,
          enabled: false,
          fieldPattern: 'email',
          streamPattern: 'app_*',
          streamType: 'logs',
          algorithm: { kind: 'hash' },
          createdAt: micros(NOW - 20 * DAY),
          updatedAt: micros(NOW - 2 * DAY),
        },
      ],
    },
  },
  {
    path: '/settings/domain_management',
    serve: {
      '/domains': [
        {
          id: 'd1',
          hostname: 'status.example.com',
          state: 'active',
          certNotAfter: micros(NOW + 60 * DAY),
          createdAt: micros(NOW - 30 * DAY),
          updatedAt: micros(NOW - DAY),
        },
        {
          id: 'd2',
          hostname: 'logs.example.com',
          state: 'pending',
          createdAt: micros(NOW - DAY),
          updatedAt: micros(NOW - DAY),
        },
        {
          id: 'd3',
          hostname: 'old.example.com',
          state: 'failed',
          lastError: 'DNS validation failed',
          createdAt: micros(NOW - 9 * DAY),
          updatedAt: micros(NOW - 2 * DAY),
        },
      ],
    },
  },
  {
    path: '/settings/query_management',
    serve: {
      '/query/running': [
        {
          id: 'q-1',
          orgId: 'org',
          userId: 'dev@example.com',
          startedAt: micros(NOW - 30_000),
          statement: 'SELECT level, count(*) FROM application_logs GROUP BY level',
          scannedRows: 120_000,
        },
        {
          id: 'q-2',
          orgId: 'org',
          userId: 'ops@example.com',
          startedAt: micros(NOW - 90_000),
          statement: 'SELECT * FROM traces WHERE duration_ns > 1000000',
          scannedRows: 9_800_000,
          cancelled: true,
        },
      ],
    },
  },
];

/**
 * A row of a table is filled with bg-3 while the pointer is on it, the darkest fill a row
 * takes, and the state of a check or of a run is a word in its status color: each of these is
 * read in a row that says it.
 */
const HOVERED: Array<{ path: string; states: string[] }> = [
  { path: '/synthetics/checks', states: ['Passing', 'Degraded', 'Flaky'] },
  { path: '/synthetics/results', states: ['Passed', 'Degraded', 'Flaky'] },
];

async function serveLists(page: Page, lists: Record<string, unknown>) {
  for (const [endpoint, body] of Object.entries(lists)) {
    await page.route(`**/api/v1${endpoint}`, (route) =>
      route.request().method() === 'GET' ? route.fulfill({ json: body }) : route.fallback(),
    );
  }
}

async function openOnLight(page: Page, port: number, { path, serve, results, mount }: Scanned) {
  // An entrance that is still fading in would be measured at the opacity it has reached.
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await mountMockRoutes(page, port, { theme: 'light', ...mount });
  if (serve) await serveLists(page, serve);
  if (results) await serveExplorerResults(page);
  await page.goto(path);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light', { timeout: 15_000 });
  await page.waitForLoadState('networkidle').catch(() => undefined);

  // The page rendered: a list the mock has no answer for would leave the router's error screen.
  await expect(page.getByText('Unexpected Application Error')).toHaveCount(0);
  if (results) await expectExplorerResults(page, results);
}

async function contrastViolations(page: Page): Promise<string[]> {
  const results = await new AxeBuilder({ page }).withRules(['color-contrast']).analyze();
  return results.violations.flatMap((violation) =>
    violation.nodes.map((node) => {
      const data = (node.any[0]?.data ?? {}) as { fgColor?: string; bgColor?: string; contrastRatio?: number };
      return `${data.contrastRatio}:1 ${data.fgColor} on ${data.bgColor} — ${node.target.join(' ')}`;
    }),
  );
}

/**
 * The fill of a hovered row, as the page computes it: what `bg-bg-3` paints, the utility a row
 * takes under the pointer.
 */
async function hoveredRowFill(page: Page): Promise<string> {
  return page.evaluate(() => {
    const probe = document.body.appendChild(document.createElement('i'));
    probe.className = 'bg-bg-3';
    const fill = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return fill;
  });
}

test.describe('a11y: contrast on the light theme', () => {
  for (const scanned of PAGES) {
    const state = scanned.results ? ' with results' : '';
    test(`${scanned.path}${state} has no contrast violation`, async ({ page, mockServer }) => {
      await openOnLight(page, mockServer.port, scanned);

      expect(await contrastViolations(page)).toEqual([]);
    });
  }

  for (const { path, states } of HOVERED) {
    test(`${path} has no contrast violation on a hovered row`, async ({ page, mockServer }) => {
      await openOnLight(page, mockServer.port, { path });
      const fill = await hoveredRowFill(page);

      for (const state of states) {
        const row = page.locator('tbody tr').filter({ hasText: state }).first();
        await row.hover();
        await expect(row).toHaveCSS('background-color', fill);

        expect(await contrastViolations(page), `a row that says ${state}`).toEqual([]);
      }
    });
  }
});
