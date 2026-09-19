import type {
  ActiveMonitorRevision,
  CreateMonitorInput,
  MonitorDetail,
  MonitorRevision,
} from '@/api/synthetics';

export type CheckEditorMode = 'create' | 'edit' | 'clone';

interface DraftApi {
  createMonitor(input: CreateMonitorInput): Promise<ActiveMonitorRevision>;
  createRevision(monitorId: string, input: CreateMonitorInput): Promise<MonitorRevision>;
}

export async function saveCheckDraft(
  api: DraftApi,
  mode: CheckEditorMode,
  detail: MonitorDetail | undefined,
  input: CreateMonitorInput,
): Promise<string> {
  if (mode === 'edit' && detail) {
    await api.createRevision(detail.monitor.id, input);
    return detail.monitor.id;
  }

  const created = await api.createMonitor(input);
  return created.monitor.id;
}
