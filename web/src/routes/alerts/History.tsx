import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import { formatMicrosActive } from '@/lib/time';
import { Pill, type PillTone } from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { LoadingState } from '@/shell/LoadingState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import type { Incident, Severity } from '@/types/alerting';

import { AlertsSubNav } from './Layout';

const SEVERITY_TONE: Record<Severity, PillTone> = {
  info: 'blue',
  warning: 'yellow',
  error: 'red',
  critical: 'red',
};

function durationLabel(incident: Incident): string {
  if (!incident.resolved_at) return '—';
  const ms = (incident.resolved_at - incident.created_at) / 1000;
  if (ms < 60_000) return `${(ms / 1000).toFixed(0)}s`;
  if (ms < 3_600_000) return `${(ms / 60_000).toFixed(1)}m`;
  return `${(ms / 3_600_000).toFixed(1)}h`;
}

export function AlertsHistory() {
  const { t } = useTranslation('alerts');
  const [viewingIncidentId, setViewingIncidentId] = React.useState<string | null>(null);
  const q = useQuery({
    queryKey: ['alerts-history'],
    queryFn: () => incidentsApi.list({ scope: 'all', window_secs: 90 * 24 * 60 * 60 }),
  });
  const rows = (q.data ?? []).filter((i) => i.status === 'resolved' || i.status === 'closed');
  rows.sort((a, b) => (b.resolved_at ?? 0) - (a.resolved_at ?? 0));
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });

  return (
    <>
      <PageHeader
        title={t('history.title')}
        subtitle={t('history.subtitle')}
      />
      <AlertsSubNav />
      <PageBody>
        {state === 'loading' && <LoadingState variant="list" rows={6} />}
        {state === 'error' && (
          <ErrorState
            error={q.error}
            title={t('history.error_title')}
            onRetry={() => void q.refetch()}
          />
        )}
        {state === 'empty' && (
          <EmptyState
            strategy="backend-pending"
            title={t('history.empty_title')}
            description={t('history.empty_description')}
          />
        )}
        {state === null && (
          <div className="overflow-hidden rounded-md border border-bd-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="min-w-[260px]">{t('history.columns.summary')}</TableHead>
                  <TableHead className="w-[120px]">{t('history.columns.severity')}</TableHead>
                  <TableHead className="w-[180px]">{t('history.columns.created')}</TableHead>
                  <TableHead className="w-[180px]">{t('history.columns.resolved')}</TableHead>
                  <TableHead className="w-[80px] text-right">{t('history.columns.duration')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="text-tx-0">
                      <div className="flex flex-col gap-0.5">
                        {/* Phase 6 M1.1 #3: summary opens IncidentDetailDrawer in
                            place of the legacy /alerts/incidents/:id link. */}
                        <button
                          type="button"
                          onClick={() => setViewingIncidentId(r.id)}
                          className="text-left font-strong text-tx-0 hover:text-indigo focus-visible:outline-none focus-visible:underline"
                          data-testid="open-incident"
                        >
                          {r.summary}
                        </button>
                        <span className="text-xs text-tx-3">
                          {/* Incident id exposed as a copy/jump target so support can
                              hand it back to the backend logs. */}
                          <SignalReference type="trace_id" value={r.id}>
                            {r.id.slice(0, 12)}…
                          </SignalReference>
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Pill tone={SEVERITY_TONE[r.severity]}>
                        {t(`history.severity.${r.severity}`)}
                      </Pill>
                    </TableCell>
                    <TableCell className="text-tx-2">{formatMicrosActive(r.created_at)}</TableCell>
                    <TableCell className="text-tx-2">{formatMicrosActive(r.resolved_at)}</TableCell>
                    <TableCell className="text-right text-tx-1">{durationLabel(r)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </PageBody>
      <IncidentDetailDrawer
        incidentId={viewingIncidentId}
        onClose={() => setViewingIncidentId(null)}
      />
    </>
  );
}
