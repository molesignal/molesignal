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

import { test as base, type BrowserContext, type Page } from '@playwright/test';
import express, { type Express, type Request, type Response } from 'express';

import type { UserPreferences } from '../../src/api/me';
import { PRODUCT_ROUTES } from '../../src/product/ia';
import { registerAgentCommandRoutes } from './mockAgentCommands';
import { registerAgentKnowledgeRoutes } from './mockAgentKnowledge';
import { registerAgentResourceRoutes } from './mockAgentResources';
import { registerAgentRunRoutes } from './mockAgentRuns';
import { registerAgentUsageRoutes } from './mockAgentUsage';
import { mockIncidentExists, registerAlertRoutes } from './mockAlerts';
import { registerAutomationRoutes } from './mockAutomations';
import { registerImRoutes } from './mockIm';
import { registerQualityRoutes } from './mockQuality';
import { registerRumRoutes } from './mockRum';
import { registerLogInspectionRoutes } from './mockLogInspections';
import { registerMfaRoutes } from './mockMfa';
import { registerReliabilityRoutes } from './mockReliability';
import { registerIntakeUsageRoutes } from './mockIntakeUsage';
import { registerSearchJobRoutes } from './mockSearchJobs';
import { registerSsoTestRoutes } from './mockSsoTests';

export const FROZEN_NOW_ISO = '2026-05-23T10:00:00.000Z';

export const MOCK_IAM_PLATFORM_PERMISSIONS = [
  'sys.organizations.read',
  'sys.organizations.create',
  'sys.organizations.update',
  'sys.organizations.disable',
  'sys.organizations.delete',
  'sys.settings.retention.read',
  'sys.settings.retention.manage',
  'sys.settings.limits.read',
  'sys.settings.limits.manage',
  'sys.settings.signin.read',
  'sys.settings.signin.manage',
  'sys.settings.alerting.read',
  'sys.settings.alerting.manage',
  'sys.settings.topology.read',
  'sys.settings.topology.manage',
  'sys.signing_keys.read',
  'sys.signing_keys.rotate',
  'sys.clusters.read',
  'sys.clusters.manage',
  'sys.nodes.read',
  'sys.nodes.drain',
  'sys.query.admission.read',
  'sys.query.admission.manage',
  'sys.administrators.read',
  'sys.administrators.manage',
  'sys.dashboards.read',
  'sys.telemetry.read',
  'sys.telemetry.manage',
  'sys.trace_debug.manage',
] as const;

const MOCK_IAM_ORGANIZATION_PERMISSIONS = [
  'org.preferences.read',
  'org.sharing.read',
  'org.email_domains.read',
  'org.sso.read',
  'org.signin.read',
  'org.cipher_keys.read',
  'org.field_masking.read',
  'org.regex_patterns.read',
  'org.domains.read',
  'org.running_queries.read',
  'org.preferences.manage',
  'org.sharing.manage',
  'org.email_domains.manage',
  'org.sso.manage',
  'org.signin.manage',
  'org.cipher_keys.manage',
  'org.cipher_keys.rotate',
  'org.field_masking.manage',
  'org.regex_patterns.manage',
  'org.domains.manage',
  'org.running_queries.cancel',
  'org.members.read',
  'org.members.invite',
  'org.members.update',
  'org.members.remove',
  'org.members.approve',
  'org.members.mfa_reset',
  'iam.roles.read',
  'iam.policies.read',
  'iam.roles.manage',
  'iam.roles.delete',
  'iam.policies.manage',
  'iam.policies.delete',
  'iam.grants.read',
  'iam.cross_org.read',
  'org.teams.manage',
  'iam.grants.manage',
  'iam.cross_org.manage',
  'api_tokens.read',
  'api_tokens.issue',
  'api_tokens.revoke',
  'rum.applications.manage',
  'personal_tokens.manage',
  'service_accounts.read',
  'service_accounts.manage',
  'service_accounts.delete',
  'streams.read',
  'log_patterns.read',
  'debug_artifacts.read',
  'streams.query',
  'logs.read',
  'traces.read',
  'metrics.read',
  'profiles.read',
  'query.jobs.read',
  'query.jobs.run',
  'query.slow.read',
  'apm.services.read',
  'apm.errors.read',
  'apm.error_rules.read',
  'apm.definitions.read',
  'apm.deployments.read',
  'apm.slos.read',
  'apm.settings.read',
  'rum.events.read',
  'rum.analytics.read',
  'rum.sessions.read',
  'rum.replay.read',
  'rum.applications.read',
  'rum.issues.read',
  'rum.playlists.read',
  'logs.write',
  'traces.write',
  'metrics.write',
  'profiles.write',
  'rum.write',
  'streams.create',
  'streams.configure',
  'log_patterns.manage',
  'debug_artifacts.manage',
  'streams.delete',
  'dashboards.read',
  'dashboards.folders.read',
  'dashboards.annotations.read',
  'dashboards.edit',
  'dashboards.folders.manage',
  'dashboards.annotations.manage',
  'dashboards.create',
  'dashboards.delete',
  'dashboards.folders.delete',
  'dashboards.share',
  'alerts.rules.read',
  'alerts.routing.read',
  'alerts.escalations.read',
  'alerts.calendars.read',
  'alerts.importance.read',
  'alerts.incidents.read',
  'alerts.mutes.read',
  'alerts.inhibitions.read',
  'alerts.remediation.read',
  'alerts.response.read',
  'alerts.semantic_groups.read',
  'alerts.sources.read',
  'alerts.tickets.read',
  'alerts.discovery.read',
  'notify.connectors.read',
  'notify.policies.read',
  'notify.templates.read',
  'notify.defaults.read',
  'notify.deliveries.read',
  'alerts.rules.manage',
  'alerts.rules.delete',
  'alerts.routing.manage',
  'alerts.escalations.manage',
  'alerts.escalations.delete',
  'alerts.calendars.manage',
  'alerts.importance.manage',
  'alerts.incidents.rca',
  'alerts.remediation.manage',
  'alerts.response.manage',
  'alerts.semantic_groups.manage',
  'alerts.sources.manage',
  'alerts.tickets.manage',
  'notify.connectors.manage',
  'notify.connectors.delete',
  'notify.policies.manage',
  'notify.policies.delete',
  'notify.templates.manage',
  'notify.defaults.manage',
  'notify.deliveries.retry',
  'alerts.incidents.create',
  'alerts.incidents.acknowledge',
  'alerts.incidents.update',
  'alerts.incidents.escalate',
  'alerts.incidents.remediate',
  'alerts.incidents.tickets',
  'notify.deliveries.acknowledge',
  'alerts.incidents.silence',
  'alerts.mutes.manage',
  'alerts.inhibitions.manage',
  'schedules.read',
  'schedules.swaps.request',
  'schedules.calendar_feeds.manage',
  'schedules.create',
  'schedules.update',
  'schedules.delete',
  'schedules.overrides.manage',
  'schedules.swaps.approve',
  'saved_views.read',
  'saved_views.create',
  'saved_views.edit',
  'saved_views.delete',
  'pipelines.read',
  'connectors.read',
  'pipelines.create',
  'pipelines.edit',
  'connectors.manage',
  'pipelines.run',
  'pipelines.pause',
  'pipelines.delete',
  'functions.read',
  'tables.read',
  'functions.create',
  'tables.create',
  'functions.edit',
  'tables.edit',
  'functions.run',
  'functions.delete',
  'tables.delete',
  'reports.read',
  'reports.create',
  'reports.edit',
  'reports.schedule',
  'reports.share',
  'reports.delete',
  'audit.read',
  'agent.use',
  'inspections.read',
  'agent.approvals.read',
  'agent.approvals.request',
  'agent.automations.read',
  'agent.investigations.read',
  'agent.investigations.manage',
  'agent.knowledge.read',
  'agent.knowledge.manage',
  'agent.memories.read',
  'agent.memories.manage',
  'agent.mcp_servers.read',
  'agent.mcp_servers.manage',
  'agent.skills.read',
  'agent.skills.manage',
  'agent.skill_repositories.read',
  'agent.skill_repositories.manage',
  'agent.tools.read',
  'agent.models.read',
  'agent.profiles.read',
  'agent.prompts.read',
  'agent.toolsets.read',
  'agent.usage.read',
  'agent.manage',
  'agent.memories.settings',
  'agent.skills.rescan',
  'agent.tools.manage',
  'agent.models.manage',
  'agent.models.rotate',
  'agent.profiles.manage',
  'agent.prompts.manage',
  'agent.toolsets.manage',
  'agent.usage.manage',
  'agent.quality.read',
  'agent.quality.manage',
  'agent.network_policy.read',
  'agent.network_policy.manage',
  'agent.inbound_mcp.read',
  'agent.inbound_mcp.manage',
  'agent.im.read',
  'agent.im.manage',
  'agent.approve',
  'inspections.manage',
  'inspections.run',
  'agent.automations.manage',
  'agent.automations.run',
  'agent.automations.tokens',
  'status_pages.read',
  'status_pages.automation.read',
  'status_pages.create',
  'status_pages.update',
  'status_pages.archive',
  'status_pages.delete',
  'status_pages.access.manage',
  'status_pages.domain.manage',
  'status_pages.slack.manage',
  'status_pages.subscribers.manage',
  'status_pages.automation.manage',
  'status_pages.events.manage',
  'apm.errors.manage',
  'apm.error_rules.manage',
  'apm.definitions.manage',
  'apm.slos.manage',
  'apm.slos.delete',
  'apm.settings.manage',
  'apm.deploy',
  'intake_usage.read',
  'intake_budgets.manage',
  'rum.issues.manage',
  'rum.playlists.manage',
  'rum.replay.comments.write',
  'rum.replay.comments.manage',
  'rum.replay.summaries.generate',
  'status_pages.automation.approve',
  'status_pages.events.publish',
  'synthetics.monitors.read',
  'synthetics.results.read',
  'synthetics.locations.read',
  'synthetics.secrets.read',
  'synthetics.monitors.manage',
  'synthetics.monitors.run',
  'synthetics.monitors.archive',
  'synthetics.locations.manage',
  'synthetics.secrets.manage',
  'service_levels.read',
  'service_levels.manage',
  'postmortems.read',
  'postmortems.manage',
] as const;

/** The permission each platform page asks for, as the backend's route catalog has it. */
const MOCK_SYSTEM_ROUTE_PERMISSIONS: Record<string, readonly string[]> = {
  'iam.organizations': ['sys.organizations.read'],
  'settings.organization.management': ['sys.organizations.read'],
  'settings.client_ip': ['sys.settings.signin.read'],
  'settings.nodes': ['sys.clusters.read', 'sys.settings.topology.read'],
  'settings.signin_requirements': ['sys.settings.signin.read'],
  'settings.signin_limits': ['sys.settings.signin.read'],
  'settings.audit_retention': ['sys.settings.retention.read'],
  'settings.stream_retention': ['sys.settings.retention.read'],
  'settings.series_limits': ['sys.settings.limits.read'],
  'settings.intake_usage': ['sys.settings.limits.read'],
  'settings.alert_evaluation': ['sys.settings.alerting.read'],
};

const MOCK_PLATFORM_READS = [
  'sys.organizations.read',
  'sys.administrators.read',
  'sys.settings.retention.read',
  'sys.settings.limits.read',
  'sys.settings.signin.read',
  'sys.settings.alerting.read',
  'sys.settings.topology.read',
  'sys.clusters.read',
  'sys.nodes.read',
  'sys.query.admission.read',
] as const;

/** The pages of a module with permissions of their own, as the backend's route catalog has them. */
const MOCK_MODULE_PAGES: ReadonlyArray<readonly [string, string, readonly string[]]> = [
  ['alerts', '/alerts', [
    'alerts.incidents.read',
    'alerts.rules.read',
    'alerts.discovery.read',
    'alerts.sources.read',
    'postmortems.read',
    'alerts.mutes.read',
    'alerts.inhibitions.read',
    'alerts.routing.read',
    'alerts.escalations.read',
    'schedules.read',
    'alerts.remediation.read',
    'alerts.tickets.read',
    'alerts.semantic_groups.manage',
  ]],
  ['alert.incidents', '/alerts/incidents', ['alerts.incidents.read']],
  ['alert.rules', '/alerts/rules', ['alerts.rules.read']],
  ['alert.insights', '/alerts/insights', ['alerts.incidents.read']],
  ['alert.history', '/alerts/history', ['alerts.incidents.read']],
  ['alert.discovery', '/alerts/discovery', ['alerts.discovery.read']],
  ['alert.sources', '/alerts/sources', ['alerts.sources.read']],
  ['alerts.postmortems', '/alerts/postmortems', ['postmortems.read']],
  ['alert.silences', '/alerts/silences', ['alerts.mutes.read']],
  ['alert.inhibitions', '/alerts/inhibitions', ['alerts.inhibitions.read']],
  ['alert.routing', '/alerts/routing', ['alerts.routing.read']],
  ['alert.escalations', '/alerts/escalations', ['alerts.escalations.read']],
  ['alert.schedules', '/alerts/schedules', ['schedules.read']],
  ['alert.remediation', '/alerts/remediation', ['alerts.remediation.read']],
  ['alert.tickets', '/alerts/tickets', ['alerts.tickets.read']],
  ['semantic.groups', '/alerts/semantic-groups', ['alerts.semantic_groups.manage']],
  ['apm', '/apm', ['apm.services.read', 'apm.errors.read', 'apm.slos.read', 'apm.settings.read', 'sys.telemetry.read']],
  ['apm.overview', '/apm/overview', ['apm.services.read', 'sys.telemetry.read']],
  ['apm.services', '/apm/services', ['apm.services.read', 'sys.telemetry.read']],
  ['apm.map', '/apm/map', ['apm.services.read', 'sys.telemetry.read']],
  ['apm.errors', '/apm/errors', ['apm.errors.read', 'sys.telemetry.read']],
  ['apm.slos', '/apm/slos', ['apm.slos.read', 'sys.telemetry.read']],
  ['apm.settings', '/apm/settings', ['apm.settings.read', 'sys.telemetry.read']],
  ['rum', '/rum', ['rum.events.read', 'rum.playlists.read', 'rum.issues.read', 'rum.analytics.read', 'rum.applications.read']],
  ['rum.overview', '/rum/overview', ['rum.events.read']],
  ['rum.explorer', '/rum/explorer', ['rum.events.read']],
  ['rum.playlists', '/rum/playlists', ['rum.playlists.read']],
  ['rum.errors', '/rum/errors', ['rum.issues.read']],
  ['rum.performance', '/rum/performance', ['rum.events.read']],
  ['rum.frustration', '/rum/frustration', ['rum.events.read']],
  ['rum.analytics', '/rum/analytics', ['rum.analytics.read']],
  ['rum.feature.flags', '/rum/feature-flags', ['rum.events.read']],
  ['rum.applications', '/rum/applications', ['rum.applications.read']],
  ['rum.settings', '/rum/settings/:section', ['rum.applications.read']],
  ['rum.source.maps', '/rum/settings/source-maps', ['debug_artifacts.read']],
  ['synthetics', '/synthetics', ['synthetics.monitors.read', 'synthetics.results.read', 'synthetics.locations.read', 'synthetics.secrets.read']],
  ['synthetics.overview', '/synthetics/overview', ['synthetics.monitors.read']],
  ['synthetics.checks', '/synthetics/checks', ['synthetics.monitors.read']],
  ['synthetics.results', '/synthetics/results', ['synthetics.results.read']],
  ['synthetics.locations', '/synthetics/locations', ['synthetics.locations.read']],
  ['synthetics.agents', '/synthetics/agents', ['synthetics.locations.read']],
  ['synthetics.variables', '/synthetics/variables', ['synthetics.secrets.read']],
];

