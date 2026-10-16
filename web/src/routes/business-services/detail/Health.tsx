import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { BusinessService, BusinessServiceDependencyKind } from '@/api/businessServices';
import { Pill } from '@/shell/chrome';
import { formatRelativeMicros } from '@/time/relative';

import { STATUS_TONE, dependencyHref } from '../model';
import { ServiceSection } from '../Section';
import { Timeline } from '../Timeline';

/**
 * How the service is doing and why: its status since it last changed, the
 * dependencies that are not well with the alert incidents behind them, and
 * the last seven days.
 */
export function ServiceHealth({
  service,
  label,
  nowMicros,
}: {
  service: BusinessService;
  label: (kind: BusinessServiceDependencyKind, target: string) => string;
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('business-services');
  const { health } = service;

  return (
    <ServiceSection
      title={t('detail.health')}
      description={
        health.changed_at
          ? t('detail.health_since', {
              status: t(`statuses.${health.status}`),
              time: formatRelativeMicros(health.changed_at, i18n.language, nowMicros),
            })
          : t('detail.not_evaluated')
      }
      action={<Pill tone={STATUS_TONE[health.status]}>{t(`statuses.${health.status}`)}</Pill>}
    >
      <div className="space-y-4 px-4 pb-4">
        {health.causes.length === 0 ? (
          <p className="text-xs text-tx-2">
            {health.evaluated_at ? t('detail.no_causes') : t('detail.not_evaluated_hint')}
          </p>
        ) : (
          <ul className="divide-y divide-bd-0 rounded-md bg-bg-1">
            {health.causes.map((cause) => (
              <li key={`${cause.kind}:${cause.target}`} className="px-3 py-2.5">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="text-xs text-tx-3">{t(`dependency_kinds.${cause.kind}`)}</span>
                  <Link
                    to={dependencyHref(cause.kind, cause.target)}
                    className="min-w-0 truncate text-sm font-strong text-tx-0 hover:underline"
                  >
                    {label(cause.kind, cause.target)}
                  </Link>
                  <Pill tone={STATUS_TONE[cause.status]}>{t(`statuses.${cause.status}`)}</Pill>
                  {cause.monitor_state && (
                    <span className="text-xs text-tx-2">{t(`monitor_states.${cause.monitor_state}`)}</span>
                  )}
                </div>
                {(cause.incidents ?? []).length > 0 && (
                  <ul className="mt-1.5 space-y-1">
                    {(cause.incidents ?? []).map((incident) => (
                      <li key={incident.id} className="flex min-w-0 items-center gap-2 text-xs">
                        <span className="shrink-0 text-tx-3">{t(`severities.${incident.severity}`)}</span>
                        <Link
                          to={`/alerts/incidents/${encodeURIComponent(incident.id)}`}
                          className="min-w-0 truncate text-tx-1 hover:underline"
                        >
                          {incident.summary}
                        </Link>
                      </li>
                    ))}
                    {(cause.incident_count ?? 0) > (cause.incidents ?? []).length && (
                      <li className="text-xs text-tx-3">
                        {t('detail.more_incidents', {
                          count: (cause.incident_count ?? 0) - (cause.incidents ?? []).length,
                        })}
                      </li>
                    )}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        )}
        <Timeline history={health.history} nowMicros={nowMicros} />
      </div>
    </ServiceSection>
  );
}
