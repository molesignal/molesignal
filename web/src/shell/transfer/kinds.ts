import type { ImportReport } from '@/api/alerts';
import { http } from '@/lib/http';

import { SCHEDULE_SOURCES } from './scheduleSources';
import type { TransferKind } from './TransferMenu';

type ImportInput = { content: string; dryRun: boolean; createOnly: boolean };

/** A bundle exported from `path` under `listKey`, and an import posted to `importPath`. */
function bundleKind(
  path: string,
  importPath: string,
  listKey: string,
  rest: Omit<TransferKind, 'exportBundle' | 'importBundle'>,
): TransferKind {
  return {
    ...rest,
    exportBundle: async (ids) => {
      const { data } = await http.get<Record<string, unknown>>(path, {
        params: ids.length > 0 ? { ids: ids.join(',') } : {},
      });
      const items = data[listKey];
      return { bundle: data, count: Array.isArray(items) ? items.length : 0 };
    },
    importBundle: async (input: ImportInput) => {
      const { data } = await http.post<ImportReport>(importPath, input);
      return data;
    },
  };
}

export const ESCALATION_POLICIES = bundleKind('/alerts/escalations/export', '/alerts/escalations/import', 'policies', {
  testPrefix: 'policy',
  texts: 'transfer.policies',
  fileName: 'molesignal-escalation-policies',
  readPermission: 'alerts.escalations.read',
  managePermission: 'alerts.escalations.manage',
  queryKey: ['escalation-policies'],
  placeholder: '{"format": "molesignal.escalation-policies/v1", "policies": []}',
});

export const SCHEDULES = bundleKind('/schedules/export', '/schedules/import', 'schedules', {
  testPrefix: 'schedule',
  texts: 'transfer.schedules',
  fileName: 'molesignal-schedules',
  readPermission: 'schedules.read',
  managePermission: 'schedules.create',
  queryKey: ['schedules'],
  placeholder: '{"format": "molesignal.schedules/v1", "schedules": []}',
  migrationSources: SCHEDULE_SOURCES,
});

export const ROUTING_RULES = bundleKind('/alerts/routing/rules/export', '/alerts/routing/rules/import', 'rules', {
  testPrefix: 'routing',
  texts: 'transfer.routing',
  fileName: 'molesignal-routing-rules',
  readPermission: 'alerts.routing.read',
  managePermission: 'alerts.routing.manage',
  queryKey: ['alert-routing'],
  placeholder: '{"format": "molesignal.routing-rules/v1", "rules": []}',
});
