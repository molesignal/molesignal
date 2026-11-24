import { useQuery } from '@tanstack/react-query';
import { Briefcase } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { type BusinessKeyImpact, businessContextApi, businessContextQueryKeys } from '@/api/businessContext';
import { hasPermission, useProductAccess } from '@/product/access';
import { tracesHref } from '@/routes/apm/endpointFilters';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';
import type { Incident } from '@/types/alerting';

import { IncidentSection } from './Section';

/**
 * Who the incident touched in business terms: for each business key the
 * Spans carry, the values (tenants, plans) whose requests to its services
 * failed most around the time it began, those marked important flagged.
 * Shown to readers who may query Spans, once the organization has declared a
 * key and there is something to say.
 */
export function IncidentBusinessImpact({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useProductAccess();
  const readable = hasPermission('streams.query', access) || hasPermission('sys.telemetry.read', access);
  const impact = useQuery({
    queryKey: businessContextQueryKeys.incidentImpact(orgId, incident.id),
    queryFn: () => businessContextApi.incidentImpact(incident.id),
    enabled: Boolean(orgId) && readable,
    retry: false,
  });

  const keys = impact.data?.keys ?? [];

  if (!readable || !impact.data?.declared || keys.length === 0) return null;

  return (
    <IncidentSection title={t('drawer.incident_sections.business_impact')} icon={<Briefcase className="h-3.5 w-3.5" />} surface={surface}>
      <div data-incident-business-impact="" className="space-y-3 font-sans text-xs">
        {keys.map((key) => (
          <KeyValues key={key.key} impact={key} from={impact.data.from} to={impact.data.to} services={impact.data.services} />
        ))}
      </div>
    </IncidentSection>
  );
}

function KeyValues({ impact, from, to, services }: { impact: BusinessKeyImpact; from: number; to: number; services: string[] }) {
  const { t } = useTranslation('alerts');

  // The value's requests on the Traces page, split as the RED view there splits them.
  const href = (value: string) =>
    `${tracesHref(
      [
        { field: impact.field, op: '=', value },
        ...(services.length === 1 ? [{ field: 'service.name', op: '=' as const, value: services[0] ?? '' }] : []),
      ],
      { from, to },
    )}&tab=red`;

  return (
    <div data-business-key={impact.key} className="space-y-1">
      <p className="font-strong text-tx-1">{impact.label}</p>
      <ul className="space-y-1">
        {impact.values.map((value) => {
          const failed = value.requests > 0 ? value.errors / value.requests : 0;

          return (
            <li key={value.value} data-business-value={value.value} className="flex items-center gap-2">
              <Link
                to={href(value.value)}
                className="min-w-0 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none"
              >
                {value.value}
              </Link>
              {value.important && (
                <span className="shrink-0 rounded bg-[var(--yellow-tint)] px-1 text-[var(--yellow-ink)]">{t('drawer.business.important')}</span>
              )}
              <span className={cn('ml-auto shrink-0 tabular-nums', value.errors > 0 ? 'text-[var(--red-ink)]' : 'text-tx-2')}>
                {t('drawer.business.failed', {
                  errors: value.errors.toLocaleString(),
                  requests: value.requests.toLocaleString(),
                  rate: `${(failed * 100).toFixed(failed >= 0.1 ? 1 : 2)}%`,
                })}
              </span>
            </li>
          );
        })}
      </ul>
      {impact.more && <p className="text-tx-3">{t('drawer.business.more')}</p>}
    </div>
  );
}
