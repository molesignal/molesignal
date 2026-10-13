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

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
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
  platformPermissions: ['sys.settings.manage', 'sys.organizations.manage', 'sys.telemetry.read'],
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
        header_name: 'X-Forwarded-For',
        trusted_proxy_cidrs: ['10.0.0.0/8'],
        fallback_to_peer: true,
        allow_private_client_ips: false,
        max_chain_length: 5,
      },
    },
  },
  { path: '/settings/organization_management', mount: PLATFORM },
  {
    path: '/settings/nodes',
    serve: {
      '/clusters': [
        {
          id: 'c1',
          name: 'eu-west',
          advertise_addr: 'https://eu.example.com:5080',
          token_secret_ref: 'secret/eu',
          tls_verify: true,
          enabled: true,
          discovered: false,
          created_at_micros: micros(NOW - 30 * DAY),
          updated_at_micros: micros(NOW - DAY),
        },
        {
          id: 'c2',
          name: 'ap-south',
          advertise_addr: 'https://ap.example.com:5080',
          token_secret_ref: 'secret/ap',
          tls_verify: false,
          enabled: false,
          discovered: true,
          created_at_micros: micros(NOW - 10 * DAY),
          updated_at_micros: micros(NOW - 2 * DAY),
        },
      ],
      '/settings/service_graph': { source: 'intake' },
      '/settings/federation': {
        cluster_id: 'local',
        drain_interval_secs: 5,
        push_batch_size: 100,
        seen_events_ttl_secs: 3600,
        gossip_interval_secs: 30,
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
          created_at_micros: micros(NOW - 30 * DAY),
          rotated_at_micros: micros(NOW - DAY),
        },
        { id: 'ck-2', name: 'billing-key', alg: 'AES-256-GCM', version: 1, created_at_micros: micros(NOW - 3 * DAY) },
      ],
    },
  },
  {
    path: '/settings/field_masking',
    serve: {
      '/field_masking/rules': [
        {
          id: 'r1',
          org_id: 'org',
          name: 'Card number',
          priority: 1,
          enabled: true,
          field_pattern: 'card_*',
          stream_pattern: null,
          stream_type: null,
          algorithm: { kind: 'inner', prefix_chars: 4, suffix_chars: 4, replacement: '*' },
          created_at: micros(NOW - 30 * DAY),
          updated_at: micros(NOW - DAY),
        },
        {
          id: 'r2',
          org_id: 'org',
          name: 'Email',
          priority: 2,
          enabled: false,
          field_pattern: 'email',
          stream_pattern: 'app_*',
          stream_type: 'logs',
          algorithm: { kind: 'hash' },
          created_at: micros(NOW - 20 * DAY),
          updated_at: micros(NOW - 2 * DAY),
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
          cert_not_after_micros: micros(NOW + 60 * DAY),
          created_at_micros: micros(NOW - 30 * DAY),
          updated_at_micros: micros(NOW - DAY),
        },
        {
          id: 'd2',
          hostname: 'logs.example.com',
          state: 'pending',
          created_at_micros: micros(NOW - DAY),
          updated_at_micros: micros(NOW - DAY),
        },
        {
          id: 'd3',
          hostname: 'old.example.com',
          state: 'failed',
          last_error: 'DNS validation failed',
          created_at_micros: micros(NOW - 9 * DAY),
          updated_at_micros: micros(NOW - 2 * DAY),
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
          org_id: 'org',
          user_id: 'dev@example.com',
          started_at_micros: micros(NOW - 30_000),
          statement: 'SELECT level, count(*) FROM application_logs GROUP BY level',
          scanned_rows: 120_000,
        },
        {
          id: 'q-2',
          org_id: 'org',
          user_id: 'ops@example.com',
          started_at_micros: micros(NOW - 90_000),
          statement: 'SELECT * FROM traces WHERE duration_ns > 1000000',
          scanned_rows: 9_800_000,
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
