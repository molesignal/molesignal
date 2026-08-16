import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
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
import { ResultPagination } from '@/shell/ResultPagination';
import { SignalReference } from '@/shell/SignalReference';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import type { Incident, IncidentStatus, Severity } from '@/types/alerting';

import { AlertsSubNav } from './Layout';

const SEVERITY_TONE: Record<Severity, PillTone> = {
  info: 'blue',
  warning: 'yellow',
  error: 'red',
  critical: 'red',
};

const STATUS_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

const DEFAULT_PAGE_SIZE = 20;
const PAGE_SIZE_OPTIONS = [20, 50, 100];

type SeverityFilter = 'all' | Severity;
type StatusFilter = 'all' | IncidentStatus;

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
  const [search, setSearch] = React.useState('');
  const [severityFilter, setSeverityFilter] = React.useState<SeverityFilter>('all');
  const [statusFilter, setStatusFilter] = React.useState<StatusFilter>('all');
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(DEFAULT_PAGE_SIZE);
  const q = useQuery({
    queryKey: ['alerts-history'],
    queryFn: () => incidentsApi.list({ scope: 'all', window_secs: 90 * 24 * 60 * 60 }),
  });
  const rows = React.useMemo(
    () =>
      [...(q.data ?? [])].sort(
        (a, b) => (b.resolved_at ?? b.created_at) - (a.resolved_at ?? a.created_at),
      ),
    [q.data],
  );
  const filteredRows = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    return rows.filter((incident) => {
      if (severityFilter !== 'all' && incident.severity !== severityFilter) return false;
      if (statusFilter !== 'all' && incident.status !== statusFilter) return false;
      if (!needle) return true;

      const searchable = [
        incident.summary,
        incident.id,
        incident.rule_id,
        ...incident.affected_services,
        incident.labels.service,
        incident.labels.svc,
      ]
        .filter((value): value is string => Boolean(value))
        .join(' ')
        .toLocaleLowerCase();
      return searchable.includes(needle);
    });
  }, [rows, search, severityFilter, statusFilter]);

  React.useEffect(() => {
    setPage(1);
  }, [search, severityFilter, statusFilter]);

  const pageCount = Math.max(1, Math.ceil(filteredRows.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const pagedRows = filteredRows.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });

  return (
    <>
      <PageHeader
        title={t('history.title')}
        subtitle={t('history.subtitle')}
      />
      <AlertsSubNav />
      <PageBody className="pt-2">
        <div className="flex w-full flex-col gap-2 py-2 sm:flex-row sm:items-center">
          <label className="relative w-full sm:w-80">
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('history.filters.search_placeholder')}
              aria-label={t('history.filters.search_label')}
              className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 pl-9 pr-3 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus-visible:bg-bg-3"
            />
          </label>
          <Select
            value={severityFilter}
            onValueChange={(value) => setSeverityFilter(value as SeverityFilter)}
          >
            <SelectTrigger
              aria-label={t('history.filters.severity_label')}
              className="w-full sm:w-40"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('history.filters.all_severities')}</SelectItem>
              <SelectItem value="info">{t('history.severity.info')}</SelectItem>
              <SelectItem value="warning">{t('history.severity.warning')}</SelectItem>
              <SelectItem value="error">{t('history.severity.error')}</SelectItem>
              <SelectItem value="critical">{t('history.severity.critical')}</SelectItem>
            </SelectContent>
          </Select>
          <Select
            value={statusFilter}
            onValueChange={(value) => setStatusFilter(value as StatusFilter)}
          >
            <SelectTrigger
              aria-label={t('history.filters.status_label')}
              className="w-full sm:w-40"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('history.filters.all_statuses')}</SelectItem>
              <SelectItem value="open">{t('history.status.open')}</SelectItem>
              <SelectItem value="acknowledged">
                {t('history.status.acknowledged')}
              </SelectItem>
              <SelectItem value="resolved">{t('history.status.resolved')}</SelectItem>
              <SelectItem value="closed">{t('history.status.closed')}</SelectItem>
            </SelectContent>
          </Select>
        </div>
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
          <>
            {filteredRows.length === 0 ? (
              <EmptyState
                strategy="query-first"
                title={t('history.no_matches_title')}
                description={t('history.no_matches_description')}
                className="min-h-[180px]"
              />
            ) : (
          <Table className="min-w-[1000px]">
                <TableHeader>
                  <TableRow>
                    <TableHead className="min-w-[260px]">
                      {t('history.columns.summary')}
                    </TableHead>
                    <TableHead className="w-[120px]">
                      {t('history.columns.severity')}
                    </TableHead>
                <TableHead className="w-[130px]">
                  {t('history.columns.current_status')}
                </TableHead>
                    <TableHead className="w-[180px]">
                      {t('history.columns.created')}
                    </TableHead>
                    <TableHead className="w-[180px]">
                      {t('history.columns.resolved')}
                    </TableHead>
                    <TableHead className="w-[80px] text-right">
                      {t('history.columns.duration')}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pagedRows.map((r) => (
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
                  <TableCell>
                    <Pill tone={STATUS_TONE[r.status]}>
                      {t(`history.status.${r.status}`)}
                    </Pill>
                  </TableCell>
                      <TableCell className="text-tx-2">
                        {formatMicrosActive(r.created_at)}
                      </TableCell>
                      <TableCell className="text-tx-2">
                        {r.resolved_at ? formatMicrosActive(r.resolved_at) : '—'}
                      </TableCell>
                      <TableCell className="text-right text-tx-1">
                        {durationLabel(r)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
            {filteredRows.length > pageSize && (
              <ResultPagination
                page={currentPage}
                pageCount={pageCount}
                pageSize={pageSize}
                pageSizeOptions={PAGE_SIZE_OPTIONS}
                pageLabel={t('history.pagination.page', {
                  page: currentPage,
                  count: pageCount,
                })}
                ariaLabel={t('history.pagination.label')}
                pageSizeAriaLabel={t('history.pagination.page_size')}
                firstAriaLabel={t('history.pagination.first')}
                previousAriaLabel={t('history.pagination.previous')}
                nextAriaLabel={t('history.pagination.next')}
                lastAriaLabel={t('history.pagination.last')}
                onPageChange={setPage}
                onPageSizeChange={(nextPageSize) => {
                  setPageSize(nextPageSize);
                  setPage(1);
                }}
                className="ml-auto mt-2 w-fit border-0 bg-transparent px-0"
              />
            )}
          </>
        )}
      </PageBody>
      <IncidentDetailDrawer
        incidentId={viewingIncidentId}
        onClose={() => setViewingIncidentId(null)}
      />
    </>
  );
}
