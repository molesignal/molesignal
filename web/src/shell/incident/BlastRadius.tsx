import { useQuery } from '@tanstack/react-query';
import { Network } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceHealth } from '@/api/apm';
import { apmInsightsApi, apmInsightsQueryKeys, type BlastNode } from '@/api/apmInsights';
import { hasPermission, useProductAccess } from '@/product/access';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';
import type { Incident } from '@/types/alerting';

import { IncidentSection } from './Section';

/** How long before an incident opened its service's calls are read from. */
const LEAD_MICROS = 30 * 60_000_000;
/** The most nodes each side lists. */
const MOST = 6;
const SERVICE_LABELS = ['service.name', 'service_name', 'service', 'svc'] as const;

const HEALTH_TEXT: Record<ServiceHealth, string> = {
  critical: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  healthy: 'text-[var(--green-ink)]',
  no_traffic: 'text-tx-3',
};

/** The service an incident is about, from its labels or the services it says it touched. */
export function incidentServiceName(incident: Pick<Incident, 'labels' | 'affectedServices'>): string | null {
  for (const key of SERVICE_LABELS) {
    const value = incident.labels?.[key]?.trim();
    if (value) return value;
  }
  return incident.affectedServices?.find((name) => name.trim())?.trim() ?? null;
}

/**
 * What an incident's service reaches along the APM service map over the
 * incident's time: the services that call it, which its trouble reaches;
 * and what it calls, the unhealthy first, where the trouble may come from.
 */
export function IncidentBlastRadius({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useProductAccess();
  const service = incidentServiceName(incident);
  const environment = incident.labels?.['deployment.environment'] || incident.labels?.environment || undefined;
  const readable = hasPermission('apm.services.read', access) || hasPermission('sys.telemetry.read', access);
  const params = {
    service: service ?? '',
    ...(environment ? { environment } : {}),
    from: incident.createdAt - LEAD_MICROS,
    to: incident.resolvedAt ?? Date.now() * 1_000,
  };
  const radius = useQuery({
    queryKey: apmInsightsQueryKeys.blastRadius(orgId, params),
    queryFn: () => apmInsightsApi.blastRadius(params),
    enabled: Boolean(orgId && service && readable),
    retry: false,
  });

  // Shown once there is something to say: no section comes and goes while the map is read.
  if (!service || !readable || !radius.data) return null;

  const data = radius.data;
  const mapHref = `/apm/map?${new URLSearchParams({ ...(environment ? { environment } : {}), node: `service:${service}` })}`;

  return (
    <IncidentSection title={t('drawer.incident_sections.blast_radius')} icon={<Network className="h-3.5 w-3.5" />} surface={surface}>
      {!data.service ? (
        <p className="font-sans text-xs text-tx-3">{t('drawer.blast.off_map', { service })}</p>
      ) : (
        <div data-incident-blast-radius="" className="space-y-3 font-sans text-xs">
          <Side title={t('drawer.blast.upstream')} empty={t('drawer.blast.no_upstream')} nodes={data.upstream} environment={environment} />
          <Side title={t('drawer.blast.downstream')} empty={t('drawer.blast.no_downstream')} nodes={data.downstream} environment={environment} />
          <Link to={mapHref} className="inline-flex font-strong text-indigo-soft hover:underline focus-visible:underline focus-visible:outline-none">
            {t('drawer.blast.open_map')}
          </Link>
        </div>
      )}
    </IncidentSection>
  );
}

function Side({ title, empty, nodes, environment }: { title: string; empty: string; nodes: readonly BlastNode[]; environment: string | undefined }) {
  const { t } = useTranslation('alerts');

  return (
    <div className="space-y-1">
      <p className="font-strong text-tx-1">{title}</p>
      {nodes.length === 0 ? (
        <p className="text-tx-3">{empty}</p>
      ) : (
        <ul className="space-y-1">
          {nodes.slice(0, MOST).map((node) => (
            <li key={node.id} data-blast-node={node.id} className="flex items-center gap-2">
              <span className={cn('w-12 shrink-0 font-strong', HEALTH_TEXT[node.health])}>{t(`drawer.blast.health.${node.health}`)}</span>
              {node.kind === 'service' ? (
                <Link
                  to={`/apm/services/${encodeURIComponent(node.name)}?${new URLSearchParams({ ...(node.namespace ? { namespace: node.namespace } : {}), ...(environment ? { environment } : {}) })}`}
                  className="min-w-0 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none"
                >
                  {node.name}
                </Link>
              ) : (
                <span className="min-w-0 truncate font-strong text-tx-0">{node.name}</span>
              )}
              <span className="shrink-0 text-tx-3">{node.hops === 1 ? t('drawer.blast.direct') : t('drawer.blast.hops', { count: node.hops })}</span>
              <span className={cn('ml-auto shrink-0 tabular-nums', node.call.errorRate >= 0.02 ? 'text-[var(--red-ink)]' : 'text-tx-2')}>
                {t('drawer.blast.calls', {
                  count: node.call.requestCount,
                  formatted: node.call.requestCount.toLocaleString(),
                  rate: `${(node.call.errorRate * 100).toFixed(node.call.errorRate >= 0.1 ? 1 : 2)}%`,
                })}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
