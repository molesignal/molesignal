import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as incidentsApi from '@/api/incidents';
import { formatMicrosActive } from '@/lib/time';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { ResultPagination } from '@/shell/ResultPagination';
import type { IncidentStatus, Severity } from '@/types/alerting';

import { formatDurationSecs } from './alertRuleModel';
import { AlertsPage } from './Frame';
import { SEVERITIES_WORST_FIRST, incidentDurationSecs } from './incidents/model';
import { IncidentSummaryCell } from './incidents/SummaryCell';
import { alertFlatTableClassName } from './Surfaces';

const STATUS_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

const STATUSES: readonly IncidentStatus[] = ['open', 'acknowledged', 'resolved', 'closed'];
const DEFAULT_PAGE_SIZE = 20;
const PAGE_SIZE_OPTIONS = [20, 50, 100];

/**
 * Every incident of the last ninety days, whatever became of it. The search
 * and the two filters are the head of the list; a row names the incident the
 * way the Incidents tab does, then says when it fired, when it recovered and
 * how long that took.
 */
export function AlertsHistory() {
  const { t } = useTranslation('alerts');
  const [viewingIncidentId, setViewingIncidentId] = React.useState<string | null>(null);
  const [search, setSearch] = React.useState('');
  const [severity, setSeverity] = React.useState<Severity | ''>('');
  const [status, setStatus] = React.useState<IncidentStatus | ''>('');
  const [page, setPage] = React.useState(1);
  const [pageSize, setPageSize] = React.useState(DEFAULT_PAGE_SIZE);
  const query = useQuery({
    queryKey: ['alerts-history'],
    queryFn: () => incidentsApi.list({ scope: 'all', window_secs: 90 * 24 * 60 * 60 }),
  });
  const rows = React.useMemo(
    () =>
      [...(query.data ?? [])].sort(
        (left, right) => (right.resolved_at ?? right.created_at) - (left.resolved_at ?? left.created_at),
      ),
    [query.data],
  );
  const filtered = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();

    return rows.filter((incident) => {
      if (severity && incident.severity !== severity) return false;
      if (status && incident.status !== status) return false;
      if (!needle) return true;

      return [
        incident.summary,
        incident.id,
        incident.rule_id,
        ...incident.affected_services,
        incident.labels.service,
        incident.labels.svc,
      ]
        .filter((value): value is string => Boolean(value))
        .join(' ')
        .toLocaleLowerCase()
        .includes(needle);
    });
  }, [rows, search, severity, status]);

  React.useEffect(() => {
    setPage(1);
  }, [search, severity, status]);

  const pageCount = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const paged = filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize);
  const nowMicros = Date.now() * 1000;

  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          title: t('history.error_title'),
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('center.retry')}</ChromeButton>,
        }
      : rows.length === 0
        ? { variant: 'empty', title: t('history.empty_title'), description: t('history.empty_description') }
        : filtered.length === 0
          ? {
              variant: 'empty',
              title: t('history.no_matches_title'),
              description: t('history.no_matches_description'),
            }
          : null;

  return (
    <>
      <AlertsPage
        state={state}
        listHead={
          <>
            <label className="relative w-full min-w-0 sm:w-[320px]">
              <span className="sr-only">{t('history.filters.search_label')}</span>
              <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
              <FormInput
                type="search"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('history.filters.search_placeholder')}
                className="pl-9"
              />
            </label>
            <FormSelect
              ariaLabel={t('history.filters.severity_label')}
              className="w-full sm:w-[160px]"
              value={severity}
              onChange={(value) => setSeverity(value as Severity | '')}
              options={[
                { value: '', label: t('history.filters.all_severities') },
                ...SEVERITIES_WORST_FIRST.map((item) => ({ value: item, label: t(`severity.${item}`) })),
              ]}
            />
            <FormSelect
              ariaLabel={t('history.filters.status_label')}
              className="w-full sm:w-[160px]"
              value={status}
              onChange={(value) => setStatus(value as IncidentStatus | '')}
              options={[
                { value: '', label: t('history.filters.all_statuses') },
                ...STATUSES.map((item) => ({ value: item, label: t(`history.status.${item}`) })),
              ]}
            />
          </>
        }
      >
        <DataTable
          className={alertFlatTableClassName}
          rows={paged}
          rowKey={(incident) => incident.id}
          columns={[
            {
              key: 'incident',
              header: t('history.columns.summary'),
              cell: (incident) => (
                <IncidentSummaryCell
                  incident={incident}
                  onOpen={() => setViewingIncidentId(incident.id)}
                  // The id is what support hands back to the logs: it is read and copied as data.
                  extra={<span className="font-mono">{incident.id}</span>}
                />
              ),
            },
            {
              key: 'status',
              header: t('history.columns.current_status'),
              width: 140,
              cell: (incident) => (
                <Pill tone={STATUS_TONE[incident.status]}>{t(`history.status.${incident.status}`)}</Pill>
              ),
            },
            {
              key: 'fired',
              header: t('history.columns.created'),
              width: 180,
              cell: (incident) => (
                <span className="tabular-nums text-tx-2">{formatMicrosActive(incident.created_at)}</span>
              ),
            },
            {
              key: 'recovered',
              header: t('history.columns.resolved'),
              width: 180,
              cell: (incident) => (
                <span className="tabular-nums text-tx-2">
                  {incident.resolved_at ? formatMicrosActive(incident.resolved_at) : '—'}
                </span>
              ),
            },
            {
              key: 'duration',
              header: t('history.columns.duration'),
              width: 110,
              className: 'text-right',
              headerClassName: 'text-right',
              cell: (incident) => (
                <span className="tabular-nums text-tx-1">
                  {formatDurationSecs(incidentDurationSecs(incident, nowMicros))}
                </span>
              ),
            },
          ]}
        />
        {filtered.length > pageSize && (
          <ResultPagination
            page={currentPage}
            pageCount={pageCount}
            pageSize={pageSize}
            pageSizeOptions={PAGE_SIZE_OPTIONS}
            pageLabel={t('history.pagination.page', { page: currentPage, count: pageCount })}
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
          />
        )}
      </AlertsPage>
      <IncidentDetailDrawer incidentId={viewingIncidentId} onClose={() => setViewingIncidentId(null)} />
    </>
  );
}
