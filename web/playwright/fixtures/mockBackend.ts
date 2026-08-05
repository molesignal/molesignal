/**
 * Deterministic mock backend for Playwright e2e + perf suites
 * (web-playwright-runtime).
 *
 * Strategy
 * --------
 * 1. An in-test Express server mounted on a dynamic port serves canned JSON
 *    for every `/api/v1/*` endpoint the app may hit (12+ endpoints; see
 *    `registerRoutes` below). Payloads live as JSON files under `./data/` so
 *    spec files stay readable.
 * 2. Each spec calls `await mountMockRoutes(page, mockServer.port)` in its
 *    `beforeEach` to install `page.route('**\/api/v1/**', ...)` interceptors
 *    that proxy through to the local Express port. No request escapes to the
 *    public network.
 * 3. The clock is frozen at `2026-05-23T10:00:00Z` via `page.clock.install()`
 *    inside `mountMockRoutes` so `formatWindowSummary`, NDJSON timestamps and
 *    the visual baselines all stay deterministic.
 */
import { readFileSync } from 'node:fs';
import type { Server } from 'node:http';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { test as base, type Page } from '@playwright/test';
import express, { type Express, type Request, type Response } from 'express';

import { PRODUCT_ROUTES } from '../../src/product/ia';

export const FROZEN_NOW_ISO = '2026-05-23T10:00:00.000Z';

const MOCK_IAM_PLATFORM_PERMISSIONS = [
  'sys.organizations.manage',
  'sys.licenses.read',
  'sys.licenses.manage',
  'sys.settings.manage',
  'sys.dashboards.read',
  'sys.telemetry.read',
  'sys.telemetry.manage',
  'sys.administrators.manage',
  'sys.trace_debug.manage',
] as const;

const MOCK_IAM_ORGANIZATION_PERMISSIONS = [
  'org.settings.read',
  'org.settings.manage',
  'org.members.read',
  'org.members.manage',
  'iam.roles.read',
  'iam.roles.manage',
  'iam.policies.read',
  'iam.policies.manage',
  'org.billing.read',
  'org.billing.manage',
  'api_tokens.read',
  'api_tokens.manage',
  'streams.read',
  'streams.query',
  'streams.write',
  'streams.create',
  'streams.configure',
  'streams.delete',
  'dashboards.read',
  'dashboards.edit',
  'dashboards.create',
  'dashboards.delete',
  'dashboards.share',
  'alerts.read',
  'alerts.manage',
  'alerts.acknowledge',
  'alerts.silence',
  'schedules.read',
  'schedules.manage',
  'saved_views.read',
  'saved_views.create',
  'saved_views.edit',
  'saved_views.delete',
  'pipelines.read',
  'pipelines.create',
  'pipelines.edit',
  'pipelines.run',
  'pipelines.pause',
  'pipelines.delete',
  'functions.read',
  'functions.create',
  'functions.edit',
  'functions.run',
  'functions.delete',
  'reports.read',
  'reports.create',
  'reports.edit',
  'reports.schedule',
  'reports.share',
  'reports.delete',
  'audit.read',
  'intelligence.use',
  'intelligence.manage',
  'intelligence.approve',
] as const;

function mockCapabilityRoutes(
  scope: 'organization' | 'system' | 'api_token',
  permissions: readonly string[],
) {
  const permissionSet = new Set(permissions);
  const systemOnly = new Set([
    'iam.organizations',
    'settings.organization.management',
    'settings.license',
    'settings.client_ip',
  ]);
  const systemTelemetryOwners = new Set([
    'home',
    'logs',
    'metrics',
    'traces',
    'apm',
    'profiles',
    'streams',
    'legacy',
  ]);
  const decisions = PRODUCT_ROUTES.map((route, index) => {
    const navigable = 'nav' in route && route.nav === true;
    let allowed = scope !== 'system' && !systemOnly.has(route.id);
    if (scope === 'system') {
      allowed =
        route.owner === 'account' ||
        (systemTelemetryOwners.has(route.owner) &&
          permissionSet.has('sys.telemetry.read')) ||
        (route.owner === 'dashboards' &&
          permissionSet.has('sys.dashboards.read')) ||
        (route.id === 'iam.organizations' &&
          permissionSet.has('sys.organizations.manage')) ||
        (route.id === 'settings.organization.management' &&
          permissionSet.has('sys.organizations.manage')) ||
        (route.id === 'settings.license' &&
          permissionSet.has('sys.licenses.read')) ||
        (route.id === 'settings.client_ip' &&
          permissionSet.has('sys.settings.manage'));
    }
    return {
      id: route.id,
      path_pattern: route.path,
      allowed,
      navigation_group: navigable ? route.group : undefined,
      navigation_position: navigable ? index : undefined,
    };
  });
  const explicit = [
    ['root', '/', true],
    ['account.settings', '/account/settings/*', true],
    ['iam', '/iam', scope === 'system'
      ? permissionSet.has('sys.organizations.manage')
      : permissionSet.has('org.members.read') || permissionSet.has('iam.roles.read')],
    ['iam.users', '/iam/users', scope !== 'system' && permissionSet.has('org.members.read')],
    ['iam.roles', '/iam/roles', scope !== 'system' && permissionSet.has('iam.roles.read')],
    ['settings.notify', '/settings/notify/*', scope !== 'system' && permissionSet.has('alerts.read')],
    ['settings.tenant.tools', '/settings/:section', scope !== 'system' && permissionSet.has('org.settings.read')],
  ] as const;
  return [
    ...decisions,
    ...explicit.map(([id, path_pattern, allowed]) => ({
      id,
      path_pattern,
      allowed,
    })),
  ];
}

const MOCK_IAM_VIEWER_PERMISSIONS = [
  'streams.read',
  'streams.query',
  'dashboards.read',
  'alerts.read',
  'schedules.read',
  'saved_views.read',
  'pipelines.read',
  'functions.read',
  'reports.read',
  'intelligence.use',
] as const;

const MOCK_IAM_EDITOR_DENY_PREFIXES = [
  'org.',
  'iam.',
  'api_tokens.',
  'audit.',
  'intelligence.manage',
  'intelligence.approve',
] as const;

const MOCK_IAM_ROLE_PERMISSIONS: Record<string, readonly string[]> = {
  platform_owner: MOCK_IAM_PLATFORM_PERMISSIONS,
  owner: MOCK_IAM_ORGANIZATION_PERMISSIONS,
  admin: MOCK_IAM_ORGANIZATION_PERMISSIONS,
  editor: MOCK_IAM_ORGANIZATION_PERMISSIONS.filter(
    (permission) =>
      !MOCK_IAM_EDITOR_DENY_PREFIXES.some((prefix) =>
        permission.startsWith(prefix),
      ),
  ),
  viewer: MOCK_IAM_VIEWER_PERMISSIONS,
};

const mockIamDomain = (permission: string): string => {
  const resource = permission.split('.')[0];
  if (resource === 'sys') return 'platform';
  if (resource === 'org') return 'organization';
  if (resource === 'iam' || resource === 'api_tokens' || resource === 'audit') {
    return 'iam';
  }
  if (resource === 'dashboards') return 'dashboards';
  if (resource === 'alerts' || resource === 'schedules') return 'alerts';
  if (resource === 'pipelines' || resource === 'functions') return 'pipelines';
  if (resource === 'reports') return 'reports';
  if (resource === 'intelligence') return 'intelligence';
  return 'observability';
};

const MOCK_IAM_PERMISSION_CATALOG = {
  version: 6,
  permissions: [
    ...MOCK_IAM_PLATFORM_PERMISSIONS,
    ...MOCK_IAM_ORGANIZATION_PERMISSIONS,
  ].map((permission) => {
    const translationKey = permission.replaceAll('.', '_');
    return {
      key: permission,
      scope: permission.startsWith('sys.') ? 'platform' : 'organization',
      domain: mockIamDomain(permission),
      label_key: `permissions.${translationKey}`,
      description_key: `permissions_hint.${translationKey}`,
      builtin_roles: Object.entries(MOCK_IAM_ROLE_PERMISSIONS)
        .filter(([, permissions]) => permissions.includes(permission))
        .map(([role]) => role),
      ...(permission.startsWith('intelligence.')
        ? { feature: 'intelligence' }
        : {}),
    };
  }),
  bundles: [
    {
      key: 'readonly_observer',
      label_key: 'roles.bundles.readonly_observer',
      description_key: 'roles.bundles_hint.readonly_observer',
      permissions: MOCK_IAM_VIEWER_PERMISSIONS,
    },
  ],
};

const HERE = dirname(fileURLToPath(import.meta.url));
const DATA = join(HERE, 'data');

const readJson = <T = unknown>(name: string): T =>
  JSON.parse(readFileSync(join(DATA, name), 'utf8')) as T;
const readText = (name: string): string => readFileSync(join(DATA, name), 'utf8');

// Eagerly load fixtures at module init so request handlers stay synchronous.
const SEARCH = readJson('search.json');
const TOPOLOGY = readJson('topology.json');
const TRACE = readJson<{ trace_id: string; root_span_id: string; spans: unknown[]; truncated: boolean }>('trace.json');
const STREAMS = readJson('streams.json');
const DASHBOARDS = readJson('dashboards.json');
const ALERTS = readJson<{
  rules: unknown[];
  incidents: unknown[];
  escalations: unknown[];
}>('alerts.json');
const LOG_NDJSON = readText('log-stream.ndjson');

const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';
const EXPLICIT_THEME_KEY = 'molesignal-theme-explicit';
const AUTH_KEY = 'molesignal-auth';
const MOCK_AUTH = {
  state: {
    token: 'mock-e2e-token',
    ctx: {
      user_id: 'dev',
      org_id: 'acme-prod',
      org_name: 'acme-prod',
      display_role: 'Owner',
      roles: [
        {
          id: 'role-owner',
          key: 'owner',
          name: 'Owner',
          builtin: true,
        },
      ],
      scope: 'organization',
      display_name: 'Dev User',
      email: 'dev@molesignal.local',
    },
  },
  version: 0,
};

interface Fixtures {
  mockServer: { port: number };
}

