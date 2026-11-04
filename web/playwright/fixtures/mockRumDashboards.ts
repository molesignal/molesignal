/**
 * What the server answers about the built-in RUM dashboards: the dashboard of an organization, with
 * the three variables the real one has and two of its panels, the values the variables can take, and
 * a number for every statement a panel runs. The statements the page sent are kept for the test to
 * read. What the real panels say, and that they run, is checked in Rust against the real engine, not
 * here.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO } from './mockBackend';

const ORGANIZATION = 'acme-prod';
const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

export interface RumDashboardsCaptured {
  /** The statements the panels ran. */
  statements: string[];
  /** The statements that asked for the values of a variable. */
  variables: string[];
  /** What was sent to create a dashboard: the copies a person made. */
  created: Array<{ model: Record<string, unknown>; folder_id?: string }>;
}

function panel(id: string, title: string, statement: string, type: 'stat' | 'table') {
  return {
    kind: 'panel',
    id,
    title,
    description: '',
    gridPos: { x: id === 'sessions' ? 0 : 6, y: 0, w: 6, h: 12 },
    queryOptions: {},
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'sql',
        query: { language: 'sql', streamName: 'rum_sessions', streamType: 'logs', statement, limit: 100 },
      },
    ],
    transformations: [],
    visualization: { type, schemaVersion: 1, options: {} },
    fieldConfig: {},
    overrides: [],
    links: [],
  };
}

function variables(platform: string) {
  const source = (expression: string) => ({ kind: 'sql', streamName: 'rum_sessions', streamType: 'logs', expression });

  return [
    {
      id: 'variable-application',
      name: 'application',
      label: '应用',
      type: 'query',
      query: source(`SELECT DISTINCT "application.id" FROM rum_sessions WHERE source = '${platform}' ORDER BY 1`),
      multi: false,
      includeAll: false,
      hide: 'none',
      refresh: 'time_range_change',
    },
    {
      id: 'variable-environment',
      name: 'environment',
      label: '环境',
      type: 'query',
      query: source(
        `SELECT DISTINCT env FROM rum_sessions WHERE "application.id" = \${application:sqlstring} AND env <> '' UNION ALL SELECT '全部'`,
      ),
      options: [{ label: '全部', value: '全部', selected: true }],
      defaultValue: '全部',
      currentValue: '全部',
      multi: false,
      includeAll: false,
      hide: 'none',
      refresh: 'time_range_change',
      dependsOn: ['application'],
    },
    {
      id: 'variable-interval',
      name: 'interval',
      label: '间隔',
      type: 'interval',
      options: [
        { label: '5 分钟', value: '5 minutes' },
        { label: '15 分钟', value: '15 minutes', selected: true },
        { label: '1 小时', value: '1 hour' },
      ],
      defaultValue: '15 minutes',
      currentValue: '15 minutes',
      multi: false,
      includeAll: false,
      hide: 'none',
      refresh: 'never',
    },
  ];
}

function dashboard(id: string) {
  const uid = id.startsWith('rum-browser') ? 'rum-browser' : 'rum-mobile';
  const scope = `"application.id" = \${application:sqlstring} AND (\${environment:sqlstring} = '全部' OR env = \${environment:sqlstring})`;

  return {
    id,
    orgId: ORGANIZATION,
    uid,
    title: uid === 'rum-browser' ? 'RUM · 浏览器应用' : 'RUM · 移动应用',
    tags: ['内置', 'rum'],
    version: 1,
    createdAt: NOW,
    updatedAt: NOW,
    createdBy: 'molesignal-system',
    updatedBy: 'molesignal-system',
    model: {
      engine: 'molesignal-dashboard',
      schemaVersion: 2,
      id,
      uid,
      title: uid === 'rum-browser' ? 'RUM · 浏览器应用' : 'RUM · 移动应用',
      description: '',
      tags: ['内置', 'rum'],
      editable: false,
      defaultDashboard: false,
      timeSettings: { defaultFrom: 'now-24h', defaultTo: 'now', timezone: 'browser' },
      refreshSettings: { enabled: false, mode: 'off', allowedIntervals: ['off'] },
      variables: variables(uid === 'rum-browser' ? 'browser' : 'ios'),
      annotations: [],
      links: [],
      layout: { type: 'grid', columns: 24, rowHeight: 8, gap: 8 },
      elements: [
        panel('sessions', '会话', `SELECT count(*) AS "会话" FROM rum_sessions_latest WHERE ${scope}`, 'stat'),
        panel(
          'versions',
          '版本',
          `SELECT version AS "版本", count(*) AS "会话" FROM rum_sessions_latest WHERE ${scope} GROUP BY 1`,
          'table',
        ),
      ],
      version: 1,
      createdAt: FROZEN_NOW_ISO,
      updatedAt: FROZEN_NOW_ISO,
      createdBy: 'molesignal-system',
      updatedBy: 'molesignal-system',
      extensions: { builtin: { key: uid, version: 1 } },
    },
  };
}

/** The values a variable can take: every application, and the environments of the one chosen. */
const VALUES: Record<string, string[]> = {
  application: ['storefront', 'storefront-app'],
  environment: ['prod', 'staging', '全部'],
};

export async function installRumDashboards(page: Page): Promise<RumDashboardsCaptured> {
  const captured: RumDashboardsCaptured = { statements: [], variables: [], created: [] };

  await page.route(/\/api\/v1\/dashboards\/rum-(browser|mobile)-[^/?]+$/, (route) => {
    const id = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');

    return route.request().method() === 'GET' ? route.fulfill({ json: dashboard(id) }) : route.fallback();
  });

  // A copy is made by the mock server; what was sent is kept.
  await page.route(/\/api\/v1\/dashboards$/, (route) => {
    if (route.request().method() === 'POST') captured.created.push(route.request().postDataJSON());

    return route.fallback();
  });

  await page.route(/\/api\/v1\/dashboards\/variables\/resolve$/, (route) => {
    const body = route.request().postDataJSON() as { variable: { name: string; query: string } };
    const values = VALUES[body.variable.name] ?? [];

    captured.variables.push(body.variable.query);

    return route.fulfill({ json: { variable: body.variable.name, values, default: values[0] ?? null } });
  });

  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) => {
    if (route.request().method() !== 'POST') return route.fallback();

    const body = route.request().postDataJSON() as { statement?: string };

    captured.statements.push(body.statement ?? '');

    return route.fulfill({
      json: { columns: ['会话'], rows: [[18_420]], scannedRows: 0, tookMs: 1 },
    });
  });

  return captured;
}
