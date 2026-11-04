import type { SignalFilterHandle } from '@/api/apm';

/**
 * Another signal of the service in the same time: its logs, metrics or
 * profiles, each page narrowed to the service (and version) the handle
 * names, and to whatever else `extra` says (the stream logs were found in).
 */
export function signalHref(target: 'logs' | 'metrics' | 'profiles', handle: SignalFilterHandle, extra?: Record<string, string>): string {
  const params = new URLSearchParams({
    service: handle.service,
    namespace: handle.namespace,
    environment: handle.environment,
    from: new Date(handle.from / 1_000).toISOString(),
    to: new Date(handle.to / 1_000).toISOString(),
    ...extra,
  });

  if (handle.version) params.set('version', handle.version);
  if (handle.errorFingerprint) params.set('error', handle.errorFingerprint);

  return `/${target}?${params}`;
}

/** The logs of one trace, in the half hour either side of when it happened (`at`, in microseconds). */
export function traceLogsHref(traceId: string, at: number): string {
  const around = 30 * 60 * 1_000_000;
  const params = new URLSearchParams({
    traceId,
    from: new Date((at - around) / 1_000).toISOString(),
    to: new Date((at + around) / 1_000).toISOString(),
  });

  return `/logs?${params}`;
}

/** Where the service's documents are kept in the knowledge base: Mole Agent reads them too. */
export function serviceDocsFolder(name: string): string {
  return `services/${name}/`;
}

/** The knowledge page open on one document of a base. */
export function knowledgeHref(base: string, path: string): string {
  return `/agent/knowledge?${new URLSearchParams({ base, path })}`;
}