export function registerRoutes(app: Express): void {
  const dashboards = (DASHBOARDS as { items: Array<Record<string, unknown>> }).items.map((item) => ({ ...item }));
  const folders: Array<{ id: string; org_id: string; name: string; parent_id?: string }> = [];
  const resourceShares: Array<Record<string, unknown> & { token: string }> = [];
  const unlockedShareSessions = new Set<string>();
  let resourceSharePolicy = {
    organization_id: 'acme-prod',
    allow_public_links: true,
    allow_public_dashboards: true,
    max_public_expiry_secs: 7 * 24 * 60 * 60,
    require_public_report_password: false,
    deny_production_public_shares: false,
    allow_public_csv_download: false,
    updated_by: 'dev',
    updated_at: Date.parse(FROZEN_NOW_ISO) * 1_000,
  };
  const scheduledReports: Array<Record<string, unknown>> = [
    {
      id: 'r1',
      org_id: 'acme-prod',
      name: 'Weekly SLO',
      description: 'Core service availability and latency review.',
      dashboard_id: 'd1',
      saved_view_id: null,
      cron: 'every:7d',
      recipients: [{ kind: 'email', target: 'sre@example.com' }],
      format: 'pdf',
      time_range_json: {
        preset: 'previous-7-days',
        timezone: 'UTC',
        description: 'Core service availability and latency review.',
      },
      enabled: true,
      last_run_at_micros: Date.parse(FROZEN_NOW_ISO) * 1_000 - 86_400_000_000,
      created_at_micros: Date.parse(FROZEN_NOW_ISO) * 1_000 - 604_800_000_000,
      updated_at_micros: Date.parse(FROZEN_NOW_ISO) * 1_000,
    },
  ];
  const chatMessages = new Map<string, Array<Record<string, unknown>>>();
  const intelligenceChats: Array<Record<string, unknown>> = [];
  let userPreferences = {
    theme: 'system',
    density: 'normal',
    language: 'en-us',
    default_home_route: '/home',
    time_format: 'iso_24h',
    date_format: 'yyyy_mm_dd_dash',
    timezone: '',
    keyboard_shortcuts_enabled: true,
  };
  let hasPersonalPreferences = false;
  let workspacePreferenceDefaults = { ...userPreferences };
  let meProfile = {
    user_id: 'dev',
    email: 'dev@molesignal.local',
    username: 'Dev User',
    display_name: 'Dev User',
    avatar_url: null as string | null,
    bio: '',
    org_id: 'acme-prod',
    org_name: 'acme-prod',
    org_slug: 'acme-prod',
    display_role: 'Owner',
    created_at_micros: 1_753_584_000_000_000,
  };

  app.post('/api/v1/auth/signin', (_req, res) =>
    res.json({
      token: 'mock-dev-token',
      user_id: 'dev',
      email: 'dev@molesignal.local',
      display_name: 'Dev User',
      org_id: 'acme-prod',
      org_name: 'acme-prod',
      display_role: 'Owner',
      roles: [
        {
          id: 'role-owner',
          key: 'owner',
          name: 'Owner',
          builtin: true,
        },
      ],
    }),
  );
  app.post('/api/v1/auth/forgot-password', (_req, res) =>
    res.status(202).json({ accepted: true }),
  );
  app.get('/api/v1/me/profile', (_req, res) => res.json(meProfile));
  app.put('/api/v1/me/profile', (req, res) => {
    meProfile = { ...meProfile, ...req.body };
    if (req.body.avatar_url === '') meProfile.avatar_url = null;
    res.json(meProfile);
  });
  app.get('/api/v1/me/preferences', (_req, res) =>
    res.json(
      hasPersonalPreferences ? userPreferences : workspacePreferenceDefaults,
    ),
  );
  app.put('/api/v1/me/preferences', (req, res) => {
    userPreferences = { ...userPreferences, ...req.body };
    hasPersonalPreferences = true;
    res.json(userPreferences);
  });
  app.get('/api/v1/workspace/preferences', (_req, res) =>
    res.json(workspacePreferenceDefaults),
  );
  app.put('/api/v1/workspace/preferences', (req, res) => {
    workspacePreferenceDefaults = {
      ...workspacePreferenceDefaults,
      ...req.body,
    };
    res.json(workspacePreferenceDefaults);
  });
  app.get('/api/v1/instance', (_req, res) =>
    res.json({
      external_url: '',
      signup_enabled: false,
      version: '26.0.0.0',
      release_channel: 'stable',
    }),
  );
  let clientIpSettings = {
    mode: 'peer',
    header_name: '',
    trusted_proxy_cidrs: [] as string[],
    fallback_to_peer: true,
    allow_private_client_ips: false,
    max_chain_length: 16,
  };
  app.get('/api/v1/settings/client_ip', (_req, res) =>
    res.json(clientIpSettings),
  );
  app.put('/api/v1/settings/client_ip', (req, res) => {
    clientIpSettings = { ...clientIpSettings, ...req.body };
    res.json(clientIpSettings);
  });
  app.get('/api/v1/version', (_req, res) =>
    res.json({
      version: '26.0.0.0',
      commit: 'd86fa2d15d68',
      branch: 'main',
      build_epoch_secs: 1_785_087_406,
      build_id: 'gha-12345-1',
      release_channel: 'stable',
      edition: 'enterprise',
    }),
  );

  // ── web/* (search, topology, trace, correlation, investigation blob) ──
  app.get('/api/v1/web/search', (_req, res) => res.json(SEARCH));
  app.get('/api/v1/web/topology', (_req, res) => res.json(TOPOLOGY));
  app.get('/api/v1/web/trace/:id', (req, res) => res.json({ ...TRACE, trace_id: req.params.id }));
  app.get('/api/v1/web/correlation/:from/:to', (_req, res) =>
    res.json({
      time_range: { from: FROZEN_NOW_ISO, to: FROZEN_NOW_ISO },
      filters: [],
      prefill: {},
    }),
  );
  const investigationBlobs = new Map<string, unknown>();
  app.post('/api/v1/web/investigation/blob', (req: Request, res: Response) => {
    const id = `blob-${investigationBlobs.size + 1}`;
    investigationBlobs.set(id, req.body);
    res.json({ blob_id: id });
  });
  app.get('/api/v1/web/investigation/blob/:id', (req, res) => {
    const blob = investigationBlobs.get(req.params.id);
    if (!blob) return res.status(404).json({ error: 'not found' });
    return res.json(blob);
  });

  // ── streams / dashboards / saved_views ──
  app.get('/api/v1/streams', (_req, res) => res.json(STREAMS));
  // `dashboardsApi.list()` expects a bare `Dashboard[]` (matches the real
  // backend), not the `{ items }` envelope the fixture file uses for the
  // `/dashboards/:id` lookup below. Returning the envelope made the
  // Dashboards page crash on `.map` (blank render) under e2e.
  app.get('/api/v1/dashboards', (_req, res) => res.json(dashboards));
  app.post('/api/v1/dashboards', (req: Request, res: Response) => {
    const model =
      req.body?.model && typeof req.body.model === 'object'
        ? (req.body.model as Record<string, unknown>)
        : {};
    if (
      model.engine !== 'molesignal-dashboard' ||
      model.schemaVersion !== 2 ||
      !Array.isArray(model.elements)
    ) {
      return res.status(400).json({ error: 'invalid dashboard model' });
    }
    const nowMicros = Date.parse(FROZEN_NOW_ISO) * 1000;
    const id = `dash-${dashboards.length + 1}`;
    const uid =
      typeof model.uid === 'string' && model.uid ? model.uid : id;
    Object.assign(model, { id, uid, version: 1 });
    const dashboard = {
      id,
      org_id: 'acme-prod',
      folder_id: typeof req.body?.folder_id === 'string' ? req.body.folder_id : undefined,
      uid,
      title: typeof model.title === 'string' && model.title ? model.title : 'Untitled dashboard',
      tags: Array.isArray(model.tags) ? model.tags : [],
      model,
      version: 1,
      created_at: nowMicros,
      updated_at: nowMicros,
    };
    dashboards.push(dashboard);
    res.json(dashboard);
  });
  app.get('/api/v1/dashboards/:id', (req, res) => {
    const d = dashboards.find((x) => x.id === req.params.id);
    return d ? res.json(d) : res.status(404).json({ error: 'not found' });
  });
  app.put('/api/v1/dashboards/:id', (req: Request, res: Response) => {
    const dashboard = dashboards.find((item) => item.id === req.params.id);
    if (!dashboard) return res.status(404).json({ error: 'not found' });
    const model =
      req.body?.model && typeof req.body.model === 'object'
        ? (req.body.model as Record<string, unknown>)
        : {};
    if (
      model.engine !== 'molesignal-dashboard' ||
      model.schemaVersion !== 2 ||
      !Array.isArray(model.elements)
    ) {
      return res.status(400).json({ error: 'invalid dashboard model' });
    }
    const nextVersion =
      (typeof dashboard.version === 'number' ? dashboard.version : 0) + 1;
    Object.assign(model, {
      id: dashboard.id,
      uid: dashboard.uid,
      version: nextVersion,
    });
    dashboard.model = model;
    dashboard.title =
      typeof model.title === 'string' ? model.title : dashboard.title;
    dashboard.tags = Array.isArray(model.tags) ? model.tags : [];
    dashboard.version = nextVersion;
    dashboard.updated_at = Date.parse(FROZEN_NOW_ISO) * 1000;
    dashboard.folder_id =
      typeof req.body?.folder_id === 'string'
        ? req.body.folder_id
        : undefined;
    return res.json(dashboard);
  });
  app.delete('/api/v1/dashboards/:id', (req, res) => {
    const index = dashboards.findIndex((item) => item.id === req.params.id);
    if (index < 0) return res.status(404).json({ error: 'not found' });
    dashboards.splice(index, 1);
    return res.json({ deleted: req.params.id });
  });
  app.get('/api/v1/folders', (_req, res) => res.json(folders));
  app.post('/api/v1/folders', (req: Request, res: Response) => {
    const name = String(req.body?.name ?? '').trim();
    if (!name) return res.status(400).json({ error: 'folder name must not be empty' });
    const parentId = typeof req.body?.parent_id === 'string' && req.body.parent_id.trim()
      ? req.body.parent_id.trim()
      : undefined;
    if (parentId && !folders.some((folder) => folder.id === parentId)) {
      return res.status(404).json({ error: `parent folder ${parentId} not found` });
    }
    const folder = {
      id: `folder-${folders.length + 1}`,
      org_id: 'acme-prod',
      name,
      ...(parentId ? { parent_id: parentId } : {}),
    };
    folders.push(folder);
    return res.json(folder);
  });
  app.put('/api/v1/folders/:id', (req: Request, res: Response) => {
    const folder = folders.find((item) => item.id === req.params.id);
    if (!folder) return res.status(404).json({ error: 'not found' });
    const name = String(req.body?.name ?? '').trim();
    if (!name) return res.status(400).json({ error: 'folder name must not be empty' });
    const parentId = typeof req.body?.parent_id === 'string' && req.body.parent_id.trim()
      ? req.body.parent_id.trim()
      : undefined;
    if (parentId === folder.id) return res.status(400).json({ error: 'folder cannot be its own parent' });
    if (parentId && !folders.some((item) => item.id === parentId)) {
      return res.status(404).json({ error: `parent folder ${parentId} not found` });
    }
    folder.name = name;
    if (parentId) folder.parent_id = parentId;
    else delete folder.parent_id;
    return res.json(folder);
  });
  app.delete('/api/v1/folders/:id', (req, res) => {
    const index = folders.findIndex((folder) => folder.id === req.params.id);
    if (index < 0) return res.status(404).json({ error: 'not found' });
    const hasChild = folders.some((folder) => folder.parent_id === req.params.id);
    const hasDashboard = dashboards.some((dashboard) => dashboard.folder_id === req.params.id);
    if (hasChild || hasDashboard) {
      return res.status(409).json({ error: 'folder is not empty: move or delete its dashboards and sub-folders first' });
    }
    folders.splice(index, 1);
    return res.json({ deleted: req.params.id });
  });
  app.get('/api/v1/saved-views', (_req, res) =>
    res.json({ items: [{ id: 'sv1', name: 'Yesterday errors', filters: [] }] }),
  );
  app.get('/api/v1/saved_views', (_req, res) =>
    res.json([{ id: 'sv1', name: 'Yesterday errors', filters: [] }]),
  );

  // ── alerts (rules / incidents / escalations) ──
  app.get('/api/v1/alerts/rules', (_req, res) => res.json(ALERTS.rules));
  app.get('/api/v1/alerts/incidents', (_req, res) => res.json(ALERTS.incidents));
  app.get('/api/v1/alerts/escalations', (_req, res) => res.json(ALERTS.escalations));

  // ── Notify settings ──
  app.get('/api/v1/notify/connector-types', (_req, res) => res.json([]));
  app.get('/api/v1/notify/connectors', (_req, res) => res.json([]));
  app.get('/api/v1/notify/users', (_req, res) => res.json([]));
  app.get('/api/v1/notify/recipient-resolver-types', (_req, res) =>
    res.json(['fixed_users', 'team_members', 'oncall_schedule']),
  );
  app.get('/api/v1/notify/policies', (_req, res) => res.json([]));
  app.get('/api/v1/notify/templates', (_req, res) => res.json([]));
  app.get('/api/v1/notify/deliveries', (_req, res) => res.json([]));
  app.get('/api/v1/notify/organization-defaults', (_req, res) => res.json([]));
  app.get('/api/v1/notify/team-defaults/:teamId', (_req, res) => res.json([]));

  // ── Mole Intelligence chat + audit ──
  app.get('/api/v1/intelligence/chat', (_req, res) => res.json(intelligenceChats));
  app.post('/api/v1/intelligence/chat', (req, res) => {
    const now = Date.now() * 1000;
    const id = `mock-chat-${now}`;
    chatMessages.set(id, []);
    const chat = {
      id,
      provider: req.body?.provider ?? 'openai',
      model: req.body?.model ?? 'gpt-4o',
      title: req.body?.title ?? 'Mole Intelligence 对话',
      provider_id: req.body?.provider_id ?? null,
      analysis_mode: req.body?.analysis_mode ?? null,
      time_range_start_micros: null,
      time_range_end_micros: null,
      archive_object_key: null,
      created_at_micros: now,
      updated_at_micros: now,
    };
    intelligenceChats.unshift(chat);
    res.json(chat);
  });
  app.get('/api/v1/intelligence/chat/:id/messages', (req, res) =>
    res.json({ messages: chatMessages.get(req.params.id) ?? [] }),
  );
  app.post('/api/v1/intelligence/chat/:id/messages', (req, res) => {
    const now = Date.now() * 1000;
    const messages = chatMessages.get(req.params.id) ?? [];
    chatMessages.set(req.params.id, messages);
    if (!req.body?.regenerate_from_message_id) {
      messages.push({
        id: `mock-user-${now}`,
        chat_id: req.params.id,
        org_id: 'default',
        role: 'user',
        content: req.body?.content ?? '',
        created_at_micros: now,
      });
    }
    res.setHeader('content-type', 'text/event-stream');
    res.setHeader('cache-control', 'no-cache');
    res.flushHeaders();
    res.write(
      `event: tool_start\ndata: ${JSON.stringify({
        id: `mock-tool-${now}`,
        name: 'list_streams',
        arguments: JSON.stringify({ time_range: req.body?.time_range ?? null }),
      })}\n\n`,
    );
    setTimeout(() => {
      const answer = JSON.stringify({
        summary: 'The selected data scope is available for investigation.',
        evidence: [{ label: 'One observable data source is available', kind: 'logs' }],
        likely_causes: [],
        limitations: [],
        suggested_next_steps: ['Continue with the selected service and time range'],
        related_links: [],
        confidence: 'medium',
      });
      messages.push({
        id: `mock-assistant-${Date.now() * 1000}`,
        chat_id: req.params.id,
        org_id: 'default',
        role: 'assistant',
        content: answer,
        evidence_json: [
          {
            tool_call_id: `mock-tool-${now}`,
            tool: 'list_streams',
            status: 'success',
            summary: '1 row',
            row_count: 1,
            took_ms: 84,
            arguments: { time_range: req.body?.time_range ?? null },
          },
        ],
        created_at_micros: Date.now() * 1000,
      });
      const chat = intelligenceChats.find((item) => item.id === req.params.id);
      if (chat) chat.updated_at_micros = Date.now() * 1000;
      res.write(
        `event: tool_end\ndata: ${JSON.stringify({
          id: `mock-tool-${now}`,
          result: JSON.stringify({ streams: ['logs/default'], row_count: 1 }),
          is_error: false,
        })}\n\n`,
      );
      res.write(`event: chunk\ndata: ${JSON.stringify({ text: answer })}\n\n`);
      res.write('event: done\ndata: {"prompt_tokens":10,"completion_tokens":12,"finish_reason":"stop"}\n\n');
      res.end();
    }, 350);
  });
  app.post('/api/v1/intelligence/chat/:id/archive', (req, res) =>
    res.json({
      status: 'ok',
      object_key: `intelligence/chat/default/${req.params.id}/transcript.json`,
    }),
  );
  app.delete('/api/v1/intelligence/chat/:id', (req, res) => {
    chatMessages.delete(req.params.id);
    const index = intelligenceChats.findIndex((item) => item.id === req.params.id);
    if (index >= 0) intelligenceChats.splice(index, 1);
    res.json({});
  });
  app.get('/api/v1/intelligence/audit/chat/:id', (req, res) => {
    const now = Date.now() * 1000;
    const messages = req.params.id === 'audit-chat-seeded'
      ? [
          {
            id: 'audit-chat-user-1',
            chat_id: 'audit-chat-seeded',
            org_id: 'default',
            role: 'user',
            content: '过去一小时发生了什么变化？',
            created_at_micros: now - 2_000_000,
          },
          {
            id: 'audit-chat-assistant-1',
            chat_id: 'audit-chat-seeded',
            org_id: 'default',
            role: 'assistant',
            content: '**摘要**\n\n- `checkout` 错误率上升\n- p95 延迟升高\n\n建议先查看最近部署。',
            prompt_builtin_key: 'analysis.anomaly',
            prompt_version: 1,
            prompt_hash: 'mock-hash',
            evidence_json: [{ tool: 'query_logs', status: 'success', row_count: 12 }],
            prompt_tokens: 128,
            completion_tokens: 64,
            cost_usd: 0.0012,
            created_at_micros: now - 1_000_000,
          },
        ]
      : (chatMessages.get(req.params.id) ?? []);
    res.json({
      chat: {
        id: req.params.id,
        provider: 'openai',
        model: 'gpt-4o',
        title: 'Audit chat transcript',
        provider_id: 'openai-gpt-4o',
        analysis_mode: 'anomaly_analysis',
        time_range_start_micros: now - 3_600_000_000,
        time_range_end_micros: now,
        archive_object_key: `intelligence/chat/default/${req.params.id}/transcript.json`,
        deleted_at_micros: null,
        created_at_micros: now - 3_000_000,
        updated_at_micros: now - 500_000,
      },
      messages,
    });
  });
  app.get('/api/v1/audit', (_req, res) =>
    res.json({
      items: [
        {
          id: 'audit-intelligence-chat-1',
          org_id: 'default',
          actor_kind: 'user',
          actor_id: 'dev',
          action: 'intelligence.chat.archived',
          target_kind: 'intelligence_chat',
          target_id: 'audit-chat-seeded',
          ip: '127.0.0.1',
          user_agent: 'mock',
          payload: {
            status: 'ok',
            object_key: 'intelligence/chat/default/audit-chat-seeded/transcript.json',
            chat_id: 'audit-chat-seeded',
          },
          ts_micros: Date.now() * 1000,
        },
      ],
      next_cursor: null,
    }),
  );
  const modelProviders: Array<Record<string, unknown>> = [
    {
      id: 'openai-gpt-4o',
      provider: 'openai',
      name: 'OpenAI Production',
      base_url: null,
      default_model: 'gpt-4o',
      enabled: true,
      timeout_ms: 30_000,
      max_tokens: 4096,
      key_last4: '1234',
      key_set: true,
      created_at_micros: Date.now() * 1000,
      updated_at_micros: Date.now() * 1000,
    },
    {
      id: 'compatible-qwen',
      provider: 'openai_compatible',
      name: 'Internal Qwen',
      base_url: 'https://llm.internal.example/v1',
      default_model: 'qwen2.5-72b-instruct',
      enabled: true,
      timeout_ms: 30_000,
      max_tokens: 8192,
      key_last4: '5678',
      key_set: true,
      created_at_micros: Date.now() * 1000,
      updated_at_micros: Date.now() * 1000,
    },
  ];
  app.get('/api/v1/intelligence/settings/model-providers', (_req, res) =>
    res.json(modelProviders),
  );
  app.post('/api/v1/intelligence/settings/model-providers', (req, res) => {
    const provider = {
      ...req.body,
      id: `model-provider-${modelProviders.length + 1}`,
      key_last4:
        typeof req.body?.api_key === 'string' ? req.body.api_key.slice(-4) : null,
      key_set: Boolean(req.body?.api_key),
      created_at_micros: Date.now() * 1000,
      updated_at_micros: Date.now() * 1000,
    };
    delete provider.api_key;
    modelProviders.push(provider);
    res.json(provider);
  });
  app.put('/api/v1/intelligence/settings/model-providers/:id', (req, res) => {
    const provider = modelProviders.find((item) => item.id === req.params.id);
    if (!provider) return res.status(404).json({ error: 'not found' });
    Object.assign(provider, req.body, { updated_at_micros: Date.now() * 1000 });
    return res.json(provider);
  });
  app.post(
    '/api/v1/intelligence/settings/model-providers/:id/rotate-key',
    (req, res) => {
      const provider = modelProviders.find((item) => item.id === req.params.id);
      if (!provider) return res.status(404).json({ error: 'not found' });
      Object.assign(provider, {
        key_last4:
          typeof req.body?.api_key === 'string' ? req.body.api_key.slice(-4) : null,
        key_set: Boolean(req.body?.api_key),
        updated_at_micros: Date.now() * 1000,
      });
      return res.json(provider);
    },
  );
  const promptTemplates: Array<Record<string, unknown>> = [
    {
      id: 'prompt-system-default',
      org_id: null,
      user_id: null,
      scope: 'builtin',
      builtin_key: 'system.default',
      purpose: 'system',
      name: 'Mole Agent system instruction',
      body:
        'You are Mole Agent for {{ org_name }}. Use authorized evidence and respect approval boundaries. Current time: {{ current_time }}.',
      variables_schema: {
        type: 'object',
        properties: {
          org_name: { type: 'string' },
          current_time: { type: 'string' },
        },
      },
      is_default: true,
      enabled: true,
      version: 1,
      parent_id: null,
      created_at_micros: Date.now() * 1000,
      updated_at_micros: Date.now() * 1000,
    },
    {
      id: 'prompt-root-cause-default',
      org_id: null,
      user_id: null,
      scope: 'builtin',
      builtin_key: 'root_cause.default',
      purpose: 'root_cause',
      name: 'Root-cause investigation',
      body:
        'Investigate the selected streams {{ streams }} over {{ time_range }} and report evidence.',
      variables_schema: {
        type: 'object',
        properties: {
          streams: { type: 'string' },
          time_range: { type: 'string' },
        },
      },
      is_default: true,
      enabled: true,
      version: 1,
      parent_id: null,
      created_at_micros: Date.now() * 1000,
      updated_at_micros: Date.now() * 1000,
    },
  ];
  app.get('/api/v1/intelligence/settings/prompts', (_req, res) =>
    res.json(promptTemplates),
  );
  app.post('/api/v1/intelligence/settings/prompts', (req, res) => {
    const now = Date.now() * 1000;
    const prompt = {
      ...req.body,
      id: `prompt-${promptTemplates.length + 1}`,
      org_id: 'acme-prod',
      user_id: req.body?.scope === 'user' ? 'root' : null,
      is_default: false,
      enabled: req.body?.enabled ?? true,
      version: 1,
      created_at_micros: now,
      updated_at_micros: now,
    };
    promptTemplates.push(prompt);
    return res.json(prompt);
  });
  app.put('/api/v1/intelligence/settings/prompts/:id', (req, res) => {
    const prompt = promptTemplates.find((item) => item.id === req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not found' });
    if (prompt.scope === 'builtin') {
      return res.status(400).json({ error: 'builtin prompts are immutable' });
    }
    Object.assign(prompt, req.body, {
      version: Number(prompt.version ?? 1) + 1,
      updated_at_micros: Date.now() * 1000,
    });
    return res.json(prompt);
  });
  app.post(
    '/api/v1/intelligence/settings/prompts/:id/set-default',
    (req, res) => {
      const prompt = promptTemplates.find((item) => item.id === req.params.id);
      if (!prompt) return res.status(404).json({ error: 'not found' });
      for (const candidate of promptTemplates) {
        if (
          candidate.scope === prompt.scope &&
          candidate.purpose === prompt.purpose
        ) {
          candidate.is_default = false;
        }
      }
      prompt.is_default = true;
      prompt.enabled = true;
      prompt.updated_at_micros = Date.now() * 1000;
      return res.json(prompt);
    },
  );
  app.post('/api/v1/intelligence/settings/prompts/:id/restore', (req, res) => {
    const prompt = promptTemplates.find((item) => item.id === req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not found' });
    const builtin = promptTemplates.find(
      (item) =>
        item.scope === 'builtin' &&
        item.builtin_key === prompt.builtin_key,
    );
    if (!builtin) return res.status(400).json({ error: 'no builtin parent' });
    Object.assign(prompt, {
      body: builtin.body,
      variables_schema: builtin.variables_schema,
      version: Number(prompt.version ?? 1) + 1,
      updated_at_micros: Date.now() * 1000,
    });
    return res.json(prompt);
  });
  app.delete('/api/v1/intelligence/settings/prompts/:id', (req, res) => {
    const index = promptTemplates.findIndex((item) => item.id === req.params.id);
    if (index === -1) return res.status(404).json({ error: 'not found' });
    promptTemplates.splice(index, 1);
    return res.json({ deleted: true });
  });

  // ── Mole Intelligence control plane ──
  const nowMicros = Date.parse(FROZEN_NOW_ISO) * 1000;
  const investigations: Array<Record<string, unknown>> = [
    {
      id: 'investigation-checkout',
      org_id: 'acme-prod',
      created_by: 'dev',
      chat_id: null,
      title: 'checkout-api 错误率升高调查',
      status: 'running',
      context: { service: 'checkout-api', environment: 'production' },
      summary: '错误率在最近一次发布后升高。',
      confidence: 'medium',
      current_step: '关联异常链路',
      started_at: nowMicros - 1_800_000_000,
      completed_at: null,
      created_at: nowMicros - 2_400_000_000,
      updated_at: nowMicros - 300_000_000,
    },
  ];
  const automations: Array<Record<string, unknown>> = [
    {
      id: 'automation-critical-alert',
      name: '生产环境严重告警调查',
      description: '严重告警触发的只读可观测性调查工作流',
      enabled: true,
      trigger: { type: 'alert', severity: 'critical' },
      input_context: { environment: 'production' },
      steps: ['查询关键指标', '查询异常日志', '关联异常链路', '输出根因假设'],
      allowed_tools: [
        'query_logs',
        'query_metrics',
        'get_trace',
        'list_recent_alerts',
        'get_current_on_call',
      ],
      approval_policy: { write_operations: 'required' },
      output_actions: [],
      failure_policy: { strategy: 'stop' },
      notification: {},
      created_by: 'dev',
      created_at: nowMicros - 3_600_000_000,
      updated_at: nowMicros - 600_000_000,
    },
  ];
  const agentProfiles: Array<Record<string, unknown>> = [
    {
      id: 'profile-production',
      name: '生产环境受限 Agent',
      description: '生产环境只读分析与审批后处置配置',
      model_provider_id: 'compatible-qwen',
      model: null,
      allowed_tools: [
        'query_logs',
        'query_metrics',
        'list_streams',
        'get_trace',
        'list_recent_alerts',
        'list_on_call_schedules',
        'get_current_on_call',
        'list_rum_sessions',
        'list_rum_actions',
        'list_rum_errors',
        'list_continuous_profiles',
        'list_report_templates',
        'list_scheduled_reports',
        'propose_operation',
      ],
      data_scope: {
        environments: ['production'],
        services: [],
        streams: [],
        cross_organization: false,
      },
      risk_policy: {
        l0: 'automatic',
        l1: 'automatic',
        l2: 'approval',
        l3: 'two_person_approval',
      },
      network_access: 'blocked',
      max_context_tokens: 32_000,
      max_investigation_secs: 1_800,
      max_tool_calls: 32,
      is_default: true,
      enabled: true,
      created_by: 'dev',
      created_at: nowMicros - 7_200_000_000,
      updated_at: nowMicros - 900_000_000,
    },
  ];
  app.get('/api/v1/intelligence/overview', (_req, res) =>
    res.json({
      active_investigations: 0,
      pending_approvals: 0,
      recent_completed: 0,
      automation_runs: 0,
      enabled_automations: 0,
    }),
  );
  app.get('/api/v1/intelligence/investigations', (_req, res) =>
    res.json({ investigations }),
  );
  app.post('/api/v1/intelligence/investigations', (req, res) => {
    const investigation = {
      id: `investigation-${investigations.length + 1}`,
      org_id: 'acme-prod',
      created_by: 'dev',
      chat_id: null,
      status: 'draft',
      summary: null,
      confidence: null,
      current_step: null,
      started_at: null,
      completed_at: null,
      created_at: nowMicros,
      updated_at: nowMicros,
      ...req.body,
    };
    investigations.push(investigation);
    res.json(investigation);
  });
  app.get('/api/v1/intelligence/investigations/:id', (req, res) => {
    const investigation = investigations.find((item) => item.id === req.params.id);
    if (!investigation) return res.status(404).json({ error: 'not found' });
    return res.json({
      investigation,
      steps: [],
      evidence: [],
      hypotheses: [],
    });
  });
  app.put('/api/v1/intelligence/investigations/:id', (req, res) => {
    const investigation = investigations.find((item) => item.id === req.params.id);
    if (!investigation) return res.status(404).json({ error: 'not found' });
    Object.assign(investigation, req.body, { updated_at: nowMicros });
    return res.json(investigation);
  });
  app.get('/api/v1/intelligence/automations', (_req, res) =>
    res.json({ automations }),
  );
  app.post('/api/v1/intelligence/automations', (req, res) => {
    const automation = {
      id: `automation-${automations.length + 1}`,
      created_by: 'dev',
      created_at: nowMicros,
      updated_at: nowMicros,
      ...req.body,
    };
    automations.push(automation);
    res.json(automation);
  });
  app.put('/api/v1/intelligence/automations/:id', (req, res) => {
    const automation = automations.find((item) => item.id === req.params.id);
    if (!automation) return res.status(404).json({ error: 'not found' });
    Object.assign(automation, req.body, { updated_at: nowMicros });
    return res.json(automation);
  });
  app.post('/api/v1/intelligence/automations/:id/dry-run', (_req, res) =>
    res.json({ status: 'completed', writes: 0 }),
  );
  app.get('/api/v1/intelligence/approvals', (_req, res) =>
    res.json({
      approvals: [
        {
          id: 'approval-checkout-alert',
          investigation_id: null,
          action: '确认告警事件',
          target: 'production / checkout-api / high-error-rate',
          parameters: { incident_id: 'incident-checkout-500' },
          reason: 'checkout-api 错误率持续高于阈值，值班工程师已完成初步确认。',
          impact: '仅更新告警确认状态，不会修改服务配置或工作负载。',
          risk: 'l2',
          status: 'pending',
          requested_by: 'dev',
          required_approvals: 1,
          reviews: [],
          expires_at: 1_769_164_800_000_000,
          decided_at: null,
          created_at: 1_769_161_200_000_000,
          updated_at: 1_769_161_200_000_000,
        },
      ],
    }),
  );
  app.get('/api/v1/intelligence/executions', (_req, res) =>
    res.json({ executions: [] }),
  );
  app.get('/api/v1/intelligence/settings/agent-profiles', (_req, res) =>
    res.json({ profiles: agentProfiles }),
  );
  app.post('/api/v1/intelligence/settings/agent-profiles', (req, res) => {
    const profile = {
      id: `profile-${agentProfiles.length + 1}`,
      created_by: 'dev',
      created_at: nowMicros,
      updated_at: nowMicros,
      ...req.body,
    };
    agentProfiles.push(profile);
    res.json(profile);
  });
  app.put('/api/v1/intelligence/settings/agent-profiles/:id', (req, res) => {
    const profile = agentProfiles.find((item) => item.id === req.params.id);
    if (!profile) return res.status(404).json({ error: 'not found' });
    Object.assign(profile, req.body, { updated_at: nowMicros });
    return res.json(profile);
  });
  const makeTool = (
    name: string,
    description: string,
    domain: string,
    category: string,
    risk = 'l0',
    access = 'read_only',
    inputSchema: Record<string, unknown> = {
      type: 'object',
      properties: {},
      additionalProperties: false,
    },
  ): Record<string, unknown> => ({
    id: name,
    name,
    display_name: name,
    description,
    technical_description: description,
    domain,
    category,
    source: { kind: 'builtin', label: 'MoleSignal' },
    input_schema: inputSchema,
    output_schema: { type: 'object' },
    risk,
    minimum_risk: risk,
    execution_mode: risk === 'l0' ? 'automatic' : 'single_approval',
    enabled: true,
    available_to_agent: true,
    status: 'healthy',
    capabilities: {
      read_only: access === 'read_only',
      supports_dry_run: true,
      idempotent: access === 'read_only',
      streaming: false,
    },
    limits: {
      timeout_ms: 30_000,
      max_calls_per_run: 32,
      max_response_bytes: 1_048_576,
    },
    environment_overrides: {},
    tags: [category],
    statistics: {
      calls_24h: name === 'query_logs' ? 184 : 0,
      success_rate: name === 'query_logs' ? 99.4 : null,
      p95_ms: name === 'query_logs' ? 412 : null,
      last_called_at: name === 'query_logs' ? nowMicros - 120_000_000 : null,
      last_error: null,
    },
    access,
  });
  const intelligenceTools: Array<Record<string, unknown>> = [
    makeTool(
      'query_logs',
      '在授权范围内执行只读日志查询。',
      'observability',
      'Logs',
      'l0',
      'read_only',
      {
        type: 'object',
        required: ['query'],
        properties: {
          query: {
            type: 'string',
            title: '查询语句',
            description: '只读 SQL 或日志检索表达式。',
          },
          stream: { type: 'string', title: '数据流' },
          limit: { type: 'integer', title: '返回条数', default: 100 },
        },
        additionalProperties: false,
      },
    ),
    makeTool('query_metrics', '执行只读 PromQL 指标查询。', 'observability', 'Metrics'),
    makeTool('list_streams', '列出当前组织可查询的可观测数据流。', 'observability', 'Streams'),
    makeTool('get_trace', '按 Trace ID 查询完整链路。', 'observability', 'Trace'),
    makeTool('list_rum_sessions', '列出最近的真实用户会话。', 'observability', 'RUM'),
    makeTool('list_rum_actions', '列出 RUM 行为与 Web Vitals 事件。', 'observability', 'RUM'),
    makeTool('list_rum_errors', '列出最近的前端错误。', 'observability', 'RUM'),
    makeTool('list_continuous_profiles', '列出持续剖析数据。', 'observability', 'Profiles'),
    makeTool('list_recent_alerts', '列出当前活跃告警事件。', 'alerts_on_call', 'Alert'),
    makeTool(
      'list_on_call_schedules',
      '列出可用值班表及当前值班人。',
      'alerts_on_call',
      'On-call',
    ),
    makeTool(
      'get_current_on_call',
      '查询当前值班人；schedule_id 可选。',
      'alerts_on_call',
      'On-call',
    ),
    makeTool(
      'list_report_templates',
      '列出报告模板。',
      'dashboard_reports',
      'Reports',
    ),
    makeTool(
      'list_scheduled_reports',
      '列出定时报告。',
      'dashboard_reports',
      'Reports',
    ),
    makeTool(
      'propose_operation',
      '创建操作审批请求，但不会直接执行。',
      'automation',
      'Operations',
      'l2',
      'creates_approval_request',
      {
        type: 'object',
        required: ['operation', 'reason'],
        properties: {
          operation: { type: 'string', title: '操作' },
          target: { type: 'string', title: '目标' },
          reason: { type: 'string', title: '原因' },
        },
        additionalProperties: false,
      },
    ),
  ];
  const toolPolicyDefaults: Record<string, unknown> = {
    org_id: 'acme-prod',
    risk_modes: {
      l0: 'automatic',
      l1: 'confirmation',
      l2: 'single_approval',
      l3: 'dual_approval',
      l4: 'dual_approval',
    },
    environment_overrides: {
      production: {
        l0: 'automatic',
        l1: 'confirmation',
        l2: 'single_approval',
        l3: 'dual_approval',
        l4: 'dual_approval',
      },
    },
    updated_by: 'dev',
    created_at: nowMicros,
    updated_at: nowMicros,
  };
  const toolDependencies = (name: string) => {
    const dependent = name === 'query_logs';
    return {
      tool_name: name,
      total: dependent ? 2 : 0,
      agent_profiles: dependent
        ? [{ id: 'profile-production', name: '生产环境受限 Agent', enabled: true, is_default: true }]
        : [],
      automations: dependent
        ? [{ id: 'automation-critical-alert', name: '生产环境严重告警调查', enabled: true }]
        : [],
      investigation_templates: [],
    };
  };
  const findTool = (name: string) =>
    intelligenceTools.find((tool) => tool.id === name || tool.name === name);
  const mcpServers: Array<Record<string, unknown>> = [
    {
      id: 'mcp-observability',
      org_id: 'acme-prod',
      name: 'internal-observability',
      transport: 'streamable_http',
      endpoint_url: 'https://mcp.internal.example/v1',
      command_template: null,
      auth_type: 'bearer_token',
      auth_header: null,
      credential_last4: '7f2a',
      credential_set: true,
      private_only: true,
      allowed_domains: ['mcp.internal.example'],
      allowed_cidrs: ['10.0.0.0/8'],
      follow_redirects: false,
      tls_verify: true,
      timeout_ms: 10_000,
      max_response_bytes: 1_048_576,
      enabled: true,
      status: 'healthy',
      last_error: null,
      last_tested_at: nowMicros - 300_000_000,
      last_synced_at: nowMicros - 600_000_000,
      created_by: 'dev',
      created_at: nowMicros - 86_400_000_000,
      updated_at: nowMicros - 600_000_000,
      tool_count: 0,
    },
  ];

  app.get('/api/v1/intelligence/tools', (_req, res) =>
    res.json({
      tools: intelligenceTools,
      dynamic_http: false,
      shell: false,
      browser: false,
      open_mcp: true,
      mcp_servers: { total: mcpServers.length, healthy: 1, unhealthy: 0 },
    }),
  );
  app.get('/api/v1/intelligence/tools/policies', (_req, res) =>
    res.json(toolPolicyDefaults),
  );
  app.put('/api/v1/intelligence/tools/policies', (req, res) => {
    Object.assign(toolPolicyDefaults, req.body, {
      updated_by: 'dev',
      updated_at: nowMicros,
    });
    res.json(toolPolicyDefaults);
  });
  app.get('/api/v1/intelligence/tools/:id/dependencies', (req, res) =>
    res.json(toolDependencies(req.params.id)),
  );
  app.get('/api/v1/intelligence/tools/:id/calls', (req, res) =>
    res.json({
      calls:
        req.params.id === 'query_logs'
          ? [
              {
                id: 'tool-call-query-logs',
                tool_name: 'query_logs',
                chat_id: 'chat-checkout',
                investigation_id: null,
                risk: 'l0',
                input: { query: 'service = checkout-api', authorization: '<redacted>' },
                output_summary: '128 rows',
                status: 'success',
                error: null,
                duration_ms: 284,
                called_by: 'dev',
                call_source: 'chat',
                profile_id: 'profile-production',
                approval_id: null,
                policy_decision: { allowed: true, execution_mode: 'automatic' },
                audit_id: 'audit-tool-call-1',
                created_at: nowMicros - 120_000_000,
              },
            ]
          : [],
    }),
  );
  app.post('/api/v1/intelligence/tools/:id/test', (req, res) =>
    res.json({
      success: true,
      validated: true,
      dry_run: req.body?.dry_run !== false,
      executed: false,
      side_effects: false,
      duration_ms: 18,
      message: '参数校验与 dry-run 已完成。',
      request: req.body?.arguments ?? {},
      response: { rows: 0, preview: [] },
    }),
  );
  app.put('/api/v1/intelligence/tools/:id/policy', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    Object.assign(tool, req.body);
    return res.json(tool);
  });
  app.post('/api/v1/intelligence/tools/:id/enable', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    Object.assign(tool, { enabled: true, available_to_agent: true, status: 'healthy' });
    return res.json(tool);
  });
  app.post('/api/v1/intelligence/tools/:id/disable', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    const dependencies = toolDependencies(req.params.id);
    if (dependencies.total > 0 && !req.body?.force) {
      return res.status(409).json({ error: 'tool has active dependencies', dependencies });
    }
    Object.assign(tool, { enabled: false, available_to_agent: false, status: 'disabled' });
    return res.json({ tool, dependencies });
  });
  app.get('/api/v1/intelligence/tools/:id', (req, res) => {
    const tool = findTool(req.params.id);
    return tool
      ? res.json({ tool, dependencies: toolDependencies(req.params.id) })
      : res.status(404).json({ error: 'not found' });
  });

  app.get('/api/v1/intelligence/mcp-servers', (_req, res) =>
    res.json({ servers: mcpServers }),
  );
  app.post('/api/v1/intelligence/mcp-servers', (req, res) => {
    const server = {
      id: `mcp-${mcpServers.length + 1}`,
      org_id: 'acme-prod',
      command_template: null,
      credential_last4:
        typeof req.body?.credential === 'string'
          ? req.body.credential.slice(-4)
          : null,
      credential_set: Boolean(req.body?.credential),
      status: req.body?.enabled ? 'unavailable' : 'disabled',
      last_error: null,
      last_tested_at: null,
      last_synced_at: null,
      created_by: 'dev',
      created_at: nowMicros,
      updated_at: nowMicros,
      tool_count: 0,
      ...req.body,
    };
    delete server.credential;
    mcpServers.push(server);
    res.json(server);
  });
  app.get('/api/v1/intelligence/mcp-servers/:id', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    const tools = intelligenceTools.filter(
      (tool) => {
        const source = tool.source as
          | { kind?: string; server_id?: string }
          | undefined;
        return source?.kind === 'mcp' && source.server_id === req.params.id;
      },
    );
    return server
      ? res.json({ server, tools })
      : res.status(404).json({ error: 'not found' });
  });
  app.put('/api/v1/intelligence/mcp-servers/:id', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    const credential =
      typeof req.body?.credential === 'string' ? req.body.credential : '';
    const next = { ...req.body };
    delete next.credential;
    Object.assign(server, next, {
      ...(credential
        ? { credential_set: true, credential_last4: credential.slice(-4) }
        : {}),
      updated_at: nowMicros,
    });
    return res.json(server);
  });
  app.delete('/api/v1/intelligence/mcp-servers/:id', (req, res) => {
    const index = mcpServers.findIndex((item) => item.id === req.params.id);
    if (index >= 0) mcpServers.splice(index, 1);
    res.json({});
  });
  app.post('/api/v1/intelligence/mcp-servers/:id/test', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    Object.assign(server, { status: 'healthy', last_error: null, last_tested_at: nowMicros });
    return res.json({
      success: true,
      server,
      discovered_tools: [
        {
          name: 'search_knowledge',
          title: 'Search knowledge',
          description: 'Search the internal runbook knowledge base.',
          inputSchema: {
            type: 'object',
            required: ['query'],
            properties: { query: { type: 'string' } },
          },
          annotations: { readOnlyHint: true },
        },
        {
          name: 'restart_workload',
          title: 'Restart workload',
          description: 'Restart a selected workload.',
          inputSchema: {
            type: 'object',
            required: ['workload'],
            properties: { workload: { type: 'string' } },
          },
          annotations: { destructiveHint: true },
        },
      ],
    });
  });
  app.post('/api/v1/intelligence/mcp-servers/:id/sync', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    const selectedTools = Array.isArray(req.body?.selected_tools)
      ? req.body.selected_tools.map(String)
      : [];
    const imported = selectedTools.map((remoteName: string) => {
      const name = `mcp_internal_observability_${remoteName}`;
      const existing = findTool(name);
      if (existing) return existing;
      const destructive = remoteName === 'restart_workload';
      const tool = makeTool(
        name,
        destructive
          ? 'Restart a selected workload.'
          : 'Search the internal runbook knowledge base.',
        destructive ? 'automation' : 'knowledge_context',
        destructive ? 'Operations' : 'Knowledge',
        destructive ? 'l4' : 'l0',
      );
      Object.assign(tool, {
        remote_name: remoteName,
        display_name:
          remoteName === 'restart_workload'
            ? 'Restart workload'
            : 'Search knowledge',
        source: {
          kind: 'mcp',
          label: 'internal-observability',
          server_id: server.id,
          server_name: server.name,
        },
        execution_mode: destructive ? 'dual_approval' : 'automatic',
        enabled: false,
        available_to_agent: false,
        status: 'disabled',
        last_synced_at: nowMicros,
      });
      intelligenceTools.push(tool);
      return tool;
    });
    Object.assign(server, {
      last_synced_at: nowMicros,
      tool_count: selectedTools.length,
    });
    return res.json({ server, tools: imported });
  });

  // ── annotations / metrics / log query (NDJSON live tail) ──
  app.get('/api/v1/annotations', (_req, res) =>
    res.json({ items: [{ id: 'an1', at: FROZEN_NOW_ISO, text: 'deploy v0.2.0' }] }),
  );
  app.get('/api/v1/query/promql/capabilities', (_req, res) =>
    res.json({
      engine: 'molesignal-promql',
      version: 1,
      functions: [
        {
          label: 'rate',
          insert_text: 'rate(${1:metric}[${2:5m}])',
          detail: 'rate(range-vector)',
          documentation: 'Per-second average rate over a range vector.',
          kind: 'function',
        },
        {
          label: 'irate',
          insert_text: 'irate(${1:metric}[${2:5m}])',
          detail: 'irate(range-vector)',
          documentation: 'Instantaneous per-second rate.',
          kind: 'function',
        },
        {
          label: 'increase',
          insert_text: 'increase(${1:metric}[${2:5m}])',
          detail: 'increase(range-vector)',
          documentation: 'Total increase over a range vector.',
          kind: 'function',
        },
        {
          label: 'avg_over_time',
          insert_text: 'avg_over_time(${1:metric}[${2:5m}])',
          detail: 'avg_over_time(range-vector)',
          documentation: 'Average over a range vector.',
          kind: 'function',
        },
        {
          label: 'abs',
          insert_text: 'abs(${1:vector})',
          detail: 'abs(vector)',
          documentation: 'Absolute value.',
          kind: 'function',
        },
        {
          label: 'histogram_quantile',
          insert_text: 'histogram_quantile(${1:0.95}, ${2:vector})',
          detail: 'histogram_quantile(q, vector)',
          documentation: 'Calculates a histogram quantile.',
          kind: 'function',
        },
      ],
      aggregations: [],
      keywords: [],
      operators: [],
    }),
  );
  app.get('/api/v1/metrics/catalog', (_req, res) =>
    res.json({
      items: [
        { name: 'http_requests_total', metric_type: 'counter', labels: ['service', 'status'], field_count: 4 },
        { name: 'http_request_duration_seconds', metric_type: 'histogram', labels: ['service', 'route'], field_count: 6 },
        { name: 'process_cpu_seconds_total', metric_type: 'counter', labels: ['host'], field_count: 3 },
        { name: 'process_resident_memory_bytes', metric_type: 'gauge', labels: ['host'], field_count: 3 },
        { name: 'go_goroutines', metric_type: 'gauge', labels: ['host'], field_count: 3 },
        { name: 'tokio_active_tasks', metric_type: 'gauge', labels: ['service'], field_count: 3 },
        { name: 'db_query_duration_seconds', metric_type: 'histogram', labels: ['database', 'operation'], field_count: 6 },
        { name: 'queue_depth', metric_type: 'gauge', labels: ['queue'], field_count: 3 },
        { name: 'cache_hit_ratio', metric_type: 'gauge', labels: ['cache'], field_count: 3 },
        { name: 'rust_alloc_bytes', metric_type: 'gauge', labels: ['host'], field_count: 3 },
        { name: 'kafka_consumer_lag', metric_type: 'gauge', labels: ['consumer', 'topic'], field_count: 4 },
        { name: 'storage_disk_usage_ratio', metric_type: 'gauge', labels: ['host', 'mount'], field_count: 4 },
      ],
      next_cursor: null,
      previous_cursor: null,
    }),
  );
  app.post('/api/v1/query', (req: Request, res: Response) => {
    if (req.body?.language === 'promql') {
      const start = Number(req.body?.time_range?.start ?? Date.parse(FROZEN_NOW_ISO) * 1000 - 3_600_000_000);
      const end = Number(req.body?.time_range?.end ?? Date.parse(FROZEN_NOW_ISO) * 1000);
      const pointCount = 61;
      const statuses = [
        { label: '500', base: 23, amplitude: 7, trend: 8, phase: 0.2 },
        { label: '502', base: 8, amplitude: 4, trend: -1, phase: 1.4 },
        { label: '503', base: 3, amplitude: 1.6, trend: 1, phase: 2.1 },
      ];
      const rows = statuses.flatMap((status) =>
        Array.from({ length: pointCount }, (_, index) => {
          const progress = index / (pointCount - 1);
          const timestamp = start + (end - start) * progress;
          const value = Math.max(
            0,
            status.base +
              Math.sin(index / 5 + status.phase) * status.amplitude +
              Math.sin(index / 2.2) * status.amplitude * 0.28 +
              progress * status.trend,
          );
          return [timestamp, Number(value.toFixed(3)), status.label];
        }),
      );
      return res.json({
        columns: ['_timestamp', 'value', 'status'],
        rows,
        scanned_rows: 8421,
        took_ms: 14,
      });
    }
    return res.json({ rows: [{ t: FROZEN_NOW_ISO, v: 1 }], took_ms: 2 });
  });
  app.get('/api/v1/metrics', (_req, res) => res.json({ series: [] }));
  // /query/stream streams an initial batch, then after 3s streams a second
  // batch followed by `__meta__`. The 3s gap is the window for the live-tail
  // spec to scroll up between batches so the trailing rows become
  // `newRowsCount > 0` and the "new rows" badge appears.
  const streamLogs = (_req: Request, res: Response): void => {
    res.setHeader('content-type', 'application/x-ndjson');
    res.flushHeaders();
    const lines = LOG_NDJSON.trim().split('\n');
    // Initial 3 lines fire immediately.
    for (let i = 0; i < 3 && i < lines.length; i++) {
      res.write(lines[i] + '\n');
    }
    // Remaining lines (incl. final `__meta__`) fire after 3s.
    setTimeout(() => {
      for (let i = 3; i < lines.length; i++) {
        res.write(lines[i] + '\n');
      }
      res.end();
    }, 3000);
  };
  app.get('/api/v1/query/stream', streamLogs);
  app.post('/api/v1/query/stream', streamLogs);

  // ── admin: sso / orgs / users / domains / scheduled-reports / schedules ──
  app.get('/api/v1/auth/sso/providers', (_req, res) =>
    res.json([{ id: 'okta', name: 'Okta', kind: 'oidc' }]),
  );
  app.get('/api/v1/sso/providers', (_req, res) =>
    res.json([{ id: 'okta', name: 'Okta', kind: 'oidc', enabled: true }]),
  );
  app.get('/api/v1/sso/providers/roles', (_req, res) =>
    res.json([
      { id: 'role-owner', name: 'Owner' },
      { id: 'role-admin', name: 'Admin' },
      { id: 'role-viewer', name: 'Viewer' },
    ]),
  );
  // `/orgs` matches the real backend shape: a flat array of OrgView
  // (`src/api/http/routes/iam_directory.rs::list_orgs`). The web client
  // tolerates both `Org[]` and `{ items: Org[] }`, but tests should drive
  // the canonical shape so divergence shows up early.
  app.get('/api/v1/orgs', (_req, res) =>
    res.json([
      {
        id: 'acme-prod',
        name: 'acme-prod',
        slug: 'acme-prod',
        display_role: 'Owner',
        roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
      },
      {
        id: 'acme-staging',
        name: 'acme-staging',
        slug: 'acme-staging',
        display_role: 'Admin',
        roles: [{ id: 'role-admin', key: 'admin', name: 'Admin', builtin: true }],
      },
    ]),
  );
  app.post('/api/v1/orgs/:id/select', (req, res) =>
    res.json({
      token: `fake-jwt-after-switch:${req.params.id}`,
      user_id: 'u1',
      org_id: req.params.id,
      org_name: req.params.id,
      display_role: 'Owner',
      roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
      system: false,
    }),
  );
  app.get('/api/v1/users', (_req, res) =>
    res.json({
      items: [
        {
          id: 'u1',
          email: 'sre@example.com',
          display_role: 'Admin',
          roles: [{ id: 'role-admin', key: 'admin', name: 'Admin', builtin: true }],
        },
      ],
    }),
  );
  app.get('/api/v1/teams', (_req, res) => res.json([]));
  app.get('/api/v1/roles', (_req, res) =>
    res.json([
      {
        id: 'role-owner',
        key: 'owner',
        name: 'Owner',
        description: 'Full administrative access.',
        builtin: true,
        role_type: 'organization',
        scope: 'organization',
        permissions: MOCK_IAM_ROLE_PERMISSIONS.owner,
        usage: {
          memberships: 1,
          api_tokens: 0,
          invitations: 0,
          bindings: 1,
          total: 2,
        },
        created_at_micros: Date.parse(FROZEN_NOW_ISO) * 1000,
        updated_at_micros: Date.parse(FROZEN_NOW_ISO) * 1000,
      },
    ]),
  );
  app.get('/api/v1/iam/permissions', (_req, res) =>
    res.json(MOCK_IAM_PERMISSION_CATALOG),
  );
  app.get('/api/v1/iam/capabilities', (_req, res) =>
    res.json({
      organization_id: 'acme-prod',
      scope: 'organization',
      display_role: 'Owner',
      roles: [
        {
          id: 'role-owner',
          key: 'owner',
          name: 'Owner',
          builtin: true,
        },
      ],
      permissions: MOCK_IAM_ROLE_PERMISSIONS.owner,
      features: ['intelligence', 'domain_management', 'federated_search'],
      version: 1,
      route_catalog_version: 1,
      routes: mockCapabilityRoutes(
        'organization',
        MOCK_IAM_ROLE_PERMISSIONS.owner ?? [],
      ),
    }),
  );
  app.get('/api/v1/iam/role-bindings', (_req, res) => res.json([]));
  app.get('/api/v1/iam/cross-org-grants', (_req, res) => res.json([]));
  app.get('/api/v1/iam/share-targets', (_req, res) =>
    res.json([{ id: 'partner-prod', name: 'Partner Production' }]),
  );
  app.get('/api/v1/domains', (_req, res) =>
    res.json({ items: [{ id: 'd1', host: 'molesignal.dev', verified: true }] }),
  );
  app.get('/api/v1/scheduled-reports', (_req, res) =>
    res.json({ items: scheduledReports }),
  );
  app.get('/api/v1/scheduled_reports', (_req, res) =>
    res.json(scheduledReports),
  );
  app.get('/api/v1/scheduled_reports/:id/deliveries', (_req, res) =>
    res.json([]),
  );
  app.get('/api/v1/scheduled_reports/:id/preview', (req, res) =>
    res
      .type('application/json')
      .send(
        JSON.stringify({
          report_id: req.params.id,
          generated_at: FROZEN_NOW_ISO,
        }),
      ),
  );
  app.get('/api/v1/schedules', (_req, res) => res.json({ items: [] }));
  app.get('/api/v1/resource_shares/policy', (_req, res) =>
    res.json(resourceSharePolicy),
  );
  app.put('/api/v1/resource_shares/policy', (req, res) => {
    resourceSharePolicy = {
      ...resourceSharePolicy,
      ...req.body,
      updated_at: Date.parse(FROZEN_NOW_ISO) * 1_000,
    };
    res.json(resourceSharePolicy);
  });
  app.get('/api/v1/resource_shares', (req, res) => {
    const resourceType =
      typeof req.query.resource_type === 'string'
        ? req.query.resource_type
        : null;
    const resourceId =
      typeof req.query.resource_id === 'string'
        ? req.query.resource_id
        : null;
    res.json(
      resourceShares
        .filter(
          (share) =>
            (!resourceType || share.resource_type === resourceType) &&
            (!resourceId || share.resource_id === resourceId),
        )
        .map(({ token, password: _password, ...share }) => ({
          ...share,
          url:
            share.enabled && !share.revoked_at
              ? `/s/${token}`
              : null,
        })),
    );
  });
  app.post('/api/v1/resource_shares', (req: Request, res: Response) => {
    const password =
      typeof req.body?.password === 'string' &&
      req.body.password.trim().length > 0
        ? req.body.password.trim()
        : null;
    if (
      req.body?.share_mode === 'public_link' &&
      req.body?.resource_type === 'dashboard' &&
      password === null
    ) {
      return res.status(400).json({
        error: 'invalid argument',
        message:
          'invalid argument: public dashboard shares require a password',
      });
    }
    const createdAt = Date.parse(FROZEN_NOW_ISO) * 1_000;
    const expiresInSecs =
      typeof req.body?.expires_in_secs === 'number'
        ? req.body.expires_in_secs
        : null;
    const token = `ms${String(resourceShares.length + 1).padStart(10, '0')}`;
    const expiresAt =
      expiresInSecs === null
        ? null
        : createdAt + expiresInSecs * 1_000_000;
    const share = {
      id: `share-${resourceShares.length + 1}`,
      organization_id: 'acme-prod',
      resource_type: req.body.resource_type,
      resource_id: req.body.resource_id,
      resource_version_id: null,
      share_mode: req.body.share_mode,
      permissions:
        req.body.resource_type === 'dashboard'
          ? ['dashboards.view', 'dashboard_panels.execute']
          : ['reports.view'],
      constraints: req.body.constraints ?? {},
      expires_at: expiresAt,
      max_views: req.body.max_views ?? null,
      view_count: 0,
      allow_download: req.body.allow_download ?? false,
      enabled: true,
      cross_org_grant_id: null,
      snapshot_content_type:
        req.body.resource_type === 'report' ? 'application/pdf' : null,
      snapshot_filename:
        req.body.resource_type === 'report' ? 'weekly-slo.pdf' : null,
      created_by: 'dev',
      created_at: createdAt,
      last_accessed_at: null,
      revoked_at: null,
      password,
      token,
    };
    resourceShares.push(share);
    const {
      token: _token,
      password: _password,
      ...responseShare
    } = share;
    return res.json({
      share: { ...responseShare, url: `/s/${token}` },
      url: `/s/${token}`,
    });
  });
  app.delete('/api/v1/resource_shares/:id', (req, res) => {
    const share = resourceShares.find((item) => item.id === req.params.id);
    if (!share) return res.status(404).json({ error: 'share not found' });
    share.enabled = false;
    share.revoked_at = Date.parse(FROZEN_NOW_ISO) * 1_000;
    const {
      token: _token,
      password: _password,
      ...response
    } = share;
    return res.json({ ...response, url: null });
  });
  app.post('/api/v1/resource_shares/:id/rotate', (req, res) => {
    const share = resourceShares.find((item) => item.id === req.params.id);
    if (!share) return res.status(404).json({ error: 'share not found' });
    share.token = `rotated-${share.id}`;
    const { token, password: _password, ...response } = share;
    return res.json({
      share: { ...response, url: `/s/${token}` },
      url: `/s/${token}`,
    });
  });
  app.get('/s/:token', (req, res) => {
    const share = resourceShares.find(
      (item) => item.token === req.params.token && item.enabled,
    );
    if (!share) return res.status(404).send('resource share not found');
    if (share.share_mode === 'public_link') {
      res.cookie('molesignal_share_session', `session-${share.id}`, {
        httpOnly: true,
        sameSite: 'lax',
      });
      return res.redirect(302, '/shared');
    }
    return res.redirect(
      302,
      share.resource_type === 'dashboard'
        ? `/dashboards/${share.resource_id}`
        : `/reports?report=${share.resource_id}`,
    );
  });
  app.get('/api/v1/public/share', (req, res) => {
    const sessionId = (req.headers.cookie ?? '')
      .split(';')
      .map((value) => value.trim())
      .find((value) => value.startsWith('molesignal_share_session='))
      ?.split('=')[1];
    const share = resourceShares.find(
      (item) => sessionId === `session-${item.id}` && item.enabled,
    );
    if (!share || share.share_mode !== 'public_link') {
      return res.status(401).json({ error: 'missing share session' });
    }
    const requiresPassword =
      typeof share.password === 'string' &&
      !unlockedShareSessions.has(String(sessionId));
    if (requiresPassword) {
      return res.json({
        kind: share.resource_type,
        requires_password: true,
        expires_at_micros: share.expires_at,
      });
    }
    if (share.resource_type === 'dashboard') {
      const dashboard = dashboards.find(
        (item) => item.id === share.resource_id,
      );
      const definition = globalThis.structuredClone(
        dashboard?.model ?? {},
      ) as Record<string, unknown>;
      definition.annotations = [];
      definition.links = [];
      definition.editable = false;
      const sanitizeElements = (elements: unknown): void => {
        if (!Array.isArray(elements)) return;
        for (const element of elements) {
          if (!element || typeof element !== 'object') continue;
          const item = element as Record<string, unknown>;
          item.links = [];
          if (Array.isArray(item.queries)) {
            item.queries = item.queries.map((query) => ({
              ...(query as Record<string, unknown>),
              query: {},
            }));
          }
          sanitizeElements(item.elements);
          if (Array.isArray(item.tabs)) {
            for (const tab of item.tabs) {
              if (tab && typeof tab === 'object') {
                sanitizeElements(
                  (tab as Record<string, unknown>).elements,
                );
              }
            }
          }
        }
      };
      sanitizeElements(definition.elements);
      return res.json({
        kind: 'dashboard',
        title: dashboard?.title ?? 'Shared dashboard',
        requires_password: false,
        expires_at_micros: share.expires_at,
        constraints: share.constraints,
        definition,
        watermark: {
          share_id: share.id,
          accessed_at_micros: Date.parse(FROZEN_NOW_ISO) * 1_000,
        },
      });
    }
    return res.json({
      kind: 'report',
      title: 'Weekly SLO',
      format: 'pdf',
      requires_password: false,
      allow_download: share.allow_download,
      expires_at_micros: share.expires_at,
      content_type: 'application/pdf',
      watermark: {
        share_id: share.id,
        accessed_at_micros: Date.parse(FROZEN_NOW_ISO) * 1_000,
      },
    });
  });
  app.post('/api/v1/public/share/query', (_req, res) =>
    res.json({
      columns: ['timestamp', 'value'],
      rows: [[Date.parse(FROZEN_NOW_ISO) * 1_000, 1]],
      scanned_rows: 1,
      took_ms: 1,
    }),
  );
  app.post('/api/v1/public/share/unlock', (req, res) => {
    const sessionId = (req.headers.cookie ?? '')
      .split(';')
      .map((value) => value.trim())
      .find((value) => value.startsWith('molesignal_share_session='))
      ?.split('=')[1];
    const share = resourceShares.find(
      (item) => sessionId === `session-${item.id}` && item.enabled,
    );
    if (!share || share.share_mode !== 'public_link') {
      return res.status(401).json({ error: 'missing share session' });
    }
    if (
      typeof share.password === 'string' &&
      req.body?.password !== share.password
    ) {
      return res.status(401).json({ error: 'invalid share password' });
    }
    unlockedShareSessions.add(String(sessionId));
    return res.json({ unlocked: true });
  });

  // Pro license with all gated features unlocked, so dev:mock surfaces
  // the same UI a paying pro/SaaS deployment sees.
  const licenseSnapshot = {
    edition: 'pro',
    verified: true,
    expired: false,
    issued_to: 'dev',
    features: ['intelligence', 'domain_management', 'federated_search'],
    max_ingest_bytes_per_day: null,
    expires_at_micros: null,
    active_version_id: 'license-dev',
  };
  app.get('/api/v1/system/license', (_req, res) => res.json(licenseSnapshot));
  app.post('/api/v1/system/license/versions', (_req, res) =>
    res.status(201).json(licenseSnapshot),
  );
  const intelligenceToolsets: Array<{
    id: string;
    name: string;
    enabled: boolean;
    schema: unknown;
    updated_at_micros: number;
  }> = [];
  app.get('/api/v1/intelligence/settings/toolsets', (_req, res) =>
    res.json(intelligenceToolsets),
  );
  app.post('/api/v1/intelligence/settings/toolsets', (req: Request, res: Response) => {
    const row = {
      id: `tool-${intelligenceToolsets.length + 1}`,
      name: String(req.body?.name ?? ''),
      enabled: Boolean(req.body?.enabled ?? true),
      schema: req.body?.schema ?? {},
      updated_at_micros: Date.now() * 1000,
    };
    intelligenceToolsets.push(row);
    res.json(row);
  });
  app.delete('/api/v1/intelligence/settings/toolsets/:id', (req, res) => {
    const idx = intelligenceToolsets.findIndex((r) => r.id === req.params.id);
    if (idx >= 0) intelligenceToolsets.splice(idx, 1);
    res.json({});
  });

  // Catch-all under /api/v1 so any uncovered endpoint resolves to `{}` and
  // never hangs the app in a forever-spinner.
  app.get('/api/v1/*', (_req, res) => res.json({}));
  app.post('/api/v1/*', (_req, res) => res.json({}));
  app.put('/api/v1/*', (_req, res) => res.json({}));
  app.delete('/api/v1/*', (_req, res) => res.json({}));
}

