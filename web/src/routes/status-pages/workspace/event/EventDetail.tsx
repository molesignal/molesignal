import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { StatusPageIncident } from '@/api/statusPages';
import { Pill } from '@/shell/chrome';

import {
  INCIDENT_TONE,
  affectedComponentNames,
  formatMicros,
  impactLabel,
  incidentStatusLabel,
} from '../../model';
import { useStatusPageWorkspace } from '../Layout';
import { promisedNextUpdate } from './draft';

/** A published event as it stands: its facts and every update said about it. */
export function EventDetail({ event }: { event: StatusPageIncident }) {
  const { t, i18n } = useTranslation('status-pages');
  const { snapshot } = useStatusPageWorkspace();
  const components = affectedComponentNames(event, snapshot.components);
  const timezone = snapshot.page.timezone;
  const nextUpdate = promisedNextUpdate(event);

  return (
    <div className="space-y-7">
      <dl className="grid grid-cols-2 gap-4 rounded-lg border border-bd-0 bg-bg-2 p-4 text-sm">
        <Detail label={t('fields.incident_status')}>
          <Pill tone={INCIDENT_TONE[event.status]}>{incidentStatusLabel(t, event.status, event.kind)}</Pill>
        </Detail>
        <Detail label={t('fields.impact')}>{impactLabel(t, event.impact)}</Detail>
        <Detail label={t('fields.starts_at')}>
          {formatMicros(event.started_at, timezone, i18n.language)}
        </Detail>
        <Detail label={t('fields.affected_components')}>
          {components.join(', ') || t('values.none')}
        </Detail>
        {nextUpdate !== null && (
          <Detail label={t('fields.next_update')}>
            {t('values.next_update_by', { time: formatMicros(nextUpdate, timezone, i18n.language) })}
          </Detail>
        )}
      </dl>
      <section>
        <h3 className="text-sm font-display-strong text-tx-0">{t('forms.event.timeline')}</h3>
        <div className="mt-3 border-l border-bd-1 pl-4">
          {event.updates.map((update) => (
            <article key={update.id} className="relative pb-6 last:pb-0">
              <span className="absolute -left-[19px] top-1.5 h-2 w-2 rounded-full bg-indigo" />
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs font-strong text-tx-1">
                  {incidentStatusLabel(t, update.status, event.kind)}
                </span>
                <time className="text-xs tabular-nums text-tx-3">
                  {formatMicros(update.created_at, timezone, i18n.language)}
                </time>
                {/* Telling subscribers is the rule; only an update that went out quietly is marked. */}
                {update.notify_subscribers === false && <Pill tone="dim">{t('values.not_notified')}</Pill>}
              </div>
              <p className="mt-1.5 whitespace-pre-wrap text-sm leading-6 text-tx-2">{update.message}</p>
              {update.next_update_at != null && (
                <p className="mt-1 text-xs text-tx-3">
                  {t('values.next_update_by', {
                    time: formatMicros(update.next_update_at, timezone, i18n.language),
                  })}
                </p>
              )}
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}

function Detail({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className="mt-1 text-tx-1">{children}</dd>
    </div>
  );
}