function mockCapabilityRoutes(
  scope: 'organization' | 'system' | 'api_token',
  permissions: readonly string[],
) {
  const permissionSet = new Set(permissions);
  const systemOnly = new Set([
    'iam.organizations',
    'settings.organization.management',
    'settings.client_ip',
    'settings.nodes',
    'settings.signin_requirements',
    'settings.signin_limits',
    'settings.audit_retention',
    'settings.stream_retention',
    'settings.series_limits',
    'settings.intake_usage',
    'settings.alert_evaluation',
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
        (systemTelemetryOwners.has(route.owner) &&
          permissionSet.has('sys.telemetry.read')) ||
        (route.owner === 'dashboards' &&
          permissionSet.has('sys.dashboards.read')) ||
        (route.id in MOCK_SYSTEM_ROUTE_PERMISSIONS &&
          (MOCK_SYSTEM_ROUTE_PERMISSIONS[route.id] ?? []).some((permission) =>
            permissionSet.has(permission),
          ));
    }
    return {
      id: route.id,
      pathPattern: route.path,
      allowed,
      navigationGroup: navigable ? route.group : undefined,
      navigationPosition: navigable ? index : undefined,
    };
  });
  // The permission each route asks for, as the backend's route catalog has it
  // (`20260101000003_iam_route_catalog.sql`).
  const explicit = [
    ['root', '/', true],
    ['account.settings', '/account/settings/*', true],
    ['settings', '/settings', scope === 'system'
      ? MOCK_PLATFORM_READS.some((permission) => permissionSet.has(permission))
      : [
          'org.preferences.read',
          'org.sharing.read',
          'org.cipher_keys.read',
          'org.regex_patterns.read',
          'org.field_masking.read',
          'org.domains.read',
          'org.running_queries.read',
          'org.signin.read',
          'audit.read',
          'notify.connectors.read',
          'notify.policies.read',
          'notify.templates.read',
          'notify.defaults.read',
          'notify.deliveries.read',
          'alerts.calendars.read',
          'alerts.importance.read',
        ].some((permission) => permissionSet.has(permission))],
    ['iam', '/iam', scope === 'system'
      ? permissionSet.has('sys.organizations.read') || permissionSet.has('sys.administrators.read')
      : permissionSet.has('org.members.read') ||
        permissionSet.has('iam.roles.read') ||
        permissionSet.has('iam.policies.read') ||
        permissionSet.has('iam.grants.read') ||
        permissionSet.has('api_tokens.read') ||
        permissionSet.has('service_accounts.read') ||
        permissionSet.has('org.sso.read') ||
        permissionSet.has('org.email_domains.read')],
    ['stream.usage', '/streams/usage', permissionSet.has('intake_usage.read') || permissionSet.has('sys.telemetry.read')],
    ['iam.users', '/iam/users', scope !== 'system' && permissionSet.has('org.members.read')],
    ['iam.invitations', '/iam/invitations', scope !== 'system' && permissionSet.has('org.members.read')],
    ['iam.approvals', '/iam/approvals', scope !== 'system' && permissionSet.has('org.members.read')],
    ['iam.teams', '/iam/teams', scope !== 'system' && permissionSet.has('org.members.read')],
    ['iam.roles', '/iam/roles', permissionSet.has(scope === 'system' ? 'sys.administrators.read' : 'iam.roles.read')],
    ['iam.permission.policies', '/iam/policies', permissionSet.has(scope === 'system' ? 'sys.administrators.read' : 'iam.policies.read')],
    ['iam.groups', '/iam/groups', permissionSet.has(scope === 'system' ? 'sys.administrators.read' : 'iam.grants.read')],
    ['iam.api.tokens', '/iam/api-tokens', scope !== 'system' && permissionSet.has('api_tokens.read')],
    ['iam.service.accounts', '/iam/service-accounts', scope !== 'system' && permissionSet.has('service_accounts.read')],
    ['iam.sso', '/iam/sso', scope !== 'system' && permissionSet.has('org.sso.read')],
    ['iam.sso.test', '/iam/sso/tests/:id', scope !== 'system' && permissionSet.has('org.sso.manage')],
    ['iam.email.domains', '/iam/email-domains', scope !== 'system' && permissionSet.has('org.email_domains.read')],
    ['settings.general', '/settings/general', scope !== 'system' && (permissionSet.has('org.preferences.read') || permissionSet.has('org.sharing.read'))],
    ['settings.notify', '/settings/notify/*', scope !== 'system' && [
      'notify.connectors.read',
      'notify.policies.read',
      'notify.templates.read',
      'notify.defaults.read',
      'notify.deliveries.read',
      'org.members.read',
    ].some((permission) => permissionSet.has(permission))],
    ['logs.inspections', '/logs/inspections', scope !== 'system' && permissionSet.has('inspections.read') && permissionSet.has('logs.read')],
    ['logs.inspection.detail', '/logs/inspections/:id', scope !== 'system' && permissionSet.has('inspections.read') && permissionSet.has('logs.read')],
    ['settings.tenant.tools', '/settings/:section', scope !== 'system' && [
      'org.cipher_keys.read',
      'org.regex_patterns.read',
      'org.field_masking.read',
      'org.domains.read',
      'org.running_queries.read',
    ].some((permission) => permissionSet.has(permission))],
    ...MOCK_MODULE_PAGES.map(([id, path, any]) => [
      id,
      path,
      any.some((permission) => permissionSet.has(permission)),
    ] as const),
  ] as const;
  // A route listed here is decided here. The app takes the first decision it
  // finds for a route, so the row derived from the product catalog above must
  // not answer for it as well.
  const explicitAllowed = new Map<string, boolean>(
    explicit.map(([id, , allowed]) => [id, allowed]),
  );
  const catalogIds = new Set<string>(decisions.map((decision) => decision.id));
  return [
    ...decisions.map((decision) => ({
      ...decision,
      allowed: explicitAllowed.get(decision.id) ?? decision.allowed,
    })),
    ...explicit
      .filter(([id]) => !catalogIds.has(id))
      .map(([id, pathPattern, allowed]) => ({
        id,
        pathPattern: pathPattern,
        allowed,
      })),
  ];
}

const MOCK_IAM_VIEWER_PERMISSIONS = [
  'streams.read',
  'log_patterns.read',
  'debug_artifacts.read',
  'streams.query',
  'logs.read',
  'traces.read',
  'metrics.read',
  'profiles.read',
  'query.jobs.read',
  'query.jobs.run',
  'query.slow.read',
  'apm.services.read',
  'apm.errors.read',
  'apm.error_rules.read',
  'apm.definitions.read',
  'apm.deployments.read',
  'apm.slos.read',
  'apm.settings.read',
  'rum.events.read',
  'rum.analytics.read',
  'rum.sessions.read',
  'rum.replay.read',
  'rum.applications.read',
  'rum.issues.read',
  'rum.playlists.read',
  'dashboards.read',
  'dashboards.folders.read',
  'dashboards.annotations.read',
  'alerts.rules.read',
  'alerts.routing.read',
  'alerts.escalations.read',
  'alerts.calendars.read',
  'alerts.importance.read',
  'alerts.incidents.read',
  'alerts.mutes.read',
  'alerts.inhibitions.read',
  'alerts.remediation.read',
  'alerts.response.read',
  'alerts.semantic_groups.read',
  'alerts.sources.read',
  'alerts.tickets.read',
  'alerts.discovery.read',
  'notify.connectors.read',
  'notify.policies.read',
  'notify.templates.read',
  'notify.defaults.read',
  'notify.deliveries.read',
  'schedules.read',
  'schedules.swaps.request',
  'schedules.calendar_feeds.manage',
  'saved_views.read',
  'pipelines.read',
  'connectors.read',
  'functions.read',
  'tables.read',
  'reports.read',
  'agent.use',
  'inspections.read',
  'agent.approvals.read',
  'agent.approvals.request',
  'agent.automations.read',
  'agent.investigations.read',
  'agent.investigations.manage',
  'agent.knowledge.read',
  'agent.knowledge.manage',
  'agent.memories.read',
  'agent.memories.manage',
  'agent.mcp_servers.read',
  'agent.mcp_servers.manage',
  'agent.skills.read',
  'agent.skills.manage',
  'agent.skill_repositories.read',
  'agent.skill_repositories.manage',
  'agent.tools.read',
  'agent.models.read',
  'agent.profiles.read',
  'agent.prompts.read',
  'agent.toolsets.read',
  'agent.usage.read',
  'personal_tokens.manage',
] as const;

const MOCK_IAM_EDITOR_DENY_PREFIXES = [
  'org.',
  'iam.',
  'api_tokens.',
  'audit.',
  'agent.manage',
  'agent.approve',
  'agent.memories.settings',
  'agent.skills.rescan',
  'agent.tools.manage',
  'agent.models.',
  'agent.profiles.manage',
  'agent.prompts.manage',
  'agent.toolsets.manage',
  'agent.usage.manage',
  'agent.quality.',
  'agent.network_policy.',
  'agent.inbound_mcp.',
  'agent.im.',
] as const;

const MOCK_IAM_ROLE_PERMISSIONS: Record<string, readonly string[]> = {
  platform_admin: MOCK_IAM_PLATFORM_PERMISSIONS,
  // An owner holds the organization's whole catalog and nothing of the platform's.
  owner: [...MOCK_IAM_ORGANIZATION_PERMISSIONS, 'org.delete'],
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
  if (
    resource === 'iam' ||
    resource === 'api_tokens' ||
    resource === 'personal_tokens' ||
    resource === 'service_accounts' ||
    resource === 'audit'
  ) {
    return 'iam';
  }
  if (resource === 'dashboards') return 'dashboards';
  if (resource === 'alerts' || resource === 'schedules' || resource === 'notify') return 'alerts';
  if (
    resource === 'pipelines' ||
    resource === 'functions' ||
    resource === 'connectors' ||
    resource === 'tables'
  ) {
    return 'pipelines';
  }
  if (resource === 'reports') return 'reports';
  if (resource === 'agent' || resource === 'inspections') return 'agent';
  if (
    resource === 'status_pages' ||
    resource === 'synthetics' ||
    resource === 'service_levels' ||
    resource === 'postmortems'
  ) {
    return resource;
  }
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
      labelKey: `permissions.${translationKey}`,
      descriptionKey: `permissions_hint.${translationKey}`,
      builtinRoles: Object.entries(MOCK_IAM_ROLE_PERMISSIONS)
        .filter(([, permissions]) => permissions.includes(permission))
        .map(([role]) => role),
      ...(permission.startsWith('agent.')
        ? { feature: 'agent' }
        : {}),
    };
  }),
};

/** Whether a policy action names `permission`, by name or by a pattern with `*`. */
const mockActionMatches = (action: string, permission: string): boolean => {
  const parts = action.split('*');
  if (parts.length === 1) return action === permission;
  const first = parts[0] ?? '';
  const last = parts[parts.length - 1] ?? '';
  if (!permission.startsWith(first)) return false;
  let rest = permission.slice(first.length);
  for (const part of parts.slice(1, -1)) {
    const at = rest.indexOf(part);
    if (at < 0) return false;
    rest = rest.slice(at + part.length);
  }
  return rest.length >= last.length && rest.endsWith(last);
};

/** The organization permissions a policy document allows, or why it allows none. */
const mockResolvePolicy = (document: unknown): { permissions: string[] } | { error: string } => {
  const statements = (document as { Statement?: unknown } | null)?.Statement;
  if ((document as { Version?: unknown } | null)?.Version !== '1') return { error: 'Version must be "1"' };
  if (!Array.isArray(statements)) return { error: 'invalid policy document: Statement is not a list' };
  const allowed = new Set<string>();
  for (const [index, statement] of statements.entries()) {
    const { Effect, Action } = statement as { Effect?: unknown; Action?: unknown };
    if (Effect !== 'Allow') return { error: `Statement[${index}].Effect must be "Allow"` };
    const actions = typeof Action === 'string' ? [Action] : Array.isArray(Action) ? Action : [];
    for (const [position, raw] of actions.entries()) {
      const action = String(raw).trim().toLowerCase();
      const matched = MOCK_IAM_ORGANIZATION_PERMISSIONS.filter((permission) =>
        mockActionMatches(action, permission),
      );
      if (matched.length === 0) {
        return { error: `Statement[${index}].Action[${position}]: no permission is called \`${action}\`` };
      }
      matched.forEach((permission) => allowed.add(permission));
    }
  }
  if (allowed.size === 0) return { error: 'a policy must allow at least one permission' };
  return { permissions: [...allowed].sort() };
};

const mockBuiltinPolicy = (key: string, name: string, permissions: readonly string[]) => ({
  id: `builtin:${key}`,
  key,
  name,
  description: '',
  builtin: true,
  document: { Version: '1', Statement: [{ Effect: 'Allow', Action: [...permissions] }] },
  permissions: [...permissions].sort(),
  createdBy: null as string | null,
  createdAt: Date.parse('2026-01-01T00:00:00Z') * 1000,
  updatedAt: Date.parse('2026-01-01T00:00:00Z') * 1000,
});

const HERE = dirname(fileURLToPath(import.meta.url));
const DATA = join(HERE, 'data');

const readJson = <T = unknown>(name: string): T =>
  JSON.parse(readFileSync(join(DATA, name), 'utf8')) as T;
const readText = (name: string): string => readFileSync(join(DATA, name), 'utf8');

