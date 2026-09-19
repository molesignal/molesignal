import { useQueries } from '@tanstack/react-query';
import * as React from 'react';

import * as agentAutomationsApi from '@/api/agent/automations';
import * as agentInvestigationsApi from '@/api/agent/investigations';
import * as agentMcpApi from '@/api/agent/mcpServers';
import * as agentModelProvidersApi from '@/api/agent/modelProviders';
import * as agentPromptsApi from '@/api/agent/prompts';
import * as alertsApi from '@/api/alerts';
import type { AuditEvent } from '@/api/audit';
import * as connectorsApi from '@/api/connectors';
import * as dashboardsApi from '@/api/dashboards';
import * as notifyApi from '@/api/notify';
import * as rolesApi from '@/api/roles';
import * as schedulesApi from '@/api/schedules';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import * as statusPagesApi from '@/api/statusPages';
import * as streamsApi from '@/api/streams';
import * as syntheticsApi from '@/api/synthetics';
import * as teamsApi from '@/api/teams';

interface NameEntry {
  kind: string;
  id: string;
  name: string;
}

interface Catalog {
  key: string;
  kinds: string[];
  load: () => Promise<NameEntry[]>;
}

const STALE_TIME = 60_000;

function namedEntries(
  kinds: string[],
  rows: Array<{ id: string; name: string }>,
): NameEntry[] {
  return rows.flatMap((row) =>
    kinds.map((kind) => ({ kind, id: row.id, name: row.name })),
  );
}

const CATALOGS: Catalog[] = [
  {
    key: 'synthetic-monitors',
    kinds: ['synthetic_monitor'],
    load: async () => namedEntries(['synthetic_monitor'], await syntheticsApi.listMonitors()),
  },
  {
    key: 'synthetic-secrets',
    kinds: ['synthetic_secret'],
    load: async () => namedEntries(['synthetic_secret'], await syntheticsApi.listSecrets()),
  },
  {
    key: 'status-pages',
    kinds: ['status_page'],
    load: async () => {
      const [active, archived] = await Promise.all([
        statusPagesApi.list('active'),
        statusPagesApi.list('archived'),
      ]);
      return namedEntries(['status_page'], [...active, ...archived]);
    },
  },
  {
    key: 'schedules',
    kinds: ['schedule', 'on_call_schedule'],
    load: async () =>
      namedEntries(['schedule', 'on_call_schedule'], await schedulesApi.list()),
  },
  {
    key: 'dashboards',
    kinds: ['dashboard'],
    load: async () =>
      namedEntries(
        ['dashboard'],
        (await dashboardsApi.list()).map((row) => ({ id: row.id, name: row.title })),
      ),
  },
  {
    key: 'alert-rules',
    kinds: ['alert', 'alert_rule'],
    load: async () => namedEntries(['alert', 'alert_rule'], await alertsApi.list()),
  },
  {
    key: 'streams',
    kinds: ['stream'],
    load: async () => {
      const rows = await streamsApi.list();
      return rows.flatMap((row) => [
        { kind: 'stream', id: row.id, name: row.name },
        { kind: 'stream', id: row.name, name: row.name },
      ]);
    },
  },
  {
    key: 'teams',
    kinds: ['team'],
    load: async () => namedEntries(['team'], await teamsApi.list()),
  },
  {
    key: 'roles',
    kinds: ['role'],
    load: async () => namedEntries(['role'], await rolesApi.list()),
  },
  {
    key: 'service-accounts',
    kinds: ['service_account'],
    load: async () => namedEntries(['service_account'], await serviceAccountsApi.list()),
  },
  {
    key: 'connectors',
    kinds: ['connector'],
    load: async () => namedEntries(['connector'], await connectorsApi.list()),
  },
  {
    key: 'notify-connectors',
    kinds: ['notify_connector'],
    load: async () => namedEntries(['notify_connector'], await notifyApi.listConnectors()),
  },
  {
    key: 'agent-prompts',
    kinds: ['agent_prompt'],
    load: async () => namedEntries(['agent_prompt'], await agentPromptsApi.list()),
  },
  {
    key: 'agent-model-providers',
    kinds: ['agent_model_provider'],
    load: async () =>
      namedEntries(['agent_model_provider'], await agentModelProvidersApi.list()),
  },
  {
    key: 'agent-mcp-servers',
    kinds: ['agent_mcp_server'],
    load: async () =>
      namedEntries(['agent_mcp_server'], await agentMcpApi.listMcpServers()),
  },
  {
    key: 'agent-investigations',
    kinds: ['agent_investigation'],
    load: async () =>
      namedEntries(
        ['agent_investigation'],
        (await agentInvestigationsApi.listInvestigations()).map((row) => ({
          id: row.id,
          name: row.title,
        })),
      ),
  },
  {
    key: 'agent-automations',
    kinds: ['agent_automation'],
    load: async () =>
      namedEntries(['agent_automation'], await agentAutomationsApi.listAutomations()),
  },
];

