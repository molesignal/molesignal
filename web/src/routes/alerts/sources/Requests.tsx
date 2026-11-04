import { ChevronDown, ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { AlertOutcome, IntakeRequest, IntakeStatus } from '@/api/alertSources';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shell/ui/table';

import { outcomeCounts } from './model';

const STATUS_INK: Record<IntakeStatus, string> = {
  accepted: 'text-[var(--green-ink)]',
  rejected: 'text-[var(--red-ink)]',
  forwarded: 'text-[var(--blue-ink)]',
};

const OUTCOME_INK: Record<AlertOutcome, string> = {
  opened: 'text-[var(--red-ink)]',
  updated: 'text-[var(--orange-ink)]',
  acknowledged: 'text-[var(--yellow-ink)]',
  resolved: 'text-[var(--green-ink)]',
  unchanged: 'text-tx-2',
  ignored: 'text-tx-2',
};

/** HTTP codes as words in a colour: 4xx orange, 5xx red, the rest in the body grey. */
function httpInk(status: number): string {
  if (status >= 500) return 'text-[var(--red-ink)]';
  if (status >= 400) return 'text-[var(--orange-ink)]';
  return 'text-tx-2';
}

/**
 * What a source received, the latest first: when, what became of the request
 * and of each of its alerts. A row opens to the alerts it carried, each with
 * the incident it landed on, or, for a request that was refused, the start of
 * its body.
 */
export function SourceRequests({ requests }: { requests: readonly IntakeRequest[] }) {
  const { t } = useTranslation('alerts');
  const [open, setOpen] = React.useState<ReadonlySet<string>>(() => new Set());

  const toggle = (id: string) =>
    setOpen((current) => {
      const next = new Set(current);

      if (next.has(id)) next.delete(id);
      else next.add(id);

      return next;
    });

  return (
    // The first and last columns start where the section's words do.
    <Table data-source-requests className="[&_tr>*:first-child]:pl-[20px] [&_tr>*:last-child]:pr-[20px]">
      <TableHeader>
        <TableRow>
          <TableHead style={{ width: 200 }}>{t('alert_sources.requests.columns.time')}</TableHead>
          <TableHead style={{ width: 150 }}>{t('alert_sources.requests.columns.result')}</TableHead>
          <TableHead style={{ width: 240 }}>{t('alert_sources.requests.columns.alerts')}</TableHead>
          <TableHead>{t('alert_sources.requests.columns.message')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {requests.map((request) => {
          const expanded = open.has(request.id);
          const Chevron = expanded ? ChevronDown : ChevronRight;

          return (
            <React.Fragment key={request.id}>
              <TableRow
                data-request-status={request.status}
                tabIndex={0}
                aria-expanded={expanded}
                className="cursor-pointer"
                onClick={() => toggle(request.id)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    toggle(request.id);
                  }
                }}
              >
                <TableCell className="whitespace-nowrap tabular-nums">
                  <span className="inline-flex items-center gap-1.5">
                    <Chevron aria-hidden className="h-3.5 w-3.5 text-tx-3" />
                    {formatMicrosActive(request.receivedAt)}
                  </span>
                </TableCell>
                <TableCell className="whitespace-nowrap">
                  <span className={STATUS_INK[request.status]}>
                    {t(`alert_sources.requests.status.${request.status}`)}
                  </span>
                  <span className={cn('ml-2 tabular-nums', httpInk(request.httpStatus))}>{request.httpStatus}</span>
                </TableCell>
                <TableCell className="whitespace-nowrap">
                  <OutcomeSummary request={request} />
                </TableCell>
                <TableCell className="max-w-0 truncate text-tx-2">
                  {[
                    request.viaCluster ? t('alert_sources.requests.via', { cluster: request.viaCluster }) : null,
                    request.message,
                  ]
                    .filter(Boolean)
                    .join(' · ')}
                </TableCell>
              </TableRow>
              {expanded && (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={4} className="whitespace-normal bg-[var(--page-canvas)]">
                    <RequestDetail request={request} />
                  </TableCell>
                </TableRow>
              )}
            </React.Fragment>
          );
        })}
      </TableBody>
    </Table>
  );
}

function OutcomeSummary({ request }: { request: IntakeRequest }) {
  const { t } = useTranslation('alerts');
  const counts = outcomeCounts(request.alerts);

  return (
    <span className="inline-flex items-center gap-2 tabular-nums">
      <span className="text-tx-1">{t('alert_sources.requests.alert_count', { count: request.alertCount })}</span>
      {counts.map(({ outcome, count }) => (
        <span key={outcome} className={OUTCOME_INK[outcome]}>
          {t(`alert_sources.requests.outcomes.${outcome}`)} {count}
        </span>
      ))}
    </span>
  );
}

function RequestDetail({ request }: { request: IntakeRequest }) {
  const { t } = useTranslation('alerts');

  return (
    <div className="flex flex-col gap-2 py-1 font-sans text-xs">
      {request.alerts.map((alert, index) => (
        <div key={`${alert.dedupKey}-${index}`} className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
          <span className="text-tx-2">{t(`alert_sources.requests.actions.${alert.action}`)}</span>
          <span className="min-w-0 font-strong text-tx-0">{alert.summary || alert.dedupKey}</span>
          {alert.severity && <span className="text-tx-2">{t(`severity.${alert.severity}`)}</span>}
          <span className={OUTCOME_INK[alert.outcome]}>{t(`alert_sources.requests.outcomes.${alert.outcome}`)}</span>
          {alert.reason && <span className="text-tx-2">{alert.reason}</span>}
          {alert.incidentId && (
            <Link
              to={`/alerts/incidents/${encodeURIComponent(alert.incidentId)}`}
              className="text-indigo-soft underline-offset-4 outline-none hover:underline focus-visible:underline"
            >
              {t('alert_sources.requests.open_incident')}
            </Link>
          )}
          <span className="basis-full truncate font-mono text-tx-3">{alert.dedupKey}</span>
        </div>
      ))}
      {request.bodyExcerpt && (
        <div className="flex flex-col gap-1">
          <span className="text-tx-2">{t('alert_sources.requests.body')}</span>
          <pre className="m-0 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md bg-[var(--functional-surface)] px-3 py-2 font-mono text-xs text-tx-1">
            {request.bodyExcerpt}
          </pre>
        </div>
      )}
      {request.alerts.length === 0 && !request.bodyExcerpt && (
        <span className="text-tx-2">{request.message ?? t('alert_sources.requests.no_alerts')}</span>
      )}
    </div>
  );
}