// Eagerly load fixtures at module init so request handlers stay synchronous.
const SEARCH = readJson('search.json');
const TOPOLOGY = readJson('topology.json');
const TRACE = readJson<{ traceId: string; rootSpanId: string; spans: unknown[]; truncated: boolean }>('trace.json');
const STREAMS = readJson('streams.json');
const DASHBOARDS = readJson('dashboards.json');
const LOG_NDJSON = readText('log-stream.ndjson');

const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';
const EXPLICIT_THEME_KEY = 'molesignal-theme-explicit';
const AUTH_KEY = 'molesignal-auth';
/**
 * The personal preferences the mock user starts with. `mountMockRoutes` seeds
 * the browser with the same look, so the two cannot drift apart.
 */
const MOCK_USER_PREFERENCES: UserPreferences = {
  theme: 'system',
  density: 'normal',
  language: 'en-us',
  defaultHomeRoute: '/home',
  timeFormat: 'iso_24h',
  dateFormat: 'yyyy_mm_dd_dash',
  timezone: '',
  keyboardShortcutsEnabled: true,
};
const MOCK_AUTH = {
  state: {
    ctx: {
      userId: 'dev',
      orgId: 'acme-prod',
      orgName: 'acme-prod',
      displayRole: 'Owner',
      roles: [
        {
          id: 'role-owner',
          key: 'owner',
          name: 'Owner',
          builtin: true,
        },
      ],
      scope: 'organization',
      displayName: 'Dev User',
      email: 'dev@molesignal.local',
    },
  },
  version: 1,
};

/** The identity the page keeps, as `/auth/session` answers it. */
function sessionView(ctx: typeof MOCK_AUTH.state.ctx) {
  return {
    userId: ctx.userId,
    email: ctx.email,
    displayName: ctx.displayName,
    orgId: ctx.orgId,
    orgName: ctx.orgName,
    displayRole: ctx.displayRole,
    roles: ctx.roles,
    scope: ctx.scope,
  };
}

interface Fixtures {
  mockServer: { port: number };
}

/** What the session cookies of each browser context hold. */
const BROWSER_COOKIES = new WeakMap<BrowserContext, { session: object | null }>();

