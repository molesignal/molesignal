import { useQuery } from '@tanstack/react-query';
import { GitCommitHorizontal, History } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import type { IncidentChange, SimilarIncident } from '@/api/incidents';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { useUsers } from '@/shell/useUsers';
import type { Incident } from '@/types/alerting';

import { courseDuration } from './Course';
import { IncidentSection } from './Section';

type Translate = ReturnType<typeof useTranslation<'alerts'>>['t'];

const SEVERITY_INK: Record<SimilarIncident['severity'], string> = {
  critical: 'text-red',
  error: 'text-orange',
  warning: 'text-yellow',
  info: 'text-blue',
};

/** Past incidents like this one: what they were, how they ended and what was found then. */
export function IncidentSimilar({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const { nameOf } = useUsers();
  const similar = useQuery({
    queryKey: ['incident-similar', incident.id],
    queryFn: () => incidentsApi.similar(incident.id),
    staleTime: 60_000,
    retry: false,
  });

  return (
    <IncidentSection title={t('drawer.incident_sections.similar')} icon={<History className="h-3.5 w-3.5" />} surface={surface}>
      {similar.isLoading ? (
        <p className="font-sans text-xs text-tx-3">{t('related.loading')}</p>
      ) : !Array.isArray(similar.data) || similar.data.length === 0 ? (
        <p className="font-sans text-xs text-tx-2">{t('related.similar_none')}</p>
      ) : (
        <ul data-incident-similar className="flex flex-col gap-3 font-sans text-xs">
          {similar.data.map((item) => (
            <li key={item.id} data-similar-incident={item.id} className="min-w-0">
              <div className="flex min-w-0 items-baseline gap-2">
                <span className={cn('shrink-0 font-strong', SEVERITY_INK[item.severity])}>
                  {t(`history.severity.${item.severity}`)}
                </span>
                <Link
                  to={`/alerts/incidents/${encodeURIComponent(item.id)}`}
                  className="min-w-0 truncate text-sm text-tx-0 underline-offset-2 hover:underline"
                >
                  {item.summary}
                </Link>
              </div>
              <p className="mt-0.5 text-tx-2">
                {[
                  formatMicrosActive(item.createdAt, false),
                  item.resolvedAt
                    ? t('related.lasted', { duration: courseDuration(item.resolvedAt - item.createdAt) })
                    : t(`history.status.${item.status}`, { defaultValue: item.status }),
                  item.resolvedBy ? t('related.resolved_by', { name: nameOf(item.resolvedBy) }) : null,
                  item.notes > 0 ? t('related.notes', { count: item.notes }) : null,
                  item.reasons.map((reason) => t(`related.reasons.${reason}`, { defaultValue: reason })).join(t('routing.list_separator')),
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </p>
              {item.rootCause && <p className="mt-1 line-clamp-2 text-tx-1">{item.rootCause}</p>}
            </li>
          ))}
        </ul>
      )}
    </IncidentSection>
  );
}

/** What changed near the trigger: deployments, configuration and flags of its services and those below. */
export function IncidentChanges({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const changes = useQuery({
    queryKey: ['incident-changes', incident.id],
    queryFn: () => incidentsApi.changes(incident.id),
    staleTime: 60_000,
    retry: false,
  });
  const list = changes.data?.changes ?? [];

  return (
    <IncidentSection
      title={t('drawer.incident_sections.changes')}
      icon={<GitCommitHorizontal className="h-3.5 w-3.5" />}
      surface={surface}
    >
      {changes.isLoading ? (
        <p className="font-sans text-xs text-tx-3">{t('related.loading')}</p>
      ) : list.length === 0 ? (
        <p className="font-sans text-xs text-tx-2">{t('related.changes_none')}</p>
      ) : (
        <ul data-incident-changes className="flex flex-col gap-2.5 font-sans text-xs">
          {list.map((change, index) => (
            <ChangeRow key={`${change.kind}-${change.at}-${index}`} change={change} triggeredAt={incident.createdAt} t={t} />
          ))}
        </ul>
      )}
    </IncidentSection>
  );
}

function ChangeRow({ change, triggeredAt, t }: { change: IncidentChange; triggeredAt: number; t: Translate }) {
  const offset = change.at - triggeredAt;
  const when =
    Math.abs(offset) < 60_000_000
      ? t('related.at_trigger')
      : t(offset < 0 ? 'related.before_trigger' : 'related.after_trigger', {
          duration: courseDuration(Math.abs(offset)),
        });
  const failed = change.status === 'failed' || change.status === 'rolled_back';

  return (
    <li data-incident-change={change.kind} className="grid grid-cols-[88px_minmax(0,1fr)] gap-x-3">
      <span className="tabular-nums text-tx-2">{when}</span>
      <span className="min-w-0">
        <span className="font-strong text-tx-0">{t(`related.kinds.${change.kind}`)}</span>{' '}
        {change.kind === 'rule' ? (
          <Link
            to={`/alerts/rules/${encodeURIComponent(change.ruleId ?? '')}/edit`}
            className="text-tx-1 underline-offset-2 hover:underline"
          >
            {change.ruleName}
          </Link>
        ) : (
          <span className="text-tx-1">
            {[change.service, change.version].filter(Boolean).join(' ')}
          </span>
        )}
        {change.downstream && <span className="ml-1.5 text-tx-2">{t('related.downstream')}</span>}
        {failed && <span className="ml-1.5 text-red">{t(`related.status.${change.status}`)}</span>}
        {(change.author || change.description || change.link) && (
          <span className="mt-0.5 block truncate text-tx-2">
            {[change.author, change.description].filter(Boolean).join(' · ')}
            {change.link && (
              <a
                href={change.link}
                target="_blank"
                rel="noreferrer"
                className="ml-1.5 text-indigo-soft underline-offset-2 hover:underline"
              >
                {t('related.open_link')}
              </a>
            )}
          </span>
        )}
      </span>
    </li>
  );
}
