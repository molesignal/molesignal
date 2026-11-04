import { useQuery } from '@tanstack/react-query';
import { Inbox } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as alertSourcesApi from '@/api/alertSources';
import { formatMicrosActive } from '@/lib/time';
import { distinctLabels } from '@/routes/alerts/sources/model';
import { cn } from '@/shell/lib/cn';
import type { Incident } from '@/types/alerting';

import { externalSourceOf } from './origin';
import { IncidentSection } from './Section';

/** The query an incident's source side is read under; the detail page reads the same. */
export function incidentSourceKey(incidentId: string) {
  return ['incidents', incidentId, 'source'] as const;
}

/**
 * What the sender said about an incident it opened: which source it came
 * through, the key it is known by there, when the sender last spoke of it and,
 * for a notification group (Alertmanager, Grafana), each alert in the group
 * with what tells it apart from the others.
 */
export function IncidentExternalAlert({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const external = externalSourceOf(incident);

  const query = useQuery({
    queryKey: incidentSourceKey(incident.id),
    queryFn: () => alertSourcesApi.incidentSource(incident.id),
    enabled: Boolean(external),
    staleTime: 15_000,
    retry: false,
  });

  if (!external || !query.data) return null;

  const { source, dedupKey: dedup_key, lastIntakeAt: last_intake_at, members } = query.data;
  const telling = distinctLabels(members);

  return (
    <IncidentSection title={t('alert_sources.incident.title')} icon={<Inbox className="h-3.5 w-3.5" />} surface={surface}>
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 font-sans text-xs">
        <dt className="text-tx-2">{t('alert_sources.incident.source')}</dt>
        <dd className="min-w-0 truncate text-tx-0">
          {source ? (
            <Link
              to={`/alerts/sources/${encodeURIComponent(source.id)}`}
              className="underline-offset-4 outline-none hover:underline focus-visible:underline"
            >
              {source.name}
            </Link>
          ) : (
            <span className="text-tx-2">{t('alert_sources.incident.deleted')}</span>
          )}
          <span className="ml-2 text-tx-2">{t(`alert_sources.kinds.${external.kind}`, { defaultValue: external.kind })}</span>
        </dd>
        <dt className="text-tx-2">{t('alert_sources.incident.dedup_key')}</dt>
        <dd className="min-w-0 break-all font-mono text-tx-1">{dedup_key}</dd>
        <dt className="text-tx-2">{t('alert_sources.incident.last_intake')}</dt>
        <dd className="tabular-nums text-tx-1">{formatMicrosActive(last_intake_at)}</dd>
      </dl>
      {members.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <div className="font-sans text-xs text-tx-2">
            {t('alert_sources.incident.members', { count: members.length })}
          </div>
          <ul data-incident-members className="flex flex-col rounded-md bg-bg-2 px-3 py-1 font-sans text-xs">
            {members.map((member, index) => (
              <li
                key={index}
                className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5 border-b border-bd-0 py-1.5 last:border-b-0"
              >
                <span
                  className={cn(
                    'shrink-0',
                    member.status === 'firing' ? 'text-[var(--red-ink)]' : 'text-[var(--green-ink)]',
                  )}
                >
                  {t(`alert_sources.incident.member_${member.status}`)}
                </span>
                <span className="min-w-0 truncate text-tx-0">
                  {member.summary ?? member.labels.alertname ?? '—'}
                </span>
                {telling.length > 0 && (
                  <span className="min-w-0 truncate font-mono text-tx-2">
                    {telling.map((key) => `${key}=${member.labels[key] ?? ''}`).join(' · ')}
                  </span>
                )}
                {member.startsAt && (
                  <span className="ml-auto shrink-0 tabular-nums text-tx-2">
                    {t('alert_sources.incident.since', { time: formatMicrosActive(member.startsAt) })}
                  </span>
                )}
                {member.url && (
                  <a
                    href={member.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="shrink-0 text-indigo-soft underline-offset-4 outline-none hover:underline focus-visible:underline"
                  >
                    {t('alert_sources.incident.open_sender')}
                  </a>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </IncidentSection>
  );
}
