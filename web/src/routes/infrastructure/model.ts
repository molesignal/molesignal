import type { ServiceIdentity } from '@/api/apm';
import type { Entity, EntityKind } from '@/api/entities';

/** Seen within two sweeps is running now; older is only what was last seen. */
export const CURRENT_MICROS = 30 * 60 * 1_000_000;
const HOUR_MICROS = 3_600_000_000;

/** The resource attribute each kind is told apart by in metrics. */
const NAMING_ATTRIBUTE: Record<EntityKind, string> = {
  cluster: 'k8s.cluster.name',
  node: 'k8s.node.name',
  host: 'host.name',
  pod: 'k8s.pod.name',
  container: 'container.id',
};

/** The page of one entity. */
export function entityHref(kind: EntityKind, key: string): string {
  return `/infrastructure/${kind}?${new URLSearchParams({ key })}`;
}

/** Whether an entity was seen lately enough to be running now. */
export function isCurrent(entity: Pick<Entity, 'lastSeenAt'>, now: number): boolean {
  return entity.lastSeenAt >= now - CURRENT_MICROS;
}

/**
 * The place a set of resource attributes runs on, keyed as the sweep keys it:
 * a pod as `cluster/namespace/pod`, a host outside Kubernetes by its name.
 * Inside a pod `host.name` names the pod, so a pod's attributes make no host.
 */
export function placeOf(attributes: Record<string, string | undefined>): { kind: EntityKind; key: string } | null {
  const value = (name: string) => attributes[name]?.trim() || undefined;
  const cluster = value('k8s.cluster.name') ?? '';
  const pod = value('k8s.pod.name');
  const node = value('k8s.node.name');

  if (pod) return { kind: 'pod', key: `${cluster}/${value('k8s.namespace.name') ?? ''}/${pod}` };
  if (node) return { kind: 'node', key: `${cluster}/${node}` };
  if (cluster) return null;

  const host = value('host.name');
  return host ? { kind: 'host', key: host } : null;
}

/** A service's APM page. */
export function serviceHref(service: ServiceIdentity): string {
  const params = new URLSearchParams({ namespace: service.namespace, environment: service.environment });
  return `/apm/services/${encodeURIComponent(service.name)}?${params}`;
}

/** The last hour around `now` as a page's `from` and `to`. */
function lastHour(now: number): Record<string, string> {
  return {
    from: new Date((now - HOUR_MICROS) / 1_000).toISOString(),
    to: new Date(now / 1_000).toISOString(),
  };
}

/** The entity's metrics over the last hour, by the attribute its kind is named with. */
export function metricsHref(entity: Pick<Entity, 'kind' | 'name' | 'key'>, now: number): string {
  const value = entity.kind === 'container' ? entity.key : entity.name;
  const promql = `{${JSON.stringify(NAMING_ATTRIBUTE[entity.kind])}=${JSON.stringify(value)}}`;
  return `/metrics?${new URLSearchParams({ promql, ...lastHour(now) })}`;
}

/** The entity's logs over the last hour: only pods and hosts are named in links to the logs. */
export function logsHref(entity: Pick<Entity, 'kind' | 'name'>, now: number): string | null {
  if (entity.kind === 'pod') return `/logs?${new URLSearchParams({ pod: entity.name, ...lastHour(now) })}`;
  if (entity.kind === 'host') return `/logs?${new URLSearchParams({ host: entity.name, ...lastHour(now) })}`;
  return null;
}

/** A service in a line: its name, and the namespace and environment when said. */
export function serviceLabel(service: ServiceIdentity): string {
  const where = [service.namespace, service.environment].filter(Boolean).join(' · ');
  return where ? `${service.name} (${where})` : service.name;
}
