import { BellRing, Rocket } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { VersionSummary } from '@/api/apm';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';
import type { Severity } from '@/types/alerting';

import { useActiveIncidents } from '../frame/alerts';
import { ApmCard } from '../frame/Page';
import { VerdictWord } from '../frame/VersionVerdict';
import { servicePath } from '../scope';
import { serviceIncidents, serviceRowKey } from './model';

const ROW = 'grid h-8 items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0';
const LINK_ROW = `${ROW} hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none`;

const SEVERITY_TEXT: Record<Severity, string> = {
  critical: 'text-[var(--red-ink)]',
  error: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  info: 'text-[var(--blue-ink)]',
};

/** How many incidents the card lists. */
const ALERT_LIMIT = 8;

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{children}</p>;
}

/**
 * The alerts going on that name a service, the most severe first: what the
 * Alerts module is telling about the services, read where they are watched.
 */
export function ActiveAlertsCard() {
  const { t, i18n } = useTranslation('apm');
  const { t: tAlerts } = useTranslation('alerts');
  const incidents = useActiveIncidents();
  const listed = serviceIncidents(incidents.data ?? []);
  const denied = incidents.isError && toApiError(incidents.error).status === 403;

  return (
    <ApmCard
      icon={BellRing}
      tone="text-[var(--red-ink)]"
      title={t('landing.alerts')}
      testId="overview-alerts"
      actions={
        <Link to="/alerts/incidents" className="font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none">
          {t('actions.see_all')}
        </Link>
      }
    >
      {incidents.isPending ? (
        <Empty>{t('landing.alerts_loading')}</Empty>
      ) : denied ? (
        <Empty>{t('landing.alerts_denied')}</Empty>
      ) : incidents.isError ? (
        <Empty>{t('landing.alerts_unavailable')}</Empty>
      ) : listed.length === 0 ? (
        <Empty>{t('landing.no_alerts')}</Empty>
      ) : (
        listed.slice(0, ALERT_LIMIT).map(({ incident, service }) => (
          <Link
            key={incident.id}
            to={`/alerts/incidents/${encodeURIComponent(incident.id)}`}
            data-apm-alert={incident.id}
            className={cn(LINK_ROW, 'grid-cols-[52px_minmax(0,1fr)_minmax(0,140px)_112px]')}
          >
            <span className={cn('font-strong', SEVERITY_TEXT[incident.severity])}>{tAlerts(`severity.${incident.severity}`)}</span>
            <span className="truncate text-tx-0" title={incident.summary}>
              {incident.summary}
            </span>
            <span className="truncate text-tx-2">{service}</span>
            <span className="truncate whitespace-nowrap text-right tabular-nums text-tx-3">{formatRelativeMicros(incident.createdAt, i18n.language)}</span>
          </Link>
        ))
      )}
    </ApmCard>
  );
}

/** The versions first seen within the range: what was deployed, where and when, and those that did better or worse than the version before. */
export function DeploymentsCard({ deployments, searchParams }: { deployments: readonly VersionSummary[]; searchParams: URLSearchParams }) {
  const { t, i18n } = useTranslation('apm');

  return (
    <ApmCard icon={Rocket} tone="text-[var(--purple-ink)]" title={t('landing.deployments')} description={t('landing.deployments_description')} testId="overview-deployments">
      {deployments.length === 0 ? (
        <Empty>{t('landing.no_deployments')}</Empty>
      ) : (
        deployments.slice(0, 8).map((deployment) => (
          <Link
            key={`${serviceRowKey(deployment.service)}@${deployment.version}`}
            to={servicePath(deployment.service, 'deployments', searchParams)}
            data-apm-deployment={deployment.version}
            className={cn(LINK_ROW, 'grid-cols-[minmax(0,1fr)_minmax(0,140px)_112px]')}
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="truncate font-strong text-tx-0">{deployment.service.name}</span>
              <span className="truncate text-tx-3">{deployment.service.environment}</span>
            </span>
            <span className="flex min-w-0 items-center gap-1.5">
              <span className="truncate tabular-nums text-tx-1" title={deployment.version}>
                {deployment.version}
              </span>
              {deployment.verdict && (deployment.verdict.status === 'regressed' || deployment.verdict.status === 'improved') && (
                <VerdictWord verdict={deployment.verdict} className="shrink-0" />
              )}
            </span>
            <span className="truncate whitespace-nowrap text-right tabular-nums text-tx-3">{formatRelativeMicros(deployment.firstSeenAt, i18n.language)}</span>
          </Link>
        ))
      )}
    </ApmCard>
  );
}
