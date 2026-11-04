import { useQuery } from '@tanstack/react-query';
import { BellPlus, ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import { useActionAccess } from '@/product/actionAccess';
import { apmNewAnomalyHref, apmNewRuleHref } from '@/routes/alerts/rule-workbench/apmSource';
import { cn } from '@/shell/lib/cn';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/shell/ui/dropdown-menu';
import type { ApmAlertMetric, Incident, Severity } from '@/types/alerting';

import { incidentService } from '../overview/model';

const SEVERITY_ORDER: Record<Severity, number> = { critical: 0, error: 1, warning: 2, info: 3 };

export const SEVERITY_TEXT: Record<Severity, string> = {
  critical: 'text-[var(--red-ink)]',
  error: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  info: 'text-[var(--blue-ink)]',
};

/** The incidents going on, read once for every APM page that shows them. */
export function useActiveIncidents() {
  return useQuery({
    queryKey: ['apm', 'incidents', 'active'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    refetchInterval: 60_000,
    retry: false,
  });
}

/** The incidents of `incidents` about a service, in its environment when they name one, and about one endpoint when asked. */
export function incidentsAbout(
  incidents: readonly Incident[] | undefined,
  service: { name: string; environment?: string | null },
  endpoint?: string,
): Incident[] {
  return (incidents ?? [])
    .filter((incident) => incident.status === 'open' || incident.status === 'acknowledged')
    .filter((incident) => incidentService(incident) === service.name)
    .filter((incident) => {
      const environment = incident.labels?.['deployment.environment'];
      return !environment || !service.environment || environment === service.environment;
    })
    .filter((incident) => !endpoint || incident.labels?.['apm.endpoint'] === endpoint)
    .sort((left, right) => SEVERITY_ORDER[left.severity] - SEVERITY_ORDER[right.severity] || right.createdAt - left.createdAt);
}

/** The most severe of incidents, which their count is coloured by. */
export function worstSeverity(incidents: readonly Incident[]): Severity | null {
  return incidents[0]?.severity ?? null;
}

/** How many alerts are going on about a service, as a word in the colour of the worst; nothing when none. */
export function AlertCount({ incidents }: { incidents: readonly Incident[] }) {
  const { t } = useTranslation('apm');
  const worst = worstSeverity(incidents);
  if (!worst) return null;

  return (
    <Link
      to="/alerts/incidents"
      data-apm-alert-count={incidents.length}
      title={incidents.map((incident) => incident.summary).join('\n')}
      className={cn('inline-flex items-center gap-1 font-sans text-xs font-strong hover:underline focus-visible:underline focus-visible:outline-none', SEVERITY_TEXT[worst])}
    >
      {t('alerts.active', { count: incidents.length })}
    </Link>
  );
}

/**
 * A new alert rule on an APM figure, with the service filled in: against a
 * threshold in the rule editor, or against the same time on the days
 * before in the anomaly editor.
 */
export function NewApmAlert({
  service,
  endpoint,
  metric,
}: {
  service: { namespace: string; name: string; environment: string };
  endpoint?: { name: string; kind: string } | null;
  metric?: ApmAlertMetric;
}) {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const access = useActionAccess({ permission: 'alerts.rules.manage' });
  if (access.disabled) return null;

  const source = { namespace: service.namespace, service: service.name, environment: service.environment, endpoint: endpoint ?? null, ...(metric ? { metric } : {}) };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-apm-new-alert=""
          className="inline-flex h-7 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3"
        >
          <BellPlus aria-hidden className="h-3.5 w-3.5 text-tx-3" />
          {t('alerts.new')}
          <ChevronDown aria-hidden className="h-3 w-3 text-tx-3" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-72">
        {(['threshold', 'anomaly'] as const).map((kind) => (
          <DropdownMenuItem
            key={kind}
            data-apm-new-alert-kind={kind}
            onSelect={() => navigate(kind === 'threshold' ? apmNewRuleHref(source) : apmNewAnomalyHref(source))}
            className="flex-col items-start gap-0.5 font-sans text-xs"
          >
            <span className="font-strong text-tx-0">{t(`alerts.kind.${kind}`)}</span>
            <span className="text-tx-3">{t(`alerts.kind.${kind}_hint`)}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
