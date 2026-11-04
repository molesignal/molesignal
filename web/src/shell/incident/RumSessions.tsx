import { useQuery } from '@tanstack/react-query';
import { MonitorSmartphone } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';
import { formatMicrosActive } from '@/lib/time';
import { hasPermission, useProductAccess } from '@/product/access';
import { formatFigure } from '@/routes/rum/alerts/model';
import {
  incidentFigure,
  readSession,
  sampleSessionPath,
  type SessionSample,
  sessionSamples,
  sessionsQuery,
} from '@/routes/rum/alerts/samples';
import { formatClock } from '@/routes/rum/replay/clock';
import { useAuthStore } from '@/stores/auth';
import type { AlertRule, Incident, RumAlertMetric } from '@/types/alerting';

import { IncidentSection } from './Section';

type Translate = ReturnType<typeof useTranslation<'alerts'>>['t'];

const VITAL_CAUSE: Partial<Record<RumAlertMetric, 'lcp' | 'inp' | 'cls' | 'ttfb'>> = {
  lcp_p75: 'lcp',
  inp_p75: 'inp',
  cls_p75: 'cls',
  ttfb_p75: 'ttfb',
};

/** What made a session a sample, in a few words: the figure in it and what went with it. */
function causeOf(metric: RumAlertMetric | null, sample: SessionSample, t: Translate, language: string): string | null {
  const value = sample.value;
  const vital = metric ? VITAL_CAUSE[metric] : undefined;

  if (metric && vital) {
    return (
      [
        value === null ? null : t(`drawer.rum.cause.${vital}`, { value: formatFigure(metric, value, language) }),
        sample.detail ? t('drawer.rum.element', { element: sample.detail }) : null,
      ]
        .filter(Boolean)
        .join(' · ') || null
    );
  }

  if (metric === 'crash_free_sessions') return value === null ? null : t('drawer.rum.crashes', { count: value });

  if (metric === 'frustration_rate') return value === null ? null : t('drawer.rum.signs', { count: value });

  if (metric === 'failed_request_rate') {
    if (value === null) return sample.detail;

    const status = value === 0 ? t('drawer.rum.no_answer') : String(value);

    return sample.detail ? t('drawer.rum.cause.request', { request: sample.detail, status }) : status;
  }

  // An error is told by what it said.
  return sample.detail;
}

/**
 * The sessions an incident of a RUM rule was told by when it opened: when, who, in what, and what
 * made each of them a sample. Each opens at the moment before it when it was recorded. The incident
 * keeps the sessions and the moments; who they were and whether they were recorded is read from the
 * sessions, by those who may read RUM, and the rows stand without it.
 */
export function IncidentRumSessions({
  incident,
  rule,
  surface,
}: {
  incident: Incident;
  rule?: AlertRule | undefined;
  surface: boolean;
}) {
  const { t, i18n } = useTranslation('alerts');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useProductAccess();
  const readable = hasPermission('rum.sessions.read', access) || hasPermission('sys.telemetry.read', access);
  const figure = incidentFigure(incident, rule);
  const samples = React.useMemo(() => sessionSamples(incident), [incident]);

  const sessions = useQuery({
    queryKey: ['rum', 'alerts', 'sample-sessions', orgId, incident.id, samples.map((sample) => sample.session).join(',')],
    queryFn: () => rumApi.search(sessionsQuery(samples)),
    enabled: Boolean(orgId && figure && samples.length > 0 && readable),
    staleTime: 60_000,
    retry: false,
  });

  // A part comes when there is something to show: none for a rule of traffic, or an incident without samples.
  if (!figure || samples.length === 0) return null;

  const rows = new Map<string, RumRow>((sessions.data?.items ?? []).map((row) => [String(row['session.id']), row]));

  return (
    <IncidentSection title={t('drawer.incident_sections.rum_sessions')} icon={<MonitorSmartphone className="h-3.5 w-3.5" />} surface={surface}>
      {figure.metric && <p className="m-0 font-sans text-xs text-tx-3">{t(`drawer.rum.why.${figure.metric}`)}</p>}
      <ol data-incident-rum-sessions="" className="m-0 flex list-none flex-col p-0">
        {samples.map((sample) => {
          const person = readSession(rows.get(sample.session));
          const open = sampleSessionPath(sample, person);
          const cause = causeOf(figure.metric, sample, t, language);
          const who = person.user ? (person.anonymous ? t('drawer.rum.anonymous', { id: person.user.slice(-4) }) : person.user) : null;
          const where = [person.browser, person.system].filter(Boolean).join(' · ');

          return (
            <li
              key={sample.session}
              data-incident-rum-session={sample.session}
              className="flex items-center gap-3 border-t border-bd-0 py-2.5 first:border-t-0 first:pt-0"
            >
              <span className="min-w-0 flex-1 font-sans text-xs">
                <span className="flex flex-wrap items-baseline gap-x-2.5">
                  <span className="font-strong text-tx-0">{[formatMicrosActive(sample.at, false), who].filter(Boolean).join(' · ')}</span>
                  {where && <span className="text-tx-3">{where}</span>}
                </span>
                {cause && <span className="block truncate text-tx-1">{cause}</span>}
              </span>
              <Link
                to={open.to}
                data-incident-rum-replay={person.replay ? '' : undefined}
                className="inline-flex h-6 shrink-0 items-center rounded-md border-0 bg-[var(--control-surface)] px-2 font-sans text-xs font-strong text-tx-1 no-underline outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
              >
                {person.replay ? (open.offset === null ? t('drawer.rum.replay') : t('drawer.rum.replay_at', { time: formatClock(open.offset) })) : t('drawer.rum.session')}
              </Link>
            </li>
          );
        })}
      </ol>
      {sessions.isError && <p className="m-0 font-sans text-xs text-tx-3">{t('drawer.rum.sessions_failed')}</p>}
    </IncidentSection>
  );
}
