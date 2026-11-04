import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { AlertSource, IntakeRequest } from '@/api/alertSources';
import { Readout } from '@/shell/readout/Readout';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import { formatRelativeMicros } from '@/time/relative';
import type { Incident } from '@/types/alerting';

import { activeIncidentsOf, dayFigures, sourceState, type SourceState } from './model';

const STATE_TONE: Record<SourceState, string> = {
  off: 'var(--tx-3)',
  // Listening: the page asks again every few seconds.
  waiting: 'var(--blue)',
  quiet: 'var(--tx-3)',
  refused: 'var(--red)',
  receiving: 'var(--green)',
};

const STATE_TEXT: Partial<Record<SourceState, string>> = {
  refused: 'text-red',
  receiving: 'text-green',
};

/**
 * How one alert source stands: receiving, refused the last time, or still
 * waiting for its sender, with how its alerts are handled under it. Beside it
 * are what the last day brought: requests, the alerts in them, and the
 * incidents still open. It is the page's opening, so nothing in it is said
 * again below; its kind is in the page's header.
 */
export function SourceStanding({
  source,
  requests,
  requestLimit,
  incidents,
  policyName,
  nowMicros,
}: {
  source: AlertSource;
  /** The latest requests, newest first. */
  requests: readonly IntakeRequest[];
  /** How many requests were asked for: a full page means the day may hold more. */
  requestLimit: number;
  incidents: readonly Incident[];
  policyName: string;
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('alerts');
  const { state, latest } = sourceState(source, requests);
  const figures = dayFigures(requests, requestLimit, nowMicros);
  const open = activeIncidentsOf(source.id, incidents);
  const when = (micros: number) => formatRelativeMicros(micros, i18n.language, nowMicros);
  const labels = Object.entries(source.labels).map(([key, value]) => `${key}=${value}`);

  const detail: Record<SourceState, string | undefined> = {
    off: t('alert_sources.standing.off_detail'),
    waiting: t('alert_sources.standing.waiting_detail'),
    quiet: source.lastIntakeAt
      ? t('alert_sources.standing.last_received', { when: when(source.lastIntakeAt) })
      : undefined,
    refused: latest
      ? t('alert_sources.standing.refused_detail', {
          when: when(latest.receivedAt),
          reason: latest.message ?? `HTTP ${latest.httpStatus}`,
        })
      : undefined,
    receiving: latest ? t('alert_sources.standing.last_received', { when: when(latest.receivedAt) }) : undefined,
  };

  // Before the first request there is nothing to count: the verdict stands alone.
  const readouts =
    state === 'waiting' ? undefined : (
      <>
        <Readout
          className={STATEMENT_CELL}
          riseIndex={1}
          label={t('alert_sources.standing.requests')}
          value={figures.capped ? `${figures.requests}+` : String(figures.requests)}
          trend={figures.requests > 0 ? { values: figures.hourly, variant: 'bars', zeroBased: true } : undefined}
          detail={
            figures.refused > 0
              ? t('alert_sources.standing.refused_count', { count: figures.refused })
              : figures.requests > 0
                ? t('alert_sources.standing.none_refused')
                : t('alert_sources.standing.no_requests')
          }
        />
        <Readout
          className={STATEMENT_CELL}
          riseIndex={2}
          label={t('alert_sources.standing.alerts')}
          value={String(figures.alerts)}
          detail={t('alert_sources.standing.alert_outcomes', { opened: figures.opened, resolved: figures.resolved })}
        />
        <Readout
          className={STATEMENT_CELL}
          riseIndex={3}
          label={t('alert_sources.standing.open_incidents')}
          value={String(open.length)}
          tone={open.length > 0 ? 'danger' : undefined}
          detail={
            open[0]
              ? t('alert_sources.standing.oldest_open', { when: when(open[0].createdAt) })
              : t('alert_sources.standing.none_open')
          }
        />
      </>
    );

  return (
    <Statement
      label={t('alert_sources.standing.label')}
      tone={STATE_TONE[state]}
      title={
        state === 'waiting' ? (
          <span className="inline-flex items-center gap-3">
            <span aria-hidden className="ms-live h-2.5 w-2.5 shrink-0 rounded-full bg-[var(--blue)]" />
            {t('alert_sources.standing.waiting')}
          </span>
        ) : (
          t(`alert_sources.standing.${state}`)
        )
      }
      titleClassName={STATE_TEXT[state]}
      detail={detail[state]}
      dataAttributes={{ 'data-source-standing': state }}
      readouts={readouts}
    >
      {/* How its alerts are handled: settings, under the verdict, not figures beside it. */}
      <dl className="type-data m-0 mt-3 flex min-w-0 flex-wrap gap-x-4 gap-y-1">
        <Setting label={t('alert_sources.detail.meta.policy')}>{policyName}</Setting>
        <Setting label={t('alert_sources.detail.meta.severity')}>{t(`severity.${source.severity}`)}</Setting>
        {labels.length > 0 && <Setting label={t('alert_sources.detail.meta.labels')}>{labels.join(', ')}</Setting>}
        {source.homeCluster && (
          <Setting label={t('alert_sources.detail.meta.cluster')}>{source.homeCluster}</Setting>
        )}
      </dl>
    </Statement>
  );
}

function Setting({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 gap-1.5">
      <dt className="shrink-0 font-medium text-tx-2">{label}</dt>
      <dd className="m-0 min-w-0 truncate text-tx-1">{children}</dd>
    </div>
  );
}