export function resourceNameKey(kind: string, id: string): string {
  return `${kind}:${id}`;
}

export function auditTargetName(
  event: AuditEvent,
  names: ReadonlyMap<string, string>,
): string {
  const kind = event.target_kind?.trim();
  const id = event.target_id?.trim();
  if (!kind) return '—';
  const resolved = id ? names.get(resourceNameKey(kind, id)) : undefined;
  const name = resolved ?? payloadName(event.payload);
  return `${kind}${name ? `/${name}` : id ? `/${id}` : ''}`;
}

export function rawAuditTarget(event: AuditEvent): string {
  const kind = event.target_kind?.trim();
  const id = event.target_id?.trim();
  return kind ? `${kind}${id ? `/${id}` : ''}` : '—';
}

export function useAuditResourceNames(events: AuditEvent[]): ReadonlyMap<string, string> {
  const targetKinds = React.useMemo(
    () => new Set(events.map((event) => event.target_kind).filter(Boolean)),
    [events],
  );
  const actorKinds = React.useMemo(
    () => new Set(events.map((event) => event.actor_kind).filter(Boolean)),
    [events],
  );

  const catalogQueries = useQueries({
    queries: CATALOGS.map((catalog) => ({
      queryKey: ['audit', 'resource-names', catalog.key],
      queryFn: catalog.load,
      enabled:
        catalog.kinds.some((kind) => targetKinds.has(kind)) ||
        (catalog.key === 'status-pages' &&
          [...targetKinds].some((kind) => kind?.startsWith('status_page'))) ||
        (catalog.kinds.includes('service_account') && actorKinds.has('service_account')),
      staleTime: STALE_TIME,
      retry: false,
    })),
  });

  const statusPageIds = React.useMemo(() => {
    const ids = new Set<string>();
    for (const event of events) {
      if (!event.target_kind?.startsWith('status_page_')) continue;
      const id = stringValue(recordOf(event.payload).status_page_id);
      if (id) ids.add(id);
    }
    return [...ids];
  }, [events]);

  const statusPageQueries = useQueries({
    queries: statusPageIds.map((pageId) => ({
      queryKey: ['audit', 'resource-names', 'status-page-snapshot', pageId],
      queryFn: async (): Promise<NameEntry[]> => {
        const snapshot = await statusPagesApi.get(pageId);
        const incidents = [
          ...snapshot.active_incidents,
          ...snapshot.scheduled_maintenance,
          ...snapshot.history,
        ];
        return [
          ...namedEntries(['status_page_component'], snapshot.components),
          ...namedEntries(
            ['status_page_event'],
            incidents.map((incident) => ({ id: incident.id, name: incident.title })),
          ),
        ];
      },
      staleTime: STALE_TIME,
      retry: false,
    })),
  });

  const automationQueries = useQueries({
    queries: targetKinds.has('status_page_automation_rule')
      ? statusPageIds.map((pageId) => ({
          queryKey: ['audit', 'resource-names', 'status-page-automation', pageId],
          queryFn: async (): Promise<NameEntry[]> =>
            namedEntries(
              ['status_page_automation_rule'],
              (await statusPagesApi.listAutomationRules(pageId)).map(({ rule }) => rule),
            ),
          staleTime: STALE_TIME,
          retry: false,
        }))
      : [],
  });

  const names = new Map<string, string>();
  for (const query of [...catalogQueries, ...statusPageQueries, ...automationQueries]) {
    for (const entry of query.data ?? []) {
      if (entry.name.trim()) names.set(resourceNameKey(entry.kind, entry.id), entry.name);
    }
  }
  for (const event of events) {
    const kind = event.target_kind?.trim();
    const id = event.target_id?.trim();
    const name = payloadName(event.payload);
    const statusPageId = stringValue(recordOf(event.payload).status_page_id);
    const parentPageName = statusPageId
      ? names.get(resourceNameKey('status_page', statusPageId))
      : undefined;
    if (kind && id && name && !names.has(resourceNameKey(kind, id))) {
      names.set(resourceNameKey(kind, id), name);
    } else if (kind && id && parentPageName && !names.has(resourceNameKey(kind, id))) {
      names.set(resourceNameKey(kind, id), parentPageName);
    }
  }
  return names;
}

function payloadName(payload: Record<string, unknown>): string | undefined {
  for (const key of ['name', 'title', 'display_name', 'resource_name', 'target_name']) {
    const value = stringValue(payload[key]);
    if (value) return value;
  }
  const resource = recordOf(payload.resource);
  return stringValue(resource.name) ?? stringValue(resource.title);
}

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}
