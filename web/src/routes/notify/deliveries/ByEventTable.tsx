import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import type { NotifyConnector, NotifyDelivery } from '@/api/notify';
import { courseDuration } from '@/shell/incident/Course';
import { cn } from '@/shell/lib/cn';

import { connectorName, formatMicros } from '../model';
import { groupByEvent } from './byEvent';

function markClass(delivery: NotifyDelivery): string {
  if (delivery.status === 'failed') return 'bg-red';
  if (delivery.status === 'success' || delivery.status === 'acknowledged') return 'bg-green';
  if (delivery.status === 'skipped') return 'bg-transparent shadow-[inset_0_0_0_1px_var(--bd-2)]';
  return 'bg-yellow';
}

/**
 * Deliveries as a responder asks about them: for this alert, who was told through
 * what, how each try went and how long until the first one arrived. A mark is a
 * try, in order along its channel; pressing it opens that try.
 */
export function ByEventTable({
  rows,
  connectors,
  onOpen,
}: {
  rows: readonly NotifyDelivery[];
  connectors: readonly NotifyConnector[];
  onOpen: (delivery: NotifyDelivery) => void;
}) {
  const { t, i18n } = useTranslation('notify');

  return (
    <DataTable
      rows={groupByEvent(rows)}
      rowKey={(event) => event.eventId}
      columns={[
        {
          key: 'event',
          header: t('deliveries.columns.event'),
          width: '30%',
          sortable: true,
          sortValue: (event) => event.firstAt,
          cell: (event) => (
            <div className="min-w-0">
              <div className="truncate font-mono text-xs text-tx-1">{event.eventId}</div>
              <div className="text-xs text-tx-3">{formatMicros(event.firstAt, i18n.language)}</div>
            </div>
          ),
        },
        {
          key: 'channels',
          header: t('deliveries.by_event.channels'),
          cell: (event) => (
            <ul data-delivery-channels className="flex flex-wrap gap-x-5 gap-y-1">
              {event.channels.map((channel) => (
                <li key={channel.connectorId ?? 'none'} className="flex min-w-0 items-center gap-2">
                  <span className="truncate text-xs text-tx-2">
                    {connectorName([...connectors], channel.connectorId ?? null)}
                  </span>
                  <span className="flex items-center">
                    {channel.attempts.map((attempt, index) => (
                      <span key={attempt.id} className="flex items-center">
                        {index > 0 && <span aria-hidden className="h-px w-2 bg-bd-2" />}
                        <button
                          type="button"
                          onClick={() => onOpen(attempt)}
                          title={`#${attempt.attempt} · ${t(`statuses.${attempt.status}`)}${
                            attempt.errorMessage ? ` · ${attempt.errorMessage}` : ''
                          }`}
                          aria-label={t('deliveries.by_event.open_try', {
                            attempt: attempt.attempt,
                            status: t(`statuses.${attempt.status}`),
                          })}
                          data-delivery-mark={attempt.status}
                          className="relative flex h-2.5 w-2.5 items-center justify-center rounded-full after:absolute after:-inset-1.5 after:content-[''] hover:brightness-90 focus-visible:brightness-90 focus-visible:outline-none"
                        >
                          <span className={cn('h-2.5 w-2.5 rounded-full', markClass(attempt))} />
                        </button>
                      </span>
                    ))}
                  </span>
                </li>
              ))}
            </ul>
          ),
        },
        {
          key: 'first',
          header: t('deliveries.by_event.first_delivered'),
          width: 110,
          sortable: true,
          sortValue: (event) => event.firstDeliveredAfter,
          cell: (event) =>
            event.firstDeliveredAfter === null ? (
              <span className="text-[var(--red-ink)]">{t('deliveries.by_event.not_delivered')}</span>
            ) : (
              <span className="tabular-nums text-tx-1">{courseDuration(event.firstDeliveredAfter)}</span>
            ),
        },
        {
          key: 'failed',
          header: t('deliveries.by_event.failed_tries'),
          width: 90,
          sortable: true,
          sortValue: (event) => event.failed,
          cell: (event) =>
            event.failed > 0 ? (
              <span className="font-strong tabular-nums text-[var(--red-ink)]">{event.failed}</span>
            ) : (
              <span className="tabular-nums text-tx-3">0</span>
            ),
        },
      ]}
    />
  );
}
