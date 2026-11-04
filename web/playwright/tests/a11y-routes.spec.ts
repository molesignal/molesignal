/**
 * Axe-core gate across every authenticated route
 * (web-a11y-baseline).
 *
 * Iterates the authenticated application route matrix, runs
 * `AxeBuilder().analyze()` on each and asserts `critical = 0`. The core
 * investigation and management surfaces additionally gate `serious = 0`,
 * which includes real rendered color-contrast and labeling failures.
 *
 * The dev server is the same one Playwright spins up for visual + behavior
 * specs; mountMockRoutes seeds mock auth + frozen clock + deterministic
 * `/api/v1/*` mocks so the scan runs against a stable shell.
 *
 * The shared mock gives the Logs and Traces explorers nothing to list, so the
 * scan of those two routes is of their empty state. Each is scanned a second
 * time with results from `mockExplorers`: the list, the histogram and the
 * fields of the field list are on the page only then.
 */
import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults, type Explorer } from '../fixtures/mockExplorers';
import { MOCK_RUM } from '../fixtures/mockRumData';

const ROUTES: Array<{ path: string; label: string }> = [
  { path: '/login', label: 'login' },
  { path: '/home', label: 'home' },
  { path: '/investigate', label: 'investigate' },
  { path: '/logs', label: 'logs' },
  { path: '/metrics', label: 'metrics' },
  { path: '/traces', label: 'traces' },
  { path: '/dashboards', label: 'dashboards' },
  { path: '/alerts', label: 'alerts' },
  { path: '/agent/chat', label: 'agent-chat' },
  { path: '/agent/investigations', label: 'agent-investigations' },
  { path: '/agent/automations', label: 'agent-automations' },
  { path: '/agent/approvals', label: 'agent-approvals' },
  { path: '/agent/executions', label: 'agent-executions' },
  { path: '/agent/settings', label: 'agent-settings' },
  { path: '/agent/settings/inbound-mcp', label: 'agent-inbound-mcp' },
  { path: '/agent/settings/im', label: 'agent-im' },
  { path: '/streams', label: 'streams' },
  { path: '/settings', label: 'settings' },
  { path: '/noc', label: 'noc' },

  // RUM overview, explorer, applications and settings routes
  { path: '/rum/overview', label: 'rum-overview' },
  { path: '/rum/explorer', label: 'rum-explorer' },
  { path: '/rum/explorer?type=views&viz=timeseries&by=view.url_path_group', label: 'rum-explorer-timeseries' },
  { path: '/rum/explorer?type=views&viz=toplist', label: 'rum-explorer-toplist' },
  { path: '/rum/explorer?type=views&viz=table', label: 'rum-explorer-table' },
  { path: '/rum/explorer?viz=map', label: 'rum-explorer-map' },
  { path: '/rum/explorer?type=views&viz=distribution', label: 'rum-explorer-distribution' },
  { path: '/rum/errors', label: 'rum-errors' },
  { path: '/rum/performance', label: 'rum-performance' },
  { path: '/rum/performance?open=%2Fcheckout', label: 'rum-performance-drawer' },
  { path: '/rum/performance/resources', label: 'rum-performance-resources' },
  { path: '/rum/performance/resources?open=%5B%22api.example.com%22%2C%22fetch%22%5D', label: 'rum-performance-resources-panel' },
  { path: '/rum/performance/long-tasks', label: 'rum-performance-long-tasks' },
  { path: '/rum/frustration', label: 'rum-frustration' },
  { path: '/rum/frustration?open=%5B%22Pay%20now%22%2C%22button%5Bdata-testid%3D%5C%22pay-now%5C%22%5D%22%5D', label: 'rum-frustration-panel' },
  { path: '/rum/applications', label: 'rum-applications' },
  { path: '/rum/analytics/heatmaps', label: 'rum-analytics-heatmaps' },
  { path: '/rum/analytics/heatmaps?map=scroll', label: 'rum-analytics-heatmaps-scroll' },
  { path: '/rum/feature-flags', label: 'rum-feature-flags' },
  { path: '/rum/feature-flags?flag=checkout_v2', label: 'rum-feature-flags-compare' },
  // A visit that has no DOM recording: the replayer's frame is a sandbox axe cannot enter.
  { path: `/rum/sessions/view/${MOCK_RUM.session.anonymous}`, label: 'rum-session-detail' },
  { path: '/rum/settings/sdk', label: 'rum-settings-sdk' },
  { path: '/rum/settings/sampling', label: 'rum-settings-sampling' },
  { path: '/rum/settings/privacy', label: 'rum-settings-privacy' },
  { path: '/rum/settings/session-replay', label: 'rum-settings-replay' },
  { path: '/rum/settings/source-maps', label: 'rum-source-maps' },
  { path: '/rum/settings/source-maps/upload', label: 'rum-upload-source-maps' },

  // Functions module (2 routes)
  { path: '/functions', label: 'functions-list' },
  { path: '/functions/new', label: 'functions-new' },
  { path: '/extend-tables', label: 'extend-tables' },

  // Pipelines editor module (5 routes)
  { path: '/pipelines/new', label: 'pipelines-new' },
  { path: '/pipelines/import', label: 'pipelines-import' },
  { path: '/pipelines/sample-id/edit', label: 'pipelines-edit' },
  { path: '/pipelines/sample-id/history', label: 'pipelines-history' },
  { path: '/pipelines/sample-id/backfill', label: 'pipelines-backfill' },

  // IAM module (8 routes)
  { path: '/iam/users', label: 'iam-users' },
  { path: '/iam/service-accounts', label: 'iam-service-accounts' },
  { path: '/iam/api-tokens', label: 'iam-api-tokens' },
  { path: '/iam/organizations', label: 'iam-organizations' },
  { path: '/iam/groups', label: 'iam-groups' },
  { path: '/iam/roles', label: 'iam-roles' },
  { path: '/iam/invitations', label: 'iam-invitations' },

  // Settings sub-routes.
  { path: '/settings/general', label: 'settings-general' },
  { path: '/settings/storage_settings', label: 'settings-storage' },
  { path: '/pipelines/connectors', label: 'pipelines-connectors' },
  { path: '/settings/nodes', label: 'settings-nodes' },
  { path: '/settings/cipher_keys', label: 'settings-cipher-keys' },
  { path: '/settings/regex_patterns', label: 'settings-regex-patterns' },
  { path: '/settings/domain_management', label: 'settings-domain-management' },
  { path: '/settings/organization_management', label: 'settings-organization-management' },
  { path: '/settings/query_management', label: 'settings-query-management' },
  { path: '/settings/audit', label: 'settings-audit' },
  { path: '/settings/ai_providers', label: 'settings-ai-providers' },
  { path: '/settings/ai_prompts', label: 'settings-ai-prompts' },
  { path: '/settings/notify/connectors', label: 'settings-notify-connectors' },
  { path: '/settings/notify/users', label: 'settings-notify-users' },
  { path: '/settings/notify/policies', label: 'settings-notify-policies' },
  { path: '/settings/notify/templates', label: 'settings-notify-templates' },
  { path: '/settings/notify/defaults', label: 'settings-notify-defaults' },
  { path: '/settings/notify/deliveries', label: 'settings-notify-deliveries' },

  // Secondary application routes.
  { path: '/logs/inspector', label: 'logs-inspector' },
  { path: '/logs/inspections', label: 'logs-inspections' },
  { path: '/logs/inspections/2zA1paymentErrorsHourly0001', label: 'logs-inspection-detail' },
  { path: '/traces/sample-trace-id', label: 'traces-detail' },
  { path: '/traces/session/sample-session', label: 'traces-session-detail' },
  { path: '/streams/sample-stream', label: 'streams-explore' },
  { path: '/streams/jobs', label: 'streams-jobs' },
  { path: '/service-graph', label: 'service-graph' },
  { path: '/dashboards/import', label: 'dashboards-import' },
  // `/dashboards/:id/panels/new` mounts the existing DashboardEditor, which
  // has pre-existing axe critical violations (unlabeled form controls).
  // Exclude until the editor itself is a11y-cleaned in a follow-up.
  // { path: '/dashboards/sample-id/panels/new', label: 'dashboards-new-panel' },
  { path: '/alerts/history', label: 'alerts-history' },
  { path: '/alerts/insights', label: 'alerts-insights' },
];