/**
 * Wire `page.route('**\/api/v1/**')` to proxy every API call to the Express
 * server bound on `port`, install the frozen clock, and seed an explicit mock
 * login plus theme/density preferences so the app boots deterministically.
 *
 * Call from `test.beforeEach` in every behavior / perf spec.
 */
export async function mountMockRoutes(
  page: Page,
  port: number,
  opts: {
    theme?: 'dark' | 'light';
    density?: 'comfortable' | 'compact';
    role?: string;
    token?: string;
    orgId?: string;
    orgName?: string;
    scope?: 'organization' | 'system' | 'api_token';
    platformPermissions?: string[];
    capabilityPermissions?: string[];
    features?: string[];
  } = {},
): Promise<void> {
  const theme = opts.theme ?? 'light';
  const density = opts.density ?? 'comfortable';
  const auth = {
    ...MOCK_AUTH,
    state: {
      ...MOCK_AUTH.state,
      token: opts.token ?? MOCK_AUTH.state.token,
      ctx: {
        ...MOCK_AUTH.state.ctx,
        org_id: opts.orgId ?? MOCK_AUTH.state.ctx.org_id,
        org_name: opts.orgName ?? MOCK_AUTH.state.ctx.org_name,
        display_role: opts.role ?? MOCK_AUTH.state.ctx.display_role,
        roles:
          (opts.scope ?? MOCK_AUTH.state.ctx.scope) === 'system'
            ? [
                {
                  id: 'role-platform-owner',
                  key: 'platform_owner',
                  name: opts.role ?? MOCK_AUTH.state.ctx.display_role,
                  builtin: true,
                },
              ]
            : [
                {
                  id: `role-${(opts.role ?? MOCK_AUTH.state.ctx.display_role).toLowerCase().replaceAll(' ', '-')}`,
                  key: (opts.role ?? MOCK_AUTH.state.ctx.display_role)
                    .toLowerCase()
                    .replaceAll(' ', '_'),
                  name: opts.role ?? MOCK_AUTH.state.ctx.display_role,
                  builtin: Object.hasOwn(
                    MOCK_IAM_ROLE_PERMISSIONS,
                    (opts.role ?? MOCK_AUTH.state.ctx.display_role).toLowerCase(),
                  ),
                },
              ],
        scope: opts.scope ?? MOCK_AUTH.state.ctx.scope,
      },
    },
  };
  // `setFixedTime` instead of `clock.install`: visual baselines need Date.now
  // to be deterministic, but the behavior specs rely on real setTimeout +
  // requestAnimationFrame (palette debounce, NDJSON RAF batching). `install`
  // would freeze the timer queue and break both.
  await page.clock.setFixedTime(new Date(FROZEN_NOW_ISO));
  await page.addInitScript(
    ({ theme, density, auth, themeKey, densityKey, explicitKey, authKey }) => {
      localStorage.setItem(themeKey, theme);
      localStorage.setItem(densityKey, density);
      localStorage.setItem(explicitKey, '1');
      // Seed the zustand-persisted auth slice with a normal mock Bearer token
      // so RequireAuth exercises the authenticated application path.
      localStorage.setItem(authKey, JSON.stringify(auth));
    },
    {
      theme,
      density,
      auth,
      themeKey: THEME_KEY,
      densityKey: DENSITY_KEY,
      explicitKey: EXPLICIT_THEME_KEY,
      authKey: AUTH_KEY,
    },
  );
  // Resource share entry links are served outside `/api/v1`. Route them to
  // the per-test mock server and preserve the 302 to exercise the real flow.
  await page.route(/\/s\/[^/?#]+(?:\?.*)?$/, async (route) => {
    const reqUrl = new URL(route.request().url());
    const target = `http://127.0.0.1:${port}${reqUrl.pathname}${reqUrl.search}`;
    const response = await route.fetch({ url: target, maxRedirects: 0 });
    await route.fulfill({ response });
  });
  // Proxy every /api/v1/** request to the Express mock. `route.fetch` plus
  // `route.fulfill({ response })` preserves the upstream body (including
  // chunked NDJSON streams from /query/stream) without buffering through an
  // arrayBuffer round-trip.
  await page.route('**/api/v1/**', async (route) => {
    const reqUrl = new URL(route.request().url());
    if (reqUrl.pathname === '/api/v1/iam/permissions') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(MOCK_IAM_PERMISSION_CATALOG),
      });
      return;
    }
    if (reqUrl.pathname === '/api/v1/iam/capabilities') {
      const role = opts.role ?? MOCK_AUTH.state.ctx.display_role;
      const authorization = route.request().headers().authorization ?? '';
      const switchedOrg = authorization.includes('fake-jwt-after-switch:')
        ? authorization.split('fake-jwt-after-switch:')[1]
        : undefined;
      const scope: 'organization' | 'system' | 'api_token' = switchedOrg
        ? 'organization'
        : (opts.scope ?? 'organization');
      const platformPermissionMap: Record<string, string> = {
        system_telemetry_read: 'sys.telemetry.read',
        system_telemetry_manage: 'sys.telemetry.manage',
        license_read: 'sys.licenses.read',
        license_write: 'sys.licenses.manage',
        platform_admin_manage: 'sys.administrators.manage',
        trace_debug: 'sys.trace_debug.manage',
      };
      const permissions =
        opts.capabilityPermissions ??
        (scope === 'system'
          ? (opts.platformPermissions ?? []).map(
              (permission) => platformPermissionMap[permission] ?? permission,
            )
          : (MOCK_IAM_ROLE_PERMISSIONS[role.toLowerCase()] ?? []));
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          organization_id:
            switchedOrg ?? opts.orgId ?? MOCK_AUTH.state.ctx.org_id,
          scope,
          display_role: role,
          roles:
            scope === 'system'
              ? [
                  {
                    id: 'role-platform-owner',
                    key: 'platform_owner',
                    name: role,
                    builtin: true,
                  },
                ]
              : [
                  {
                    id: `role-${role.toLowerCase().replaceAll(' ', '-')}`,
                    key: role.toLowerCase().replaceAll(' ', '_'),
                    name: role,
                    builtin: Object.hasOwn(
                      MOCK_IAM_ROLE_PERMISSIONS,
                      role.toLowerCase(),
                    ),
                  },
                ],
          permissions,
          features:
            opts.features ??
            ['intelligence', 'domain_management', 'federated_search'],
          version: 1,
          route_catalog_version: 1,
          routes: mockCapabilityRoutes(scope, permissions),
        }),
      });
      return;
    }
    const target = `http://127.0.0.1:${port}${reqUrl.pathname}${reqUrl.search}`;
    try {
      const resp = await route.fetch({ url: target });
      await route.fulfill({ response: resp });
    } catch (error) {
      const message = String((error as Error).message);
      if (
        message.includes('Route is already handled') ||
        message.includes('Test ended') ||
        message.includes('Target page, context or browser has been closed')
      ) {
        return;
      }
      try {
        await route.fulfill({ status: 502, body: 'mock proxy failed' });
      } catch (fulfillError) {
        const fulfillMessage = String((fulfillError as Error).message);
        if (
          fulfillMessage.includes('Route is already handled') ||
          fulfillMessage.includes('Test ended') ||
          fulfillMessage.includes('Target page, context or browser has been closed')
        ) {
          return;
        }
        throw fulfillError;
      }
    }
  });
}

export const test = base.extend<Fixtures>({
  mockServer: async ({}, use) => {
    const app = express();
    app.use(express.json({ limit: '8mb' }));
    registerRoutes(app);

    const server: Server = await new Promise((resolve) => {
      const s = app.listen(0, '127.0.0.1', () => resolve(s));
    });
    const port = (server.address() as { port: number }).port;
    await use({ port });
    await new Promise<void>((resolve) => server.close(() => resolve()));
  },
});

export const expect = test.expect;