export function registerRoutes(app: Express): void {
  const agentUsage = registerAgentUsageRoutes(app, FROZEN_NOW_ISO);
  const agentCommands = registerAgentCommandRoutes(app, mockIncidentExists);
  registerAlertRoutes(app, FROZEN_NOW_ISO, () => agentUsage.refusal());
  registerAgentKnowledgeRoutes(app, FROZEN_NOW_ISO);
  registerReliabilityRoutes(app, FROZEN_NOW_ISO);
  registerSearchJobRoutes(app, FROZEN_NOW_ISO);
  registerIntakeUsageRoutes(app, FROZEN_NOW_ISO);
  registerRumRoutes(app, FROZEN_NOW_ISO);
  registerLogInspectionRoutes(app, FROZEN_NOW_ISO);
  const dashboards = (DASHBOARDS as { items: Array<Record<string, unknown>> }).items.map((item) => ({ ...item }));
  const folders: Array<{ id: string; orgId: string; name: string; parentId?: string }> = [];
  const resourceShares: Array<Record<string, unknown> & { token: string }> = [];
  const unlockedShareSessions = new Set<string>();
  let resourceSharePolicy = {
    organizationId: 'acme-prod',
    allowPublicLinks: true,
    allowPublicDashboards: true,
    maxPublicExpirySecs: 7 * 24 * 60 * 60,
    requirePublicReportPassword: false,
    denyProductionPublicShares: false,
    allowPublicCsvDownload: false,
    updatedBy: 'dev',
    updatedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
  };
  const scheduledReports: Array<Record<string, unknown>> = [
    {
      id: 'r1',
      orgId: 'acme-prod',
      name: 'Weekly SLO',
      description: 'Core service availability and latency review.',
      dashboardId: 'd1',
      savedViewId: null,
      cron: 'every:7d',
      recipients: [{ kind: 'email', target: 'sre@example.com' }],
      format: 'pdf',
      timeRangeJson: {
        preset: 'previous-7-days',
        timezone: 'UTC',
        description: 'Core service availability and latency review.',
      },
      enabled: true,
      // Last saved by Dana Reyes (`/api/v1/users`), not by the signed-in mock user. As the
      // backend does, the report names her itself: reading it does not take the member list.
      runAsUserId: 'u1',
      runAsUser: { id: 'u1', displayName: 'Dana Reyes', email: 'sre@example.com' },
      lastRunAt: Date.parse(FROZEN_NOW_ISO) * 1_000 - 86_400_000_000,
      createdAt: Date.parse(FROZEN_NOW_ISO) * 1_000 - 604_800_000_000,
      updatedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
    },
  ];
  const chatMessages = new Map<string, Array<Record<string, unknown>>>();
  const agentChats: Array<Record<string, unknown>> = [];
  let userPreferences = { ...MOCK_USER_PREFERENCES };
  let hasPersonalPreferences = false;
  let workspacePreferenceDefaults = { ...MOCK_USER_PREFERENCES };
  let meProfile = {
    userId: 'dev',
    email: 'dev@molesignal.local',
    username: 'Dev User',
    displayName: 'Dev User',
    avatarUrl: null as string | null,
    bio: '',
    orgId: 'acme-prod',
    orgName: 'acme-prod',
    orgSlug: 'acme-prod',
    displayRole: 'Owner',
    createdAt: 1_753_584_000_000_000,
  };

  const signinResponse = {
    userId: 'dev',
    email: 'dev@molesignal.local',
    displayName: 'Dev User',
    orgId: 'acme-prod',
    orgName: 'acme-prod',
    displayRole: 'Owner',
    roles: [
      {
        id: 'role-owner',
        key: 'owner',
        name: 'Owner',
        builtin: true,
      },
    ],
    scope: 'organization',
  };

  registerMfaRoutes(app, signinResponse);
  app.post('/api/v1/auth/signup/complete', (_req, res) =>
    res.json({ status: 'active', ...signinResponse }),
  );
  const ssoTests = registerSsoTestRoutes(app);
  app.post('/api/v1/auth/forgot-password', (_req, res) =>
    res.status(202).json({ accepted: true }),
  );
  app.get('/api/v1/me/profile', (_req, res) => res.json(meProfile));
  app.put('/api/v1/me/profile', (req, res) => {
    meProfile = { ...meProfile, ...req.body };
    if (req.body.avatarUrl === '') meProfile.avatarUrl = null;
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
  // How the mock user prefers a page to look, by purpose and key. Kept for as long as the
  // server lives, so a reload finds what the page before it saved.
  const uiPreference = new Map<string, { value: unknown; updatedAt: number }>();
  const uiPreferenceId = (req: { params: Record<string, string> }) => `${req.params.scope}/${req.params.key}`;
  app.get('/api/v1/me/ui-preference/:scope/:key', (req, res) =>
    res.json(uiPreference.get(uiPreferenceId(req)) ?? { value: null, updatedAt: null }),
  );
  app.put('/api/v1/me/ui-preference/:scope/:key', (req, res) => {
    const entry = { value: req.body?.value ?? null, updatedAt: Date.now() * 1_000 };
    uiPreference.set(uiPreferenceId(req), entry);
    res.json(entry);
  });
  app.delete('/api/v1/me/ui-preference/:scope/:key', (req, res) => {
    uiPreference.delete(uiPreferenceId(req));
    res.status(204).end();
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
      externalUrl: '',
      signupEnabled: false,
      signupVerifiesEmail: false,
      passkeySignin: false,
      version: '26.0.0.0',
    }),
  );
  let signinLimits = {
    accountSourceFailures: 5,
    sourceFailures: 30,
    accountFailures: 20,
    accountIntervalSecs: 60,
    lockMinutes: 15,
  };
  const signinLimitsView = () => ({
    ...signinLimits,
    source: { address: '8.8.8.8', counted: true, behindUnconfiguredProxy: false },
  });
  app.get('/api/v1/settings/signin_limits', (_req, res) => res.json(signinLimitsView()));
  app.put('/api/v1/settings/signin_limits', (req, res) => {
    signinLimits = { ...signinLimits, ...req.body };
    res.json(signinLimitsView());
  });
  let clientIpSettings = {
    mode: 'peer',
    headerName: '',
    trustedProxyCidrs: [] as string[],
    fallbackToPeer: true,
    allowPrivateClientIps: false,
    maxChainLength: 16,
  };
  app.get('/api/v1/settings/client_ip', (_req, res) =>
    res.json(clientIpSettings),
  );
  app.put('/api/v1/settings/client_ip', (req, res) => {
    clientIpSettings = { ...clientIpSettings, ...req.body };
    res.json(clientIpSettings);
  });
  // Retention, with what a shorter one would delete: figures per day of data, so the days cut
  // decide the amount. A day or two stays small; 30 days to 7 is past what asks for the days typed.
  const retention = { stream: 30, synthetic: 30, audit: 90 };
  const DAY_MICROS = 86_400_000_000;
  const RETENTION_STREAMS = [
    { orgId: 'acme-prod', orgName: 'Acme Production', id: 's-app', name: 'app-logs', streamType: 'logs', rows: 40_000, storedBytes: 60 * 1024 ** 2 },
    { orgId: 'acme-prod', orgName: 'Acme Production', id: 's-api', name: 'api-traces', streamType: 'traces', rows: 9_000, storedBytes: 12 * 1024 ** 2 },
    { orgId: 'acme-prod', orgName: 'Acme Production', id: 's-host', name: 'host-metrics', streamType: 'metrics', rows: 1_500, storedBytes: 2 * 1024 ** 2 },
  ];
  const AUDIT_ROWS_PER_DAY = 40;
  app.get('/api/v1/settings/stream_retention', (_req, res) =>
    res.json({ retentionDays: retention.stream }),
  );
  app.put('/api/v1/settings/stream_retention', (req, res) => {
    retention.stream = Number(req.body.retentionDays);
    res.json({ retentionDays: retention.stream });
  });
  app.get('/api/v1/settings/stream_retention/preview', (req, res) => {
    const days = Number(req.query.retentionDays);
    const cut = Math.max(0, retention.stream - days);
    const now = Date.now() * 1000;
    const earliest = cut > 0 ? now - retention.stream * DAY_MICROS : null;
    const streams = cut === 0 ? [] : RETENTION_STREAMS.map((stream) => ({
      ...stream,
      rows: stream.rows * cut,
      storedBytes: stream.storedBytes * cut,
      earliestAt: earliest,
    }));
    res.json({
      retentionDays: days,
      previousRetentionDays: retention.stream,
      estimated: true,
      generatedAt: now,
      rows: streams.reduce((total, stream) => total + stream.rows, 0),
      storedBytes: streams.reduce((total, stream) => total + stream.storedBytes, 0),
      earliestAt: earliest,
      streamCount: streams.length,
      streams,
      unavailableStreams: 0,
    });
  });
  app.get('/api/v1/settings/synthetic_retention', (_req, res) =>
    res.json({ retentionDays: retention.synthetic }),
  );
  app.put('/api/v1/settings/synthetic_retention', (req, res) => {
    retention.synthetic = Number(req.body.retentionDays);
    res.json({ retentionDays: retention.synthetic });
  });
  app.get('/api/v1/settings/audit_retention', (_req, res) =>
    res.json({ retentionDays: retention.audit }),
  );
  app.put('/api/v1/settings/audit_retention', (req, res) => {
    retention.audit = Number(req.body.retentionDays);
    res.json({ retentionDays: retention.audit });
  });
  app.get('/api/v1/settings/audit_retention/preview', (req, res) => {
    const days = Number(req.query.retentionDays);
    // 0 keeps for good: moving to it deletes nothing, and from it, everything older than the days.
    const kept = retention.audit === 0 ? 365 : retention.audit;
    const cut = days === 0 ? 0 : Math.max(0, kept - days);
    res.json({
      retentionDays: days,
      previousRetentionDays: retention.audit,
      rows: cut * AUDIT_ROWS_PER_DAY,
      earliestAt: cut > 0 ? Date.now() * 1000 - kept * DAY_MICROS : null,
    });
  });
  app.get('/api/v1/version', (_req, res) =>
    res.json({
      version: '26.0.0.0',
      commit: 'd86fa2d15d68',
      branch: 'main',
      buildEpochSecs: 1_785_087_406,
      buildId: 'gha-12345-1',
      releaseChannel: 'stable',
      edition: 'enterprise',
    }),
  );

  // ── web/* (search, topology, trace, correlation, investigation blob) ──
  app.get('/api/v1/web/search', (_req, res) => res.json(SEARCH));
  app.get('/api/v1/web/topology', (_req, res) => res.json(TOPOLOGY));
  app.get('/api/v1/web/trace/:id', (req, res) => res.json({ ...TRACE, traceId: req.params.id }));
  app.get('/api/v1/web/correlation/:from/:to', (_req, res) =>
    res.json({
      timeRange: { from: FROZEN_NOW_ISO, to: FROZEN_NOW_ISO },
      filters: [],
      prefill: {},
    }),
  );
  const investigationBlobs = new Map<string, unknown>();
  app.post('/api/v1/web/investigation/blob', (req: Request, res: Response) => {
    const id = `blob-${investigationBlobs.size + 1}`;
    investigationBlobs.set(id, req.body);
    res.json({ blobId: id });
  });
  app.get('/api/v1/web/investigation/blob/:id', (req, res) => {
    const blob = investigationBlobs.get(req.params.id);
    if (!blob) return res.status(404).json({ error: 'not found' });
    return res.json(blob);
  });

  // ── streams / dashboards / saved_views ──
  app.get('/api/v1/streams', (_req, res) => res.json(STREAMS));
  // No masking rule exists, so no field of any stream is masked.
  app.get('/api/v1/field_masking/effective/:streamId', (req, res) =>
    res.json({ streamId: req.params.streamId, fields: [] }),
  );
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
      orgId: 'acme-prod',
      folderId: typeof req.body?.folderId === 'string' ? req.body.folderId : undefined,
      uid,
      title: typeof model.title === 'string' && model.title ? model.title : 'Untitled dashboard',
      tags: Array.isArray(model.tags) ? model.tags : [],
      model,
      version: 1,
      createdAt: nowMicros,
      updatedAt: nowMicros,
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
    dashboard.updatedAt = Date.parse(FROZEN_NOW_ISO) * 1000;
    dashboard.folderId =
      typeof req.body?.folderId === 'string'
        ? req.body.folderId
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
    const parentId = typeof req.body?.parentId === 'string' && req.body.parentId.trim()
      ? req.body.parentId.trim()
      : undefined;
    if (parentId && !folders.some((folder) => folder.id === parentId)) {
      return res.status(404).json({ error: `parent folder ${parentId} not found` });
    }
    const folder = {
      id: `folder-${folders.length + 1}`,
      orgId: 'acme-prod',
      name,
      ...(parentId ? { parentId: parentId } : {}),
    };
    folders.push(folder);
    return res.json(folder);
  });
  app.put('/api/v1/folders/:id', (req: Request, res: Response) => {
    const folder = folders.find((item) => item.id === req.params.id);
    if (!folder) return res.status(404).json({ error: 'not found' });
    const name = String(req.body?.name ?? '').trim();
    if (!name) return res.status(400).json({ error: 'folder name must not be empty' });
    const parentId = typeof req.body?.parentId === 'string' && req.body.parentId.trim()
      ? req.body.parentId.trim()
      : undefined;
    if (parentId === folder.id) return res.status(400).json({ error: 'folder cannot be its own parent' });
    if (parentId && !folders.some((item) => item.id === parentId)) {
      return res.status(404).json({ error: `parent folder ${parentId} not found` });
    }
    folder.name = name;
    if (parentId) folder.parentId = parentId;
    else delete folder.parentId;
    return res.json(folder);
  });
  app.delete('/api/v1/folders/:id', (req, res) => {
    const index = folders.findIndex((folder) => folder.id === req.params.id);
    if (index < 0) return res.status(404).json({ error: 'not found' });
    const hasChild = folders.some((folder) => folder.parentId === req.params.id);
    const hasDashboard = dashboards.some((dashboard) => dashboard.folderId === req.params.id);
    if (hasChild || hasDashboard) {
      return res.status(409).json({ error: 'folder is not empty: move or delete its dashboards and sub-folders first' });
    }
    folders.splice(index, 1);
    return res.json({ deleted: req.params.id });
  });
  // The shape the backend returns (`SavedView`): a view without its language or
  // statement takes the Saved views page down, which is not what a mock is for.
  const savedViews = [
    {
      id: 'sv1',
      orgId: 'acme-prod',
      ownerUserId: 'test-user',
      name: 'Yesterday errors',
      language: 'sql',
      statement: "SELECT * FROM \"app_logs\" WHERE level = 'ERROR'",
      timeRangeSecs: 86_400,
      stream: 'app_logs',
      tags: ['errors'],
      pinned: true,
      createdAt: Date.parse(FROZEN_NOW_ISO) * 1000 - 7 * 86_400_000_000,
      updatedAt: Date.parse(FROZEN_NOW_ISO) * 1000 - 86_400_000_000,
    },
  ];
  app.get('/api/v1/saved-views', (_req, res) => res.json({ items: savedViews }));
  app.get('/api/v1/saved_views', (_req, res) => res.json(savedViews));
  app.get('/api/v1/saved_views/:id', (req, res) => {
    const view = savedViews.find((candidate) => candidate.id === req.params.id);
    return view ? res.json(view) : res.status(404).json({ error: `saved view ${req.params.id} not found` });
  });

  // ── alerts (rules / incidents / escalations) ──

  // ── pipelines: one saved pipeline, its runs, and what its editor reads ──
  // Without these the catch-all answers `{}` where the pages expect lists and
  // they fall to the router's error screen, which is then what gets scanned.
  app.get('/api/v1/scheduled_pipelines', (_req, res) => res.json([]));
  app.get('/api/v1/scheduled_pipelines/:id/runs', (req, res) => {
    const finished = Date.parse(FROZEN_NOW_ISO) * 1000 - 120_000_000;
    res.json([
      {
        id: 'run-2',
        pipelineId: req.params.id,
        state: 'succeeded',
        startedAt: finished - 4_000_000,
        finishedAt: finished,
        scannedRows: 18_420,
        writtenRows: 18_200,
        droppedRows: 214,
        failedRows: 6,
        attempt: 1,
        error: null,
        warning: '6 event(s) failed: step `normalize-level` run: function call error',
      },
      {
        id: 'run-1',
        pipelineId: req.params.id,
        state: 'failed',
        startedAt: finished - 304_000_000,
        finishedAt: finished - 300_000_000,
        scannedRows: 0,
        writtenRows: 0,
        droppedRows: 0,
        failedRows: 0,
        attempt: 2,
        error: '`logs`: read: unavailable: storage unavailable',
        warning: null,
      },
    ]);
  });
  // What a realtime pipeline's quotas and throttles counted today: nothing yet, unless a spec says.
  app.get('/api/v1/scheduled_pipelines/:id/limits', (_req, res) =>
    res.json({ dayStart: Date.parse(FROZEN_NOW_ISO.slice(0, 10)) * 1000, buckets: [] }),
  );
  app.get('/api/v1/scheduled_pipelines/:id', (req, res) =>
    res.json({
      id: req.params.id,
      name: 'checkout-logs',
      sourceStream: 'logs',
      targetStream: 'access_logs',
      cron: 'every:5m',
      lookbackSecs: 300,
      enabled: true,
      readUntil: { logs: Date.parse(FROZEN_NOW_ISO) * 1000 - 60_000_000 },
      retry: null,
      functionSteps: {
        mode: 'scheduled',
        signalType: 'logs',
        sources: ['logs'],
        sinks: ['access_logs'],
        sinkConnectors: [],
        delaySecs: 60,
        retry: { runRetries: 3, deliveryRetries: 3 },
        steps: [
          {
            transformName: 'normalize-level',
            kind: 'vrl',
            script: 'if is_string(.level) {\n  .level = downcase(string!(.level))\n}',
          },
        ],
      },
    }),
  );
  app.get('/api/v1/functions', (_req, res) => res.json([]));
  app.get('/api/v1/connectors', (_req, res) => res.json([]));

  // ── Notify settings ──
  app.get('/api/v1/notify/connector-types', (_req, res) => res.json([]));
  app.get('/api/v1/notify/connectors', (_req, res) => res.json([]));
  app.get('/api/v1/notify/users', (_req, res) => res.json([]));
  // What one person is reached by: nothing bound yet, no preference saved.
  app.get('/api/v1/users/:userId/notify-endpoints', (_req, res) => res.json([]));
  app.get('/api/v1/users/:userId/notify-preferences', (_req, res) => res.json([]));
  app.get('/api/v1/notify/recipient-resolver-types', (_req, res) =>
    res.json(['fixed_users', 'team_members', 'oncall_schedule']),
  );
  app.get('/api/v1/notify/policies', (_req, res) => res.json([]));
  app.get('/api/v1/notify/templates', (_req, res) => res.json([]));
  app.get('/api/v1/notify/deliveries', (_req, res) => res.json([]));
  app.get('/api/v1/notify/organization-defaults', (_req, res) => res.json([]));
  app.get('/api/v1/notify/team-defaults/:teamId', (_req, res) => res.json([]));

  // ── Mole Agent chat + audit ──
  app.get('/api/v1/agent/chat', (_req, res) => res.json(agentChats));
  app.post('/api/v1/agent/chat', (req, res) => {
    const now = Date.now() * 1000;
    const id = `mock-chat-${now}`;
    chatMessages.set(id, []);
    const chat = {
      id,
      provider: req.body?.provider ?? 'openai',
      model: req.body?.model ?? 'gpt-4o',
      title: req.body?.title ?? 'Mole Agent 对话',
      // The first question for now: the first answer names the chat (see mockAgentRuns).
      titleProvisional: Boolean(req.body?.titleProvisional),
      providerId: req.body?.providerId ?? null,
      analysisMode: req.body?.analysisMode ?? null,
      timeRangeStart: null,
      timeRangeEnd: null,
      archiveObjectKey: null,
      scope: req.body?.teamId ? 'team' : 'personal',
      teamId: req.body?.teamId ?? null,
      mountedTeamIds: [],
      ownerId: 'dev',
      readable: true,
      // Started from an incident: the chat records an investigation of it.
      subjectKind: req.body?.subject?.kind ?? null,
      subjectId: req.body?.subject?.id ?? null,
      investigationId: req.body?.subject ? `mock-investigation-${now}` : null,
      createdAt: now,
      updatedAt: now,
    };
    agentChats.unshift(chat);
    res.json(chat);
  });
  // A rename: the chat keeps its place in the list, and is no longer named by its first answer.
  app.patch('/api/v1/agent/chat/:id', (req, res) => {
    const chat = agentChats.find((item) => item.id === req.params.id);
    const title = String(req.body?.title ?? '').trim();
    if (!chat) return res.status(404).json({ error: 'not_found', message: 'chat not found' });
    if (!title) return res.status(400).json({ error: 'invalid_argument', message: 'a name is needed' });
    Object.assign(chat, { title, titleProvisional: false });
    return res.json(chat);
  });
  // A fork: a new chat with every message of its source, under it in the list.
  app.post('/api/v1/agent/chat/:id/fork', (req, res) => {
    const source = agentChats.find((chat) => chat.id === req.params.id);
    if (!source) {
      res.status(404).json({ error: 'not_found', message: 'chat not found' });
      return;
    }
    const now = Date.now() * 1000;
    const id = `mock-chat-${now}`;
    chatMessages.set(
      id,
      (chatMessages.get(req.params.id) ?? []).map((message, index) => ({
        ...message,
        id: `${id}-m${index}`,
        chatId: id,
        runId: null,
      })),
    );
    const fork = {
      ...source,
      id,
      parentChatId: source.id,
      subjectKind: null,
      subjectId: null,
      investigationId: null,
      ownerId: 'dev',
      createdAt: now,
      updatedAt: now,
    };
    agentChats.unshift(fork);
    res.json(fork);
  });
  app.get('/api/v1/agent/chat/:id/messages', (req, res) =>
    res.json({ messages: chatMessages.get(req.params.id) ?? [] }),
  );
  // A message is kept and queued as a run; its answer is read from the run's events.
  registerAgentRunRoutes(app, {
    chatMessages,
    agentChats,
    readCommand: (content) => agentCommands.read(content),
    budgetRefusal: () => agentUsage.refusal(),
    budgetNotice: () => agentUsage.notice(),
    userId: 'dev',
  });
  app.post('/api/v1/agent/chat/:id/archive', (req, res) =>
    res.json({
      status: 'ok',
      objectKey: `agent/chat/default/${req.params.id}/transcript.json`,
    }),
  );
  app.delete('/api/v1/agent/chat/:id', (req, res) => {
    chatMessages.delete(req.params.id);
    const index = agentChats.findIndex((item) => item.id === req.params.id);
    if (index >= 0) agentChats.splice(index, 1);
    res.json({});
  });
  app.get('/api/v1/agent/audit/chat/:id', (req, res) => {
    const now = Date.now() * 1000;
    const messages = req.params.id === 'audit-chat-seeded'
      ? [
          {
            id: 'audit-chat-user-1',
            chatId: 'audit-chat-seeded',
            orgId: 'default',
            role: 'user',
            content: '过去一小时发生了什么变化？',
            createdAt: now - 2_000_000,
          },
          {
            id: 'audit-chat-assistant-1',
            chatId: 'audit-chat-seeded',
            orgId: 'default',
            role: 'assistant',
            content: '**摘要**\n\n- `checkout` 错误率上升\n- p95 延迟升高\n\n建议先查看最近部署。',
            promptBuiltinKey: 'analysis.anomaly',
            promptVersion: 1,
            promptHash: 'mock-hash',
            evidenceJson: [{ tool: 'query_logs', status: 'success', rowCount: 12 }],
            promptTokens: 128,
            completionTokens: 64,
            cost: { amount: 0.0012, currency: 'USD' },
            createdAt: now - 1_000_000,
          },
        ]
      : (chatMessages.get(req.params.id) ?? []);
    res.json({
      chat: {
        id: req.params.id,
        provider: 'openai',
        model: 'gpt-4o',
        title: 'Audit chat transcript',
        providerId: 'openai-gpt-4o',
        analysisMode: 'anomaly_analysis',
        timeRangeStart: now - 3_600_000_000,
        timeRangeEnd: now,
        archiveObjectKey: `agent/chat/default/${req.params.id}/transcript.json`,
        deletedAt: null,
        createdAt: now - 3_000_000,
        updatedAt: now - 500_000,
      },
      messages,
    });
  });
  app.get('/api/v1/audit', (_req, res) =>
    res.json({
      items: [
        {
          id: 'audit-agent-chat-1',
          orgId: 'default',
          actorKind: 'user',
          actorId: 'dev',
          action: 'agent.chat.archived',
          targetKind: 'agent_chat',
          targetId: 'audit-chat-seeded',
          ip: '127.0.0.1',
          userAgent: 'mock',
          payload: {
            status: 'ok',
            objectKey: 'agent/chat/default/audit-chat-seeded/transcript.json',
            chatId: 'audit-chat-seeded',
          },
          ts: Date.now() * 1000,
        },
      ],
      nextCursor: null,
    }),
  );
  const modelProviders: Array<Record<string, unknown>> = [
    {
      id: 'openai-gpt-4o',
      provider: 'openai',
      name: 'OpenAI Production',
      baseUrl: null,
      defaultModel: 'gpt-4o',
      enabled: true,
      timeoutMs: 30_000,
      maxTokens: 4096,
      priceCurrency: 'USD',
      inputPricePerMtok: 2.5,
      cachedInputPricePerMtok: 1.25,
      outputPricePerMtok: 10,
      keyLast4: '1234',
      keySet: true,
      createdAt: Date.now() * 1000,
      updatedAt: Date.now() * 1000,
    },
    {
      id: 'compatible-qwen',
      provider: 'openai_compatible',
      name: 'Internal Qwen',
      baseUrl: 'https://llm.internal.example/v1',
      defaultModel: 'qwen2.5-72b-instruct',
      enabled: true,
      timeoutMs: 30_000,
      maxTokens: 8192,
      priceCurrency: 'CNY',
      keyLast4: '5678',
      keySet: true,
      createdAt: Date.now() * 1000,
      updatedAt: Date.now() * 1000,
    },
  ];
  app.get('/api/v1/agent/settings/model-providers', (_req, res) =>
    res.json(modelProviders),
  );
  // The organization's currencies: the common ones, a symbol of its own on CNY.
  const currencies: Array<{ code: string; name: string | null; symbol: string | null }> = [
    'USD', 'CNY', 'EUR', 'GBP', 'JPY', 'HKD', 'SGD', 'KRW',
  ].map((code) => ({ code, name: null, symbol: null }));
  const currencyView = (currency: (typeof currencies)[number]) => ({
    ...currency,
    usedBy: modelProviders
      .filter((provider) => (provider.priceCurrency ?? 'USD') === currency.code)
      .map((provider) => String(provider.name)),
    updatedAt: Date.now() * 1000,
  });
  app.get('/api/v1/agent/settings/currencies', (_req, res) =>
    res.json({ currencies: currencies.map(currencyView) }),
  );
  app.put('/api/v1/agent/settings/currencies/:code', (req, res) => {
    const code = String(req.params.code);
    const next = {
      code,
      name: (req.body?.name as string | null) ?? null,
      symbol: (req.body?.symbol as string | null) ?? null,
    };
    const index = currencies.findIndex((currency) => currency.code === code);
    if (index >= 0) currencies[index] = next;
    else currencies.push(next);
    res.json(currencyView(next));
  });
  app.delete('/api/v1/agent/settings/currencies/:code', (req, res) => {
    const index = currencies.findIndex((currency) => currency.code === req.params.code);
    if (index >= 0) currencies.splice(index, 1);
    res.json({ deleted: index >= 0 });
  });
  app.post('/api/v1/agent/settings/model-providers', (req, res) => {
    const provider = {
      priceCurrency: 'USD',
      ...req.body,
      id: `model-provider-${modelProviders.length + 1}`,
      keyLast4:
        typeof req.body?.apiKey === 'string' ? req.body.apiKey.slice(-4) : null,
      keySet: Boolean(req.body?.apiKey),
      createdAt: Date.now() * 1000,
      updatedAt: Date.now() * 1000,
    };
    delete provider.apiKey;
    modelProviders.push(provider);
    res.json(provider);
  });
  app.put('/api/v1/agent/settings/model-providers/:id', (req, res) => {
    const provider = modelProviders.find((item) => item.id === req.params.id);
    if (!provider) return res.status(404).json({ error: 'not found' });
    Object.assign(provider, req.body, { updatedAt: Date.now() * 1000 });
    return res.json(provider);
  });
  app.post(
    '/api/v1/agent/settings/model-providers/:id/rotate-key',
    (req, res) => {
      const provider = modelProviders.find((item) => item.id === req.params.id);
      if (!provider) return res.status(404).json({ error: 'not found' });
      Object.assign(provider, {
        keyLast4:
          typeof req.body?.apiKey === 'string' ? req.body.apiKey.slice(-4) : null,
        keySet: Boolean(req.body?.apiKey),
        updatedAt: Date.now() * 1000,
      });
      return res.json(provider);
    },
  );
  const promptTemplates: Array<Record<string, unknown>> = [
    {
      id: 'prompt-system-default',
      orgId: null,
      userId: null,
      scope: 'builtin',
      builtinKey: 'system.default',
      purpose: 'system',
      name: 'Mole Agent system instruction',
      body:
        'You are Mole Agent for {{ org_name }}. Use authorized evidence and respect approval boundaries. Current time: {{ current_time }}.',
      variablesSchema: {
        type: 'object',
        properties: {
          org_name: { type: 'string' },
          current_time: { type: 'string' },
        },
      },
      isDefault: true,
      enabled: true,
      version: 1,
      parentId: null,
      createdAt: Date.now() * 1000,
      updatedAt: Date.now() * 1000,
    },
    {
      id: 'prompt-root-cause-default',
      orgId: null,
      userId: null,
      scope: 'builtin',
      builtinKey: 'root_cause.default',
      purpose: 'root_cause',
      name: 'Root-cause investigation',
      body:
        'Investigate the selected streams {{ streams }} over {{ time_range }} and report evidence.',
      variablesSchema: {
        type: 'object',
        properties: {
          streams: { type: 'string' },
          timeRange: { type: 'string' },
        },
      },
      isDefault: true,
      enabled: true,
      version: 1,
      parentId: null,
      createdAt: Date.now() * 1000,
      updatedAt: Date.now() * 1000,
    },
  ];
  app.get('/api/v1/agent/settings/prompts', (_req, res) =>
    res.json(promptTemplates),
  );
  app.post('/api/v1/agent/settings/prompts', (req, res) => {
    const now = Date.now() * 1000;
    const prompt = {
      ...req.body,
      id: `prompt-${promptTemplates.length + 1}`,
      orgId: 'acme-prod',
      userId: req.body?.scope === 'user' ? 'root' : null,
      isDefault: false,
      enabled: req.body?.enabled ?? true,
      version: 1,
      createdAt: now,
      updatedAt: now,
    };
    promptTemplates.push(prompt);
    return res.json(prompt);
  });
  app.put('/api/v1/agent/settings/prompts/:id', (req, res) => {
    const prompt = promptTemplates.find((item) => item.id === req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not found' });
    if (prompt.scope === 'builtin') {
      return res.status(400).json({ error: 'builtin prompts are immutable' });
    }
    Object.assign(prompt, req.body, {
      version: Number(prompt.version ?? 1) + 1,
      updatedAt: Date.now() * 1000,
    });
    return res.json(prompt);
  });
  app.post(
    '/api/v1/agent/settings/prompts/:id/set-default',
    (req, res) => {
      const prompt = promptTemplates.find((item) => item.id === req.params.id);
      if (!prompt) return res.status(404).json({ error: 'not found' });
      for (const candidate of promptTemplates) {
        if (
          candidate.scope === prompt.scope &&
          candidate.purpose === prompt.purpose
        ) {
          candidate.isDefault = false;
        }
      }
      prompt.isDefault = true;
      prompt.enabled = true;
      prompt.updatedAt = Date.now() * 1000;
      return res.json(prompt);
    },
  );
  app.post('/api/v1/agent/settings/prompts/:id/restore', (req, res) => {
    const prompt = promptTemplates.find((item) => item.id === req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not found' });
    const builtin = promptTemplates.find(
      (item) =>
        item.scope === 'builtin' &&
        item.builtinKey === prompt.builtinKey,
    );
    if (!builtin) return res.status(400).json({ error: 'no builtin parent' });
    Object.assign(prompt, {
      body: builtin.body,
      variablesSchema: builtin.variablesSchema,
      version: Number(prompt.version ?? 1) + 1,
      updatedAt: Date.now() * 1000,
    });
    return res.json(prompt);
  });
  app.delete('/api/v1/agent/settings/prompts/:id', (req, res) => {
    const index = promptTemplates.findIndex((item) => item.id === req.params.id);
    if (index === -1) return res.status(404).json({ error: 'not found' });
    promptTemplates.splice(index, 1);
    return res.json({ deleted: true });
  });

  // ── Mole Agent control plane ──
  const nowMicros = Date.parse(FROZEN_NOW_ISO) * 1000;
  let inboundMcpSettings = {
    orgId: 'acme-prod',
    enabled: true,
    allowedOrigins: [] as string[],
    maxRequestSize: 1,
    maxResponseSize: 1,
    maxConcurrentCalls: 8,
    callsPerMinute: 60,
    readTimeoutMs: 30_000,
    updatedBy: 'dev',
    createdAt: nowMicros,
    updatedAt: nowMicros,
  };
  const inboundMcpSettingsResponse = () => ({
    settings: inboundMcpSettings,
    endpoint: 'https://molesignal.example/api/v1/mcp',
    protocolVersions: ['2026-07-28', '2025-11-25', '2025-06-18', '2025-03-26'],
    transports: ['streamable_http'],
    hardMaxSize: 8,
    oauth: {
      protectedResourceMetadata: 'https://molesignal.example/.well-known/oauth-protected-resource/api/v1/mcp',
      authorizationServerMetadata: 'https://molesignal.example/.well-known/oauth-authorization-server',
      authorizationEndpoint: 'https://molesignal.example/oauth/authorize',
      tokenEndpoint: 'https://molesignal.example/api/v1/oauth/token',
      registrationEndpoint: 'https://molesignal.example/api/v1/oauth/register',
      revocationEndpoint: 'https://molesignal.example/api/v1/oauth/revoke',
    },
  });
  app.get('/api/v1/agent/settings/inbound-mcp', (_req, res) =>
    res.json(inboundMcpSettingsResponse()),
  );
  app.put('/api/v1/agent/settings/inbound-mcp', (req, res) => {
    inboundMcpSettings = {
      ...inboundMcpSettings,
      ...req.body,
      updatedBy: 'dev',
      updatedAt: nowMicros,
    };
    res.json(inboundMcpSettingsResponse());
  });
  app.get('/api/v1/agent/settings/inbound-mcp/oauth-connections', (_req, res) =>
    res.json({ connections: [] }),
  );
  app.post('/api/v1/agent/settings/inbound-mcp/oauth-connections/:id/revoke', (_req, res) =>
    res.status(204).end(),
  );
  app.get('/api/v1/oauth/authorization-request', (req, res) =>
    res.json({
      client: {
        client_id: String(req.query.client_id ?? 'mock-client'),
        client_name: 'Example MCP Client',
        client_uri: 'https://client.example',
        metadata_document: false,
      },
      scope: String(req.query.scope ?? 'mcp'),
      resource: String(req.query.resource ?? 'https://molesignal.example/api/v1/mcp'),
      redirect_uri: String(req.query.redirect_uri ?? 'https://client.example/callback'),
      state: typeof req.query.state === 'string' ? req.query.state : null,
      organization: {
        id: 'acme-prod',
        name: 'Acme Production',
      },
    }),
  );
  app.post('/api/v1/oauth/authorize', (_req, res) =>
    res.json({ redirect_to: 'https://client.example/callback?code=mock' }),
  );
  const investigations: Array<Record<string, unknown>> = [
    {
      id: 'investigation-checkout',
      orgId: 'acme-prod',
      createdBy: 'dev',
      chatId: null,
      title: 'checkout-api 错误率升高调查',
      status: 'running',
      context: { service: 'checkout-api', environment: 'production' },
      summary: '错误率在最近一次发布后升高。',
      confidence: 'medium',
      currentStep: '关联异常链路',
      startedAt: nowMicros - 1_800_000_000,
      completedAt: null,
      createdAt: nowMicros - 2_400_000_000,
      updatedAt: nowMicros - 300_000_000,
    },
  ];
  const agentProfiles: Array<Record<string, unknown>> = [
    {
      id: 'profile-production',
      name: '生产环境受限 Agent',
      description: '生产环境只读分析与审批后处置配置',
      modelProviderId: 'compatible-qwen',
      model: null,
      allowedTools: [
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
      dataScope: {
        environments: ['production'],
        services: [],
        streams: [],
        cross_organization: false,
      },
      riskPolicy: {
        l0: 'automatic',
        l1: 'automatic',
        l2: 'approval',
        l3: 'two_person_approval',
      },
      networkAccess: 'blocked',
      maxContextTokens: 32_000,
      maxInvestigationSecs: 1_800,
      maxToolCalls: 32,
      isDefault: true,
      enabled: true,
      createdBy: 'dev',
      createdAt: nowMicros - 7_200_000_000,
      updatedAt: nowMicros - 900_000_000,
    },
  ];
  app.get('/api/v1/agent/overview', (_req, res) =>
    res.json({
      activeInvestigations: 0,
      pendingApprovals: 0,
      recentCompleted: 0,
      automationRuns: 0,
      enabledAutomations: 0,
    }),
  );
  app.get('/api/v1/agent/investigations', (_req, res) =>
    res.json({ investigations }),
  );
  app.post('/api/v1/agent/investigations', (req, res) => {
    const investigation = {
      id: `investigation-${investigations.length + 1}`,
      orgId: 'acme-prod',
      createdBy: 'dev',
      chatId: null,
      status: 'draft',
      summary: null,
      confidence: null,
      currentStep: null,
      startedAt: null,
      completedAt: null,
      createdAt: nowMicros,
      updatedAt: nowMicros,
      ...req.body,
    };
    investigations.push(investigation);
    res.json(investigation);
  });
  app.get('/api/v1/agent/investigations/:id', (req, res) => {
    const investigation = investigations.find((item) => item.id === req.params.id);
    if (!investigation) return res.status(404).json({ error: 'not found' });
    return res.json({
      investigation,
      steps: [],
      evidence: [],
      hypotheses: [],
    });
  });
  app.put('/api/v1/agent/investigations/:id', (req, res) => {
    const investigation = investigations.find((item) => item.id === req.params.id);
    if (!investigation) return res.status(404).json({ error: 'not found' });
    Object.assign(investigation, req.body, { updatedAt: nowMicros });
    return res.json(investigation);
  });
  registerAutomationRoutes(app, nowMicros);
  registerImRoutes(app, nowMicros);
  registerQualityRoutes(app, nowMicros, (id) => promptTemplates.find((item) => item.id === id));
  app.get('/api/v1/agent/approvals', (_req, res) =>
    res.json({
      approvals: [
        {
          id: 'approval-checkout-alert',
          investigationId: null,
          action: '确认告警事件',
          target: 'production / checkout-api / high-error-rate',
          parameters: { incidentId: 'incident-checkout-500' },
          reason: 'checkout-api 错误率持续高于阈值，值班工程师已完成初步确认。',
          impact: '仅更新告警确认状态，不会修改服务配置或工作负载。',
          risk: 'l2',
          status: 'pending',
          requestedBy: 'dev',
          requiredApprovals: 1,
          reviews: [],
          expiresAt: 1_769_164_800_000_000,
          decidedAt: null,
          createdAt: 1_769_161_200_000_000,
          updatedAt: 1_769_161_200_000_000,
        },
      ],
    }),
  );
  app.get('/api/v1/agent/executions', (_req, res) =>
    res.json({ executions: [] }),
  );
  app.get('/api/v1/agent/settings/agent-profiles', (_req, res) =>
    res.json({ profiles: agentProfiles }),
  );
  app.post('/api/v1/agent/settings/agent-profiles', (req, res) => {
    const profile = {
      id: `profile-${agentProfiles.length + 1}`,
      createdBy: 'dev',
      createdAt: nowMicros,
      updatedAt: nowMicros,
      ...req.body,
    };
    agentProfiles.push(profile);
    res.json(profile);
  });
  app.put('/api/v1/agent/settings/agent-profiles/:id', (req, res) => {
    const profile = agentProfiles.find((item) => item.id === req.params.id);
    if (!profile) return res.status(404).json({ error: 'not found' });
    Object.assign(profile, req.body, { updatedAt: nowMicros });
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
    displayName: name,
    description,
    technicalDescription: description,
    domain,
    category,
    source: { kind: 'builtin', label: 'MoleSignal' },
    inputSchema: inputSchema,
    outputSchema: { type: 'object' },
    risk,
    minimumRisk: risk,
    executionMode: risk === 'l0' ? 'automatic' : 'single_approval',
    enabled: true,
    availableToAgent: true,
    status: 'healthy',
    capabilities: {
      readOnly: access === 'read_only',
      supportsDryRun: true,
      idempotent: access === 'read_only',
      streaming: false,
    },
    limits: {
      timeoutMs: 30_000,
      maxCallsPerRun: 32,
      maxResponseBytes: 1_048_576,
    },
    environmentOverrides: {},
    tags: [category],
    statistics: {
      calls24h: name === 'query_logs' ? 184 : 0,
      successRate: name === 'query_logs' ? 99.4 : null,
      p95Ms: name === 'query_logs' ? 412 : null,
      lastCalledAt: name === 'query_logs' ? nowMicros - 120_000_000 : null,
      lastError: null,
    },
    access,
  });
  const agentTools: Array<Record<string, unknown>> = [
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
    orgId: 'acme-prod',
    riskModes: {
      l0: 'automatic',
      l1: 'confirmation',
      l2: 'single_approval',
      l3: 'dual_approval',
      l4: 'dual_approval',
    },
    environmentOverrides: {
      production: {
        l0: 'automatic',
        l1: 'confirmation',
        l2: 'single_approval',
        l3: 'dual_approval',
        l4: 'dual_approval',
      },
    },
    updatedBy: 'dev',
    createdAt: nowMicros,
    updatedAt: nowMicros,
  };
  const toolDependencies = (name: string) => {
    const dependent = name === 'query_logs';
    return {
      toolName: name,
      total: dependent ? 1 : 0,
      agentProfiles: dependent
        ? [{ id: 'profile-production', name: '生产环境受限 Agent', enabled: true, isDefault: true }]
        : [],
      investigationTemplates: [],
    };
  };
  const findTool = (name: string) =>
    agentTools.find((tool) => tool.id === name || tool.name === name);
  const mcpServers: Array<Record<string, unknown>> = [
    {
      id: 'mcp-observability',
      orgId: 'acme-prod',
      scope: { scope: 'org' },
      handle: 'internal_observability',
      name: 'internal-observability',
      transport: 'streamable_http',
      memberCredential: null,
      headers: [
        { name: 'x-tenant', secret: false, value: 'acme', set: true },
        { name: 'x-api-key', secret: true, last4: '9c1d', set: true },
      ],
      env: [],
      editable: true,
      moderatable: true,
      promptCount: 0,
      myCredential: null,
      endpointUrl: 'https://mcp.internal.example/v1',
      commandTemplate: null,
      authType: 'bearer_token',
      authHeader: null,
      credentialLast4: '7f2a',
      credentialSet: true,
      privateOnly: true,
      allowedDomains: ['mcp.internal.example'],
      allowedCidrs: ['10.0.0.0/8'],
      followRedirects: false,
      tlsVerify: true,
      timeoutMs: 10_000,
      maxResponseBytes: 1_048_576,
      enabled: true,
      status: 'healthy',
      lastError: null,
      lastTestedAt: nowMicros - 300_000_000,
      lastSyncedAt: nowMicros - 600_000_000,
      createdBy: 'dev',
      createdAt: nowMicros - 86_400_000_000,
      updatedAt: nowMicros - 600_000_000,
      toolCount: 0,
    },
    {
      id: 'mcp-linear',
      orgId: 'acme-prod',
      scope: { scope: 'team', teamId: 'team-platform' },
      handle: 'linear',
      name: 'Linear',
      transport: 'sse',
      endpointUrl: 'https://mcp.linear.app/sse',
      commandTemplate: null,
      authType: 'member_secret',
      authHeader: null,
      credentialLast4: null,
      credentialSet: false,
      memberCredential: {
        header: 'authorization',
        bearer: true,
        instructions: 'Create a personal API key under Linear → Settings → API.',
      },
      headers: [],
      env: [],
      privateOnly: false,
      allowedDomains: [],
      allowedCidrs: [],
      followRedirects: false,
      tlsVerify: true,
      timeoutMs: 10_000,
      maxResponseBytes: 1_048_576,
      enabled: true,
      status: 'healthy',
      lastError: null,
      lastTestedAt: nowMicros - 300_000_000,
      lastSyncedAt: nowMicros - 600_000_000,
      createdBy: 'dev',
      createdAt: nowMicros - 86_400_000_000,
      updatedAt: nowMicros - 600_000_000,
      toolCount: 0,
      promptCount: 1,
      editable: true,
      moderatable: true,
      myCredential: null,
    },
  ];
  registerAgentResourceRoutes(app, FROZEN_NOW_ISO, mcpServers);

  app.get('/api/v1/agent/tools', (_req, res) =>
    res.json({
      tools: agentTools,
      dynamicHttp: false,
      shell: false,
      browser: false,
      openMcp: true,
      mcpServers: { total: mcpServers.length, healthy: 1, unhealthy: 0 },
    }),
  );
  app.get('/api/v1/agent/tools/policies', (_req, res) =>
    res.json(toolPolicyDefaults),
  );
  app.put('/api/v1/agent/tools/policies', (req, res) => {
    Object.assign(toolPolicyDefaults, req.body, {
      updatedBy: 'dev',
      updatedAt: nowMicros,
    });
    res.json(toolPolicyDefaults);
  });
  app.get('/api/v1/agent/tools/:id/dependencies', (req, res) =>
    res.json(toolDependencies(req.params.id)),
  );
  app.get('/api/v1/agent/tools/:id/calls', (req, res) =>
    res.json({
      calls:
        req.params.id === 'query_logs'
          ? [
              {
                id: 'tool-call-query-logs',
                toolName: 'query_logs',
                chatId: 'chat-checkout',
                investigationId: null,
                risk: 'l0',
                input: { query: 'service = checkout-api', authorization: '<redacted>' },
                outputSummary: '128 rows',
                status: 'success',
                error: null,
                durationMs: 284,
                calledBy: 'dev',
                callSource: 'chat',
                profileId: 'profile-production',
                approvalId: null,
                policyDecision: { allowed: true, executionMode: 'automatic' },
                auditId: 'audit-tool-call-1',
                createdAt: nowMicros - 120_000_000,
              },
            ]
          : [],
    }),
  );
  app.post('/api/v1/agent/tools/:id/test', (req, res) =>
    res.json({
      success: true,
      validated: true,
      dryRun: req.body?.dryRun !== false,
      executed: false,
      sideEffects: false,
      durationMs: 18,
      message: '参数校验与 dry-run 已完成。',
      request: req.body?.arguments ?? {},
      response: { rows: 0, preview: [] },
    }),
  );
  app.put('/api/v1/agent/tools/:id/policy', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    Object.assign(tool, req.body);
    return res.json(tool);
  });
  app.post('/api/v1/agent/tools/:id/enable', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    Object.assign(tool, { enabled: true, availableToAgent: true, status: 'healthy' });
    return res.json(tool);
  });
  app.post('/api/v1/agent/tools/:id/disable', (req, res) => {
    const tool = findTool(req.params.id);
    if (!tool) return res.status(404).json({ error: 'not found' });
    const dependencies = toolDependencies(req.params.id);
    if (dependencies.total > 0 && !req.body?.force) {
      return res.status(409).json({ error: 'tool has active dependencies', dependencies });
    }
    Object.assign(tool, { enabled: false, availableToAgent: false, status: 'disabled' });
    return res.json({ tool, dependencies });
  });
  app.get('/api/v1/agent/tools/:id', (req, res) => {
    const tool = findTool(req.params.id);
    return tool
      ? res.json({ tool, dependencies: toolDependencies(req.params.id) })
      : res.status(404).json({ error: 'not found' });
  });

  app.get('/api/v1/agent/mcp-servers', (_req, res) =>
    res.json({ servers: mcpServers }),
  );
  app.post('/api/v1/agent/mcp-servers', (req, res) => {
    const headers = (Array.isArray(req.body?.headers) ? req.body.headers : []) as Array<{
      name: string;
      secret: boolean;
      value?: string;
    }>;
    const server = {
      id: `mcp-${mcpServers.length + 1}`,
      orgId: 'acme-prod',
      handle: String(req.body?.name ?? 'mcp').toLowerCase().replace(/[^a-z0-9]+/g, '_'),
      commandTemplate: null,
      memberCredential: null,
      env: [],
      editable: true,
      moderatable: true,
      promptCount: 0,
      myCredential: null,
      credentialLast4:
        typeof req.body?.credential === 'string'
          ? req.body.credential.slice(-4)
          : null,
      credentialSet: Boolean(req.body?.credential),
      status: req.body?.enabled ? 'unavailable' : 'disabled',
      lastError: null,
      lastTestedAt: null,
      lastSyncedAt: null,
      createdBy: 'dev',
      createdAt: nowMicros,
      updatedAt: nowMicros,
      toolCount: 0,
      ...req.body,
      scope:
        req.body?.scope === 'team'
          ? { scope: 'team', teamId: String(req.body.teamId) }
          : req.body?.scope === 'personal'
            ? { scope: 'personal', ownerId: 'dev' }
            : { scope: 'org' },
      headers: headers.map((header) =>
        header.secret
          ? { name: header.name, secret: true, last4: (header.value ?? '').slice(-4), set: Boolean(header.value) }
          : { name: header.name, secret: false, value: header.value ?? '', set: true },
      ),
    };
    delete server.credential;
    delete server.teamId;
    mcpServers.push(server);
    res.json(server);
  });
  app.get('/api/v1/agent/mcp-servers/:id', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    const tools = agentTools.filter(
      (tool) => {
        const source = tool.source as
          | { kind?: string; serverId?: string }
          | undefined;
        return source?.kind === 'mcp' && source.serverId === req.params.id;
      },
    );
    const prompts =
      req.params.id === 'mcp-linear'
        ? [
            {
              orgId: 'acme-prod',
              serverId: 'mcp-linear',
              name: 'triage',
              title: 'Triage an issue',
              description: 'Triage a Linear issue',
              arguments: [{ name: 'issue', description: 'The issue key', required: true }],
              syncedAt: nowMicros,
            },
          ]
        : [];
    return server
      ? res.json({ server, tools, prompts })
      : res.status(404).json({ error: 'not found' });
  });
  app.put('/api/v1/agent/mcp-servers/:id', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    const credential =
      typeof req.body?.credential === 'string' ? req.body.credential : '';
    const next = { ...req.body };
    delete next.credential;
    Object.assign(server, next, {
      ...(credential
        ? { credentialSet: true, credentialLast4: credential.slice(-4) }
        : {}),
      updatedAt: nowMicros,
    });
    return res.json(server);
  });
  app.delete('/api/v1/agent/mcp-servers/:id', (req, res) => {
    const index = mcpServers.findIndex((item) => item.id === req.params.id);
    if (index >= 0) mcpServers.splice(index, 1);
    res.json({});
  });
  app.post('/api/v1/agent/mcp-servers/:id/test', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    Object.assign(server, { status: 'healthy', lastError: null, lastTestedAt: nowMicros });
    return res.json({
      success: true,
      server,
      discoveredTools: [
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
  app.post('/api/v1/agent/mcp-servers/:id/sync', (req, res) => {
    const server = mcpServers.find((item) => item.id === req.params.id);
    if (!server) return res.status(404).json({ error: 'not found' });
    const selectedTools = Array.isArray(req.body?.selectedTools)
      ? req.body.selectedTools.map(String)
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
        remoteName: remoteName,
        displayName:
          remoteName === 'restart_workload'
            ? 'Restart workload'
            : 'Search knowledge',
        source: {
          kind: 'mcp',
          label: 'internal-observability',
          serverId: server.id,
          serverName: server.name,
        },
        executionMode: destructive ? 'dual_approval' : 'automatic',
        enabled: false,
        availableToAgent: false,
        status: 'disabled',
        lastSyncedAt: nowMicros,
      });
      agentTools.push(tool);
      return tool;
    });
    Object.assign(server, {
      lastSyncedAt: nowMicros,
      toolCount: selectedTools.length,
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
          insertText: 'rate(${1:metric}[${2:5m}])',
          detail: 'rate(range-vector)',
          documentation: 'Per-second average rate over a range vector.',
          kind: 'function',
        },
        {
          label: 'irate',
          insertText: 'irate(${1:metric}[${2:5m}])',
          detail: 'irate(range-vector)',
          documentation: 'Instantaneous per-second rate.',
          kind: 'function',
        },
        {
          label: 'increase',
          insertText: 'increase(${1:metric}[${2:5m}])',
          detail: 'increase(range-vector)',
          documentation: 'Total increase over a range vector.',
          kind: 'function',
        },
        {
          label: 'avg_over_time',
          insertText: 'avg_over_time(${1:metric}[${2:5m}])',
          detail: 'avg_over_time(range-vector)',
          documentation: 'Average over a range vector.',
          kind: 'function',
        },
        {
          label: 'abs',
          insertText: 'abs(${1:vector})',
          detail: 'abs(vector)',
          documentation: 'Absolute value.',
          kind: 'function',
        },
        {
          label: 'histogram_quantile',
          insertText: 'histogram_quantile(${1:0.95}, ${2:vector})',
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
  // ── APM: a workspace that has not received a trace yet. A spec that needs
  // services, transactions or errors routes its own `/api/v1/apm/**`. ──
  const emptyApmPage = (sort: string) => {
    const to = Date.parse(FROZEN_NOW_ISO) * 1_000;
    return {
      meta: {
        range: { from: to - 3_600_000_000, to },
        resolution: 'minute',
        dataQuality: { partial: false, gaps: [], overflowDimensions: [] },
        activationBoundary: false,
      },
      items: [],
      nextCursor: null,
      previousCursor: null,
      hasMore: false,
      sort,
    };
  };
  app.get('/api/v1/apm/services', (_req, res) => res.json(emptyApmPage('request_count')));
  app.get('/api/v1/apm/errors', (_req, res) => res.json(emptyApmPage('last_seen')));
  app.get('/api/v1/apm/service-definitions', (_req, res) => res.json([]));
  app.get('/api/v1/apm/overview', (_req, res) => {
    const to = Date.parse(FROZEN_NOW_ISO) * 1_000;
    res.json({
      meta: {
        range: { from: to - 3_600_000_000, to },
        resolution: 'minute',
        dataQuality: { partial: false, gaps: [], overflowDimensions: [] },
        activationBoundary: false,
      },
      red: {
        requestCount: 0,
        errorCount: 0,
        errorRate: 0,
        durationSum: 0,
        latencyPartial: false,
        exemplars: [],
      },
      trend: [],
      serviceHealth: { healthy: 0, warning: 0, critical: 0, no_traffic: 0 },
      services: [],
      topTransactions: [],
      topDependencies: [],
      topErrors: [],
      recentVersions: [],
    });
  });
  type AskedQuery = { language?: string; timeRange?: { start?: number | undefined; end?: number | undefined } } | undefined;
  const answerQuery = (body: AskedQuery, res: Response) => {
    if (body?.language === 'promql') {
      const start = Number(body?.timeRange?.start ?? Date.parse(FROZEN_NOW_ISO) * 1000 - 3_600_000_000);
      const end = Number(body?.timeRange?.end ?? Date.parse(FROZEN_NOW_ISO) * 1000);
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
        scannedRows: 8421,
        tookMs: 14,
      });
    }
    return res.json({
      columns: ['_timestamp', 'value'],
      rows: [[Date.parse(FROZEN_NOW_ISO) * 1_000, 1]],
      scannedRows: 1,
      tookMs: 2,
    });
  };
  app.post('/api/v1/query', (req: Request, res: Response) => answerQuery(req.body, res));
  // The explorers ask with GET, the query in the address.
  app.get('/api/v1/query', (req: Request, res: Response) => {
    const at = (name: string) => (req.query[name] === undefined ? undefined : Number(req.query[name]));
    answerQuery({ language: String(req.query.language ?? ''), timeRange: { start: at('start'), end: at('end') } }, res);
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
    res.json([{ id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' }]),
  );
  app.get('/api/v1/sso/providers', (_req, res) =>
    res.json([
      {
        id: 'okta',
        orgId: 'acme-prod',
        name: 'Okta',
        kind: 'oidc',
        platform: 'okta',
        enabled: true,
        oidc: {
          issuer: 'https://dev-123456.okta.com/oauth2/default',
          authorizeUrl: 'https://dev-123456.okta.com/oauth2/default/v1/authorize',
          tokenUrl: 'https://dev-123456.okta.com/oauth2/default/v1/token',
          userinfoUrl: 'https://dev-123456.okta.com/oauth2/default/v1/userinfo',
          jwksUri: 'https://dev-123456.okta.com/oauth2/default/v1/keys',
          clientId: '0oa1molesignal',
          hasClientSecret: true,
          redirectUri: 'http://localhost:5173/api/v1/auth/sso/callback',
          scopes: ['openid', 'profile', 'email'],
          fieldMapping: { subject: 'sub', email: 'email', displayName: 'name', groups: 'groups' },
          groupRoleMapping: {},
          allowedGroups: [],
        },
        test: ssoTests.standing('okta'),
      },
    ]),
  );
  // Reads the issuer's discovery document on the real server.
  app.post('/api/v1/sso/providers/discover', (req, res) => {
    const issuer = String((req.body as { issuer?: string })?.issuer ?? '').replace(/\/+$/, '');
    res.json({
      issuer,
      authorizeUrl: `${issuer}/authorize`,
      tokenUrl: `${issuer}/token`,
      userinfoUrl: `${issuer}/userinfo`,
      jwksUri: `${issuer}/jwks`,
      scopesSupported: ['openid', 'profile', 'email'],
    });
  });
  app.post('/api/v1/sso/providers/saml-metadata', (_req, res) =>
    res.json({
      idpEntityId: 'https://sts.windows.net/00000000-0000-0000-0000-000000000000/',
      idpSsoUrl: 'https://login.microsoftonline.com/00000000-0000-0000-0000-000000000000/saml2',
      idpX509Cert: '-----BEGIN CERTIFICATE-----\nMIIC8DCCAdigAwIBAgIQ\n-----END CERTIFICATE-----',
    }),
  );
  // The handoff code of a redirect sign-in. `link-demo` comes back as a first sign-in into
  // an existing account, waiting for its owner to confirm; any other code has expired.
  app.post('/api/v1/auth/sso/exchange', (req, res) => {
    if ((req.body as { code?: string })?.code === 'link-demo') {
      res.json({
        link: {
          code: 'pending-link',
          providerName: 'Okta',
          organizationName: 'acme-prod',
          emailHint: 'd***@molesignal.local',
          emailAvailable: true,
          passwordAllowed: true,
        },
      });
      return;
    }
    res.status(401).json({
      error: 'unauthorized',
      message: 'unauthorized: this sign-in link has expired; sign in again',
    });
  });
  app.post('/api/v1/auth/sso/link/email', (_req, res) => res.json({ sent: true }));
  // `123456` from the email, or any password except `wrong`, confirms the link.
  app.post('/api/v1/auth/sso/link/confirm', (req, res) => {
    const body = req.body as { emailCode?: string; password?: string };
    const confirmed =
      body?.emailCode === '123456' || (Boolean(body?.password) && body?.password !== 'wrong');
    if (!confirmed) {
      res.status(401).json({
        error: 'unauthorized',
        message: 'unauthorized: the code or password is not correct',
      });
      return;
    }
    res.json({ login: signinResponse });
  });
  let linkedIdentities = [
    {
      id: 'sso-okta',
      providerId: 'okta',
      providerName: 'Okta',
      kind: 'oidc',
      platform: 'okta',
      orgId: 'acme-prod',
      orgName: 'acme-prod',
      linkedAt: 1_760_000_000_000_000,
      lastLoginAt: 1_761_400_000_000_000,
      canUnlink: true,
    },
  ];
  app.get('/api/v1/me/sso-identities', (_req, res) => res.json(linkedIdentities));
  app.delete('/api/v1/me/sso-identities/:id', (req, res) => {
    linkedIdentities = linkedIdentities.filter((identity) => identity.id !== req.params.id);
    res.status(204).end();
  });
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
        displayRole: 'Owner',
        roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
      },
      {
        id: 'acme-staging',
        name: 'acme-staging',
        slug: 'acme-staging',
        displayRole: 'Admin',
        roles: [{ id: 'role-admin', key: 'admin', name: 'Admin', builtin: true }],
      },
      // Listed for whoever was granted platform permissions, to use them there.
      {
        id: 'system-org',
        name: '_sys',
        slug: '_sys',
        displayRole: 'Platform administrator',
        roles: [{ id: 'role-platform-admin', key: 'platform_admin', name: 'Platform administrator', builtin: true }],
        system: true,
        disabled: false,
        accessible: true,
      },
    ]),
  );
  app.post('/api/v1/orgs/:id/select', (req, res) =>
    res.json({
      userId: 'u1',
      orgId: req.params.id,
      orgName: req.params.id,
      displayRole: 'Owner',
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
          displayName: 'Dana Reyes',
          displayRole: 'Admin',
          roles: [{ id: 'role-admin', key: 'admin', name: 'Admin', builtin: true }],
        },
        {
          id: 'u2',
          email: 'kenji@example.com',
          displayName: 'Kenji Watanabe',
          displayRole: 'Editor',
          roles: [{ id: 'role-editor', key: 'editor', name: 'Editor', builtin: true }],
          mfaEnabled: true,
        },
        {
          id: 'u3',
          email: 'amara@example.com',
          displayName: 'Amara Okafor',
          displayRole: 'Editor',
          roles: [{ id: 'role-editor', key: 'editor', name: 'Editor', builtin: true }],
        },
      ].map((user) => ({
        isRoot: false,
        disabled: false,
        status: 'active',
        teamNames: [],
        loginMethod: 'password',
        createdAt: Date.parse(FROZEN_NOW_ISO) * 1_000 - 90 * 86_400_000_000,
        ...user,
      })),
    }),
  );
  app.get('/api/v1/teams', (_req, res) =>
    res.json([
      { id: 'team-platform', orgId: 'acme-prod', name: 'Platform', memberIds: ['u1', 'u2'] },
      { id: 'team-payments', orgId: 'acme-prod', name: 'Payments', memberIds: ['u3'] },
    ]),
  );
  app.get('/api/v1/auth/tokens', (_req, res) => res.json([]));
  // The signed-in user's own tokens, kept for the test like the backend keeps them.
  const personalTokens: Array<Record<string, unknown>> = [];
  app.get('/api/v1/me/api-tokens', (_req, res) => res.json(personalTokens));
  app.post('/api/v1/me/api-tokens', (req, res) => {
    const body = req.body as {
      name: string;
      policyId?: string;
      permissions?: string[];
      expiresInDays?: number;
    };
    const issuedAt = Date.parse(FROZEN_NOW_ISO) * 1000;
    const token = {
      id: `tok-personal-${personalTokens.length + 1}`,
      name: body.name,
      prefix: '0123456789',
      tokenKind: 'personal',
      userId: 'u1',
      policyId: body.policyId ?? null,
      permissions: body.permissions ?? null,
      applicationId: null,
      serviceAccountId: null,
      expiresAt: body.expiresInDays
        ? issuedAt + body.expiresInDays * 86_400_000_000
        : null,
      lastUsedAt: null,
      revoked: false,
      createdAt: issuedAt,
    };
    personalTokens.unshift(token);
    res.json({ ...token, token: 'mspat-0123456789abcdefghjkmnpqrstvwxyz0123' });
  });
  app.get('/api/v1/service-accounts', (_req, res) => res.json([]));

  // Permission policies: the built-in ones and the organization's own, as the backend keeps them.
  const policies: Array<ReturnType<typeof mockBuiltinPolicy>> = [
    mockBuiltinPolicy('readonly_observer', 'Read-only observer', MOCK_IAM_VIEWER_PERMISSIONS),
    mockBuiltinPolicy('data_intake', 'Data intake', [
      'logs.write',
      'metrics.write',
      'traces.write',
      'profiles.write',
      'apm.deploy',
    ]),
  ];
  const roles: Array<Record<string, unknown> & { id: string; permissions: readonly string[]; policyIds: string[] }> = [
    {
      id: 'role-owner',
      key: 'owner',
      name: 'Owner',
      description: 'Full administrative access.',
      builtin: true,
      roleType: 'organization',
      scope: 'organization',
      permissions: MOCK_IAM_ROLE_PERMISSIONS.owner ?? [],
      policyIds: [],
      usage: { memberships: 1, serviceAccounts: 0, invitations: 0, bindings: 1, total: 2 },
      createdAt: Date.parse(FROZEN_NOW_ISO) * 1000,
      updatedAt: Date.parse(FROZEN_NOW_ISO) * 1000,
    },
  ];
  const roleOut = (role: (typeof roles)[number]) => ({
    ...role,
    effectivePermissions: [
      ...new Set([
        ...role.permissions,
        ...policies
          .filter((policy) => role.policyIds.includes(policy.id))
          .flatMap((policy) => policy.permissions),
      ]),
    ].sort(),
  });
  const policyOut = (policy: (typeof policies)[number]) => {
    const usedBy = roles.filter((role) => role.policyIds.includes(policy.id)).length;
    const followedBy = personalTokens.filter(
      (token) => token.policyId === policy.id && !token.revoked,
    ).length;
    return { ...policy, usage: { roles: usedBy, tokens: followedBy, total: usedBy + followedBy } };
  };

  app.get('/api/v1/iam/policies', (_req, res) => res.json(policies.map(policyOut)));
  app.post('/api/v1/iam/policies', (req, res) => {
    const body = req.body as { key: string; name: string; description?: string; document: unknown };
    const resolved = mockResolvePolicy(body.document);
    if ('error' in resolved) return res.status(400).json({ error: resolved.error });
    if (policies.some((policy) => policy.key === body.key)) {
      return res.status(409).json({ error: `a permission policy with key ${body.key} already exists` });
    }
    const now = Date.parse(FROZEN_NOW_ISO) * 1000;
    const policy = {
      id: `policy-${body.key}`,
      key: body.key,
      name: body.name,
      description: body.description ?? '',
      builtin: false,
      document: body.document as ReturnType<typeof mockBuiltinPolicy>['document'],
      permissions: resolved.permissions,
      createdBy: 'u1',
      createdAt: now,
      updatedAt: now,
    };
    policies.push(policy);
    return res.json(policyOut(policy));
  });
  app.patch('/api/v1/iam/policies/:id', (req, res) => {
    const policy = policies.find((candidate) => candidate.id === req.params.id);
    if (!policy) return res.status(404).json({ error: 'permission policy not found' });
    if (policy.builtin) return res.status(403).json({ error: 'built-in permission policies cannot be edited' });
    const body = req.body as { name: string; description?: string; document: unknown };
    const resolved = mockResolvePolicy(body.document);
    if ('error' in resolved) return res.status(400).json({ error: resolved.error });
    Object.assign(policy, {
      name: body.name,
      description: body.description ?? '',
      document: body.document,
      permissions: resolved.permissions,
    });
    return res.json(policyOut(policy));
  });
  app.delete('/api/v1/iam/policies/:id', (req, res) => {
    const index = policies.findIndex((candidate) => candidate.id === req.params.id);
    const policy = policies[index];
    if (!policy) return res.status(404).json({ error: 'permission policy not found' });
    if (policy.builtin) return res.status(403).json({ error: 'built-in permission policies cannot be deleted' });
    if (policyOut(policy).usage.total > 0) return res.status(409).json({ error: 'permission policy is in use' });
    policies.splice(index, 1);
    return res.json({ deleted: true });
  });
  app.get('/api/v1/me/api-tokens/scopes', (_req, res) =>
    res.json({
      policies: policies.map(({ id, key, name, description, builtin, permissions }) => ({
        id,
        key,
        name,
        description,
        builtin,
        permissions,
      })),
      permissions: MOCK_IAM_PERMISSION_CATALOG.permissions.filter(
        (permission) => permission.scope === 'organization',
      ),
    }),
  );

  app.get('/api/v1/roles', (_req, res) => res.json(roles.map(roleOut)));
  app.post('/api/v1/roles', (req, res) => {
    const body = req.body as {
      key: string;
      name: string;
      description?: string;
      permissions: string[];
      policyIds: string[];
    };
    const now = Date.parse(FROZEN_NOW_ISO) * 1000;
    const role = {
      id: `role-${body.key}`,
      key: body.key,
      name: body.name,
      description: body.description ?? '',
      builtin: false,
      roleType: 'organization',
      scope: 'organization',
      permissions: [...body.permissions].sort(),
      policyIds: body.policyIds,
      usage: { memberships: 0, serviceAccounts: 0, invitations: 0, bindings: 0, total: 0 },
      createdAt: now,
      updatedAt: now,
    };
    roles.push(role);
    res.json(roleOut(role));
  });
  app.get('/api/v1/iam/permissions', (_req, res) =>
    res.json(MOCK_IAM_PERMISSION_CATALOG),
  );
  app.get('/api/v1/iam/capabilities', (_req, res) =>
    res.json({
      organizationId: 'acme-prod',
      scope: 'organization',
      displayRole: 'Owner',
      roles: [
        {
          id: 'role-owner',
          key: 'owner',
          name: 'Owner',
          builtin: true,
        },
      ],
      permissions: MOCK_IAM_ROLE_PERMISSIONS.owner,
      version: 1,
      routeCatalogVersion: 1,
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
  // As the backend does: every save, a new report or an existing one, makes the signed-in
  // user the one the report runs as, and the response names that user.
  const savedByMockUser = () => ({
    runAsUserId: meProfile.userId,
    runAsUser: {
      id: meProfile.userId,
      displayName: meProfile.displayName,
      email: meProfile.email,
    },
  });
  app.post('/api/v1/scheduled_reports', (req, res) => {
    const now = Date.parse(FROZEN_NOW_ISO) * 1_000;
    const report = {
      ...req.body,
      id: `r${scheduledReports.length + 1}`,
      orgId: MOCK_AUTH.state.ctx.orgId,
      ...savedByMockUser(),
      lastRunAt: null,
      createdAt: now,
      updatedAt: now,
    };
    scheduledReports.push(report);
    res.json(report);
  });
  app.put('/api/v1/scheduled_reports/:id', (req, res) => {
    const index = scheduledReports.findIndex((report) => report.id === req.params.id);
    if (index === -1) {
      res.status(404).json({ error: `scheduled report ${req.params.id} not found` });
      return;
    }
    scheduledReports[index] = {
      ...scheduledReports[index],
      ...req.body,
      ...savedByMockUser(),
      updatedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
    };
    res.json(scheduledReports[index]);
  });
  app.get('/api/v1/scheduled_reports/:id/deliveries', (_req, res) =>
    res.json([]),
  );
  app.get('/api/v1/scheduled_reports/:id/preview', (req, res) =>
    res
      .type('application/json')
      .send(
        JSON.stringify({
          reportId: req.params.id,
          generatedAt: FROZEN_NOW_ISO,
        }),
      ),
  );
  app.get('/api/v1/resource_shares/policy', (_req, res) =>
    res.json(resourceSharePolicy),
  );
  app.put('/api/v1/resource_shares/policy', (req, res) => {
    resourceSharePolicy = {
      ...resourceSharePolicy,
      ...req.body,
      updatedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
    };
    res.json(resourceSharePolicy);
  });
  app.get('/api/v1/resource_shares', (req, res) => {
    const resourceType =
      typeof req.query.resourceType === 'string'
        ? req.query.resourceType
        : null;
    const resourceId =
      typeof req.query.resourceId === 'string'
        ? req.query.resourceId
        : null;
    res.json(
      resourceShares
        .filter(
          (share) =>
            (!resourceType || share.resourceType === resourceType) &&
            (!resourceId || share.resourceId === resourceId),
        )
        .map(({ token, password: _password, ...share }) => ({
          ...share,
          url:
            share.enabled && !share.revokedAt
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
      req.body?.shareMode === 'public_link' &&
      req.body?.resourceType === 'dashboard' &&
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
      typeof req.body?.expiresInSecs === 'number'
        ? req.body.expiresInSecs
        : null;
    const token = `ms${String(resourceShares.length + 1).padStart(10, '0')}`;
    const expiresAt =
      expiresInSecs === null
        ? null
        : createdAt + expiresInSecs * 1_000_000;
    const share = {
      id: `share-${resourceShares.length + 1}`,
      organizationId: 'acme-prod',
      resourceType: req.body.resourceType,
      resourceId: req.body.resourceId,
      resourceVersionId: null,
      shareMode: req.body.shareMode,
      permissions:
        req.body.resourceType === 'dashboard'
          ? ['dashboards.view', 'dashboard_panels.execute']
          : ['reports.view'],
      constraints: req.body.constraints ?? {},
      expiresAt: expiresAt,
      maxViews: req.body.maxViews ?? null,
      viewCount: 0,
      allowDownload: req.body.allowDownload ?? false,
      enabled: true,
      crossOrgGrantId: null,
      snapshotContentType:
        req.body.resourceType === 'report' ? 'application/pdf' : null,
      snapshotFilename:
        req.body.resourceType === 'report' ? 'weekly-slo.pdf' : null,
      createdBy: 'dev',
      createdAt: createdAt,
      lastAccessedAt: null,
      revokedAt: null,
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
    share.revokedAt = Date.parse(FROZEN_NOW_ISO) * 1_000;
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
    if (!share) return res.redirect(302, '/shared?unavailable=1');
    if (share.shareMode === 'public_link') {
      res.cookie('molesignal_share_session', `session-${share.id}`, {
        httpOnly: true,
        sameSite: 'lax',
      });
      return res.redirect(302, '/shared');
    }
    return res.redirect(
      302,
      share.resourceType === 'dashboard'
        ? `/dashboards/${share.resourceId}`
        : `/reports?report=${share.resourceId}`,
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
    if (!share || share.shareMode !== 'public_link') {
      return res.status(401).json({ error: 'missing share session' });
    }
    const requiresPassword =
      typeof share.password === 'string' &&
      !unlockedShareSessions.has(String(sessionId));
    if (requiresPassword) {
      return res.json({
        kind: share.resourceType,
        requiresPassword: true,
        expiresAt: share.expiresAt,
      });
    }
    if (share.resourceType === 'dashboard') {
      const dashboard = dashboards.find(
        (item) => item.id === share.resourceId,
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
        requiresPassword: false,
        expiresAt: share.expiresAt,
        constraints: share.constraints,
        definition,
        watermark: {
          shareId: share.id,
          accessedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
        },
      });
    }
    return res.json({
      kind: 'report',
      title: 'Weekly SLO',
      format: 'pdf',
      requiresPassword: false,
      allowDownload: share.allowDownload,
      expiresAt: share.expiresAt,
      contentType: 'application/pdf',
      watermark: {
        shareId: share.id,
        accessedAt: Date.parse(FROZEN_NOW_ISO) * 1_000,
      },
    });
  });
  app.post('/api/v1/public/share/query', (_req, res) =>
    res.json({
      columns: ['timestamp', 'value'],
      rows: [[Date.parse(FROZEN_NOW_ISO) * 1_000, 1]],
      scannedRows: 1,
      tookMs: 1,
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
    if (!share || share.shareMode !== 'public_link') {
      return res.status(401).json({ error: 'missing share session' });
    }
    if (
      typeof share.password === 'string' &&
      req.body?.password !== share.password
    ) {
      return res.status(401).json({
        error: 'unauthorized',
        code: 'share_password_invalid',
        message: 'invalid share password',
      });
    }
    unlockedShareSessions.add(String(sessionId));
    return res.json({ unlocked: true });
  });

  const agentToolsets: Array<{
    id: string;
    name: string;
    enabled: boolean;
    schema: unknown;
    updatedAt: number;
  }> = [];
  app.get('/api/v1/agent/settings/toolsets', (_req, res) =>
    res.json(agentToolsets),
  );
  app.post('/api/v1/agent/settings/toolsets', (req: Request, res: Response) => {
    const row = {
      id: `tool-${agentToolsets.length + 1}`,
      name: String(req.body?.name ?? ''),
      enabled: Boolean(req.body?.enabled ?? true),
      schema: req.body?.schema ?? {},
      updatedAt: Date.now() * 1000,
    };
    agentToolsets.push(row);
    res.json(row);
  });
  app.delete('/api/v1/agent/settings/toolsets/:id', (req, res) => {
    const idx = agentToolsets.findIndex((r) => r.id === req.params.id);
    if (idx >= 0) agentToolsets.splice(idx, 1);
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
 * login plus the look the mock user has saved, so the app boots
 * deterministically and stays the way it booted.
 *
 * Call from `test.beforeEach` in every behavior / perf spec.
 */
export async function mountMockRoutes(
  page: Page,
  port: number,
  opts: {
    /**
     * The theme and the density of the page. Each is saved as the mock user's
     * preference and remembered by the browser, the way a look the user chose
     * is. Left out, it is the mock user's default: the system theme, normal
     * density.
     */
    theme?: 'dark' | 'light';
    density?: 'comfortable' | 'normal' | 'compact';
    /**
     * Other personal preferences the mock user has already saved, such as the
     * language. A theme or density given here counts the same as `theme` and
     * `density`.
     */
    preferences?: Partial<UserPreferences>;
    /** Boot without a session, for a page that is shown before sign-in. */
    signedOut?: boolean;
    role?: string;
    orgId?: string;
    orgName?: string;
    scope?: 'organization' | 'system' | 'api_token';
    platformPermissions?: string[];
    capabilityPermissions?: string[];
    /** Platform permissions held on named organizations only: permission to organization ids. */
    organizationPermissions?: Record<string, string[]>;
  } = {},
): Promise<void> {
  // The shell applies the saved preferences over what the browser remembered
  // as soon as it has read them. A page seeded with one look and saved with
  // another would change a moment after it loads, so both come from here.
  const saved: Partial<UserPreferences> = { ...opts.preferences };
  if (opts.theme) saved.theme = opts.theme;
  if (opts.density) saved.density = opts.density;
  const theme = saved.theme ?? MOCK_USER_PREFERENCES.theme;
  const density = saved.density ?? MOCK_USER_PREFERENCES.density;
  const auth = {
    ...MOCK_AUTH,
    state: {
      ...MOCK_AUTH.state,
      ctx: {
        ...MOCK_AUTH.state.ctx,
        orgId: opts.orgId ?? MOCK_AUTH.state.ctx.orgId,
        orgName: opts.orgName ?? MOCK_AUTH.state.ctx.orgName,
        displayRole: opts.role ?? MOCK_AUTH.state.ctx.displayRole,
        roles:
          (opts.scope ?? MOCK_AUTH.state.ctx.scope) === 'system'
            ? [
                {
                  id: 'role-platform-admin',
                  key: 'platform_admin',
                  name: opts.role ?? MOCK_AUTH.state.ctx.displayRole,
                  builtin: true,
                },
              ]
            : [
                {
                  id: `role-${(opts.role ?? MOCK_AUTH.state.ctx.displayRole).toLowerCase().replaceAll(' ', '-')}`,
                  key: (opts.role ?? MOCK_AUTH.state.ctx.displayRole)
                    .toLowerCase()
                    .replaceAll(' ', '_'),
                  name: opts.role ?? MOCK_AUTH.state.ctx.displayRole,
                  builtin: Object.hasOwn(
                    MOCK_IAM_ROLE_PERMISSIONS,
                    (opts.role ?? MOCK_AUTH.state.ctx.displayRole).toLowerCase(),
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
  if (Object.keys(saved).length > 0) {
    const response = await fetch(`http://127.0.0.1:${port}/api/v1/me/preferences`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(saved),
    });
    if (!response.ok) {
      throw new Error(`mock preferences were not saved: ${response.status}`);
    }
  }
  await page.addInitScript(
    ({ theme, density, auth, themeKey, densityKey, explicitKey, authKey }) => {
      // The look the browser remembers from the mock user's last visit. Only
      // what is missing is written, so a later document finds what the app
      // has stored since, as it would in a real browser.
      if (localStorage.getItem(themeKey) === null) {
        localStorage.setItem(themeKey, theme);
        if (theme !== 'system') localStorage.setItem(explicitKey, '1');
      }
      if (localStorage.getItem(densityKey) === null) {
        localStorage.setItem(densityKey, density);
      }
      // Seed the zustand-persisted identity the page shows, so RequireAuth
      // exercises the authenticated application path.
      if (auth) localStorage.setItem(authKey, JSON.stringify(auth));
    },
    {
      theme,
      density,
      auth: opts.signedOut ? null : auth,
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
  // The browser's session cookies, shared by its tabs: the first tab mounted in
  // a browser context decides whether it starts signed in.
  let cookies = BROWSER_COOKIES.get(page.context());
  if (!cookies) {
    cookies = { session: opts.signedOut ? null : sessionView(auth.state.ctx) };
    BROWSER_COOKIES.set(page.context(), cookies);
  }
  const jar = cookies;
  await page.route('**/api/v1/**', async (route) => {
    const reqUrl = new URL(route.request().url());
    if (reqUrl.pathname === '/api/v1/auth/session' || reqUrl.pathname === '/api/v1/auth/refresh') {
      if (!jar.session) {
        await route.fulfill({
          status: 401,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'unauthorized', message: 'unauthorized: not signed in' }),
        });
      } else if (reqUrl.pathname === '/api/v1/auth/refresh') {
        await route.fulfill({ status: 204 });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(jar.session),
        });
      }
      return;
    }
    if (reqUrl.pathname === '/api/v1/auth/signout') {
      jar.session = null;
      await route.fulfill({ status: 204 });
      return;
    }
    // Signing in sets the cookies (the second step's answer too, when one is asked for).
    if (
      reqUrl.pathname.startsWith('/api/v1/auth/signin') ||
      reqUrl.pathname === '/api/v1/auth/signup/complete'
    ) {
      jar.session = sessionView(auth.state.ctx);
    }
    if (reqUrl.pathname === '/api/v1/iam/permissions') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(MOCK_IAM_PERMISSION_CATALOG),
      });
      return;
    }
    if (reqUrl.pathname === '/api/v1/iam/capabilities') {
      const role = opts.role ?? MOCK_AUTH.state.ctx.displayRole;
      // The page names the session it shows; after a switch that is another organization.
      const shownOrg = (route.request().headers()['x-molesignal-session'] ?? '').split(':')[1];
      const signedInOrg = opts.orgId ?? MOCK_AUTH.state.ctx.orgId;
      const switchedOrg = shownOrg && shownOrg !== signedInOrg ? shownOrg : undefined;
      const scope: 'organization' | 'system' | 'api_token' = switchedOrg
        ? 'organization'
        : (opts.scope ?? 'organization');
      const platformPermissionMap: Record<string, string> = {
        system_telemetry_read: 'sys.telemetry.read',
        system_telemetry_manage: 'sys.telemetry.manage',
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
          organizationId: switchedOrg ?? signedInOrg,
          scope,
          displayRole: role,
          organizationPermissions: opts.organizationPermissions ?? {},
          roles:
            scope === 'system'
              ? [
                  {
                    id: 'role-platform-admin',
                    key: 'platform_admin',
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
          version: 1,
          routeCatalogVersion: 1,
          routes: mockCapabilityRoutes(scope, [
            ...permissions,
            ...Object.keys(opts.organizationPermissions ?? {}),
          ]),
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