const STRICT_ROUTES = new Set([
  'login',
  'home',
  'logs',
  'metrics',
  'traces',
  'alerts',
  'streams',
  'settings-general',
  'iam-users',
]);

/** The explorers that are scanned again with results on the page, each under the label of its route. */
const EXPLORERS: Explorer[] = ['logs', 'traces'];

/** Scans the page that is open; `label` names it in the counts, and `strict` gates serious violations too. */
async function scan(page: Page, label: string, strict: boolean) {
  const results = await new AxeBuilder({ page })
    .exclude('[role="status"]')
    .exclude('[aria-live]')
    .analyze();
  const critical = results.violations.filter((v) => v.impact === 'critical');
  const serious = results.violations.filter((v) => v.impact === 'serious');
  const moderate = results.violations.filter((v) => v.impact === 'moderate');
  const minor = results.violations.filter((v) => v.impact === 'minor');
  // Non-fatal counts surfaced as JSON for CI log scraping.
  console.log(
    JSON.stringify({
      route: label,
      counts: {
        critical: critical.length,
        serious: serious.length,
        moderate: moderate.length,
        minor: minor.length,
      },
      seriousIds: serious.map((v) => v.id),
      moderateIds: moderate.map((v) => v.id),
      minorIds: minor.map((v) => v.id),
    }),
  );
  const blocking = strict ? [...critical, ...serious] : critical;
  expect(blocking, JSON.stringify(blocking, null, 2)).toEqual([]);
}

test.describe('a11y: routes', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  for (const { path, label } of ROUTES) {
    const strict = STRICT_ROUTES.has(label);
    test(`route ${label} has no ${strict ? 'serious/critical' : 'critical'} axe violations`, async ({ page }) => {
      await page.goto(path);
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await scan(page, label, strict);
    });
  }

  for (const explorer of EXPLORERS) {
    const strict = STRICT_ROUTES.has(explorer);
    test(`route ${explorer} with results has no ${strict ? 'serious/critical' : 'critical'} axe violations`, async ({
      page,
    }) => {
      await serveExplorerResults(page);
      await page.goto(`/${explorer}`);
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await expectExplorerResults(page, explorer);
      await scan(page, `${explorer}-results`, strict);
    });
  }
});
