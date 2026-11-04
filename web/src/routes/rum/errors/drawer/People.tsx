import { MonitorPlay, Users } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumIssueSample } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';

import { OverviewCard } from '../../overview/OverviewCard';
import type { Person } from '../model';

const BUTTON =
  'inline-flex h-[26px] shrink-0 items-center rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3';

/** How an anonymous visitor is named: the start of the id the SDK keeps for them. */
const ANONYMOUS_SHOWN = 8;

const sessionHref = (id: string) => `/rum/sessions/view/${encodeURIComponent(id)}`;

/**
 * The latest errors of an issue, each with the session it happened in: who met it, where, and the
 * way into that session's replay and timeline.
 */
export function SamplesCard({ samples }: { samples: readonly RumIssueSample[] }) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();

  return (
    <OverviewCard
      icon={MonitorPlay}
      tone="text-[var(--chart-3)]"
      title={t('errors.drawer.samples.title')}
      description={t('errors.drawer.samples.description')}
      testId="issue-samples"
      className="min-w-0 flex-[999_1_460px]"
    >
      {samples.length === 0 ? (
        <p className="m-0 px-3 py-3 font-sans text-sm text-tx-3">{t('errors.drawer.samples.empty')}</p>
      ) : (
        <ul data-rum-samples="" className="m-0 list-none p-0">
          {samples.map((sample, index) => {
            const who = sample.userId
              ? sample.userId
              : sample.anonymousId
                ? t('errors.drawer.anonymous', { id: sample.anonymousId.slice(0, ANONYMOUS_SHOWN) })
                : t('errors.drawer.unknown');
            const where = [sample.page, sample.browser, sample.system, sample.version].filter(Boolean).join(' · ');

            return (
              <li key={`${sample.sessionId ?? 'none'}:${index}`} data-rum-sample="" className="flex items-center gap-3 border-b border-[var(--row-rule)] px-3 py-2 font-sans">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-strong text-tx-0">
                    {formatter.micros(sample.timestamp)} · {who}
                  </span>
                  <span className="block truncate text-xs text-tx-3">{where}</span>
                </span>
                {sample.sessionId && !sample.retained && (
                  <span data-rum-session-not-kept="" className="shrink-0 text-xs text-tx-3">
                    {t('errors.drawer.samples.not_kept')}
                  </span>
                )}
                {sample.sessionId && sample.retained && sample.hasReplay && (
                  <Link to={sessionHref(sample.sessionId)} className={BUTTON}>
                    {t('errors.drawer.samples.replay')}
                  </Link>
                )}
                {sample.sessionId && sample.retained && (
                  <Link to={sessionHref(sample.sessionId)} className={BUTTON}>
                    {t('errors.drawer.samples.timeline')}
                  </Link>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </OverviewCard>
  );
}

/** The people an issue met most, by how often. */
export function PeopleCard({ people }: { people: readonly Person[] }) {
  const { t } = useTranslation('rum');

  return (
    <OverviewCard
      icon={Users}
      tone="text-[var(--chart-4)]"
      title={t('errors.drawer.people.title')}
      description={t('errors.drawer.people.description')}
      testId="issue-people"
      className="min-w-0 flex-[1_1_260px]"
    >
      {people.length === 0 ? (
        <p className="m-0 px-3 py-3 font-sans text-sm text-tx-3">{t('errors.drawer.people.empty')}</p>
      ) : (
        <ul data-rum-people="" className="m-0 list-none p-0">
          {people.map((person) => (
            <li key={`${person.anonymous ? 'a' : 'u'}:${person.id}`} className="flex items-center justify-between gap-3 border-b border-[var(--row-rule)] px-3 py-2 font-sans text-sm">
              <span className="min-w-0 truncate font-strong text-tx-0">
                {person.anonymous ? t('errors.drawer.anonymous', { id: person.id.slice(0, ANONYMOUS_SHOWN) }) : person.id}
              </span>
              <span className="shrink-0 tabular-nums text-tx-1">{t('errors.drawer.people.times', { count: person.occurrences })}</span>
            </li>
          ))}
        </ul>
      )}
    </OverviewCard>
  );
}
