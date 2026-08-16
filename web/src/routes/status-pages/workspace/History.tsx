import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type {
  PublicIncidentStatus,
  StatusPageIncidentKind,
} from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { PageBody } from '@/shell/PageHeader';
import { ResultPagination } from '@/shell/ResultPagination';

import {
  INCIDENT_TONE,
  affectedComponentNames,
  formatDurationMicros,
  incidentStatusLabel,
} from '../model';
import {
  StatusPageCanvas,
  StatusPageFilterBand,
  StatusPageListSurface,
  statusPageFlatStateClassName,
  statusPageFlatTableClassName,
} from './CardlessSurface';
import { StatusPageEventDrawer } from './EventDrawer';
import { useStatusPageWorkspace } from './Layout';
import { shouldShowStatusPagePagination } from './pagination';

export function StatusPageHistory() {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const location = useLocation();
  const { eventId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot } = useStatusPageWorkspace();
  const [search, setSearch] = React.useState(searchParams.get('q') ?? '');
  React.useEffect(() => setSearch(searchParams.get('q') ?? ''), [searchParams]);
  const page = Math.max(1, Number(searchParams.get('page')) || 1);
  const kind = parseKind(searchParams.get('type'));
  const status = parseStatus(searchParams.get('status'));
  const component = searchParams.get('component') || undefined;
  const searchQuery = searchParams.get('q') || undefined;
  const from = parseMicros(searchParams.get('from'));
  const to = parseMicros(searchParams.get('to'));
  const query = useQuery({
    queryKey: [
      'status-pages',
      pageId,
      'history',
      kind,
      status,
      component,
      from,
      to,
      searchQuery,
      page,
    ],
    queryFn: () => statusPagesApi.history(pageId, {
      ...(kind ? { type: kind } : {}),
      ...(status ? { status } : {}),
      ...(component ? { component } : {}),
      ...(from ? { from } : {}),
      ...(to ? { to } : {}),
      ...(searchQuery ? { q: searchQuery } : {}),
      page,
    }),
  });
  const selected = query.data?.items.find((event) => event.id === eventId);

  const updateFilter = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    if (key !== 'page') next.delete('page');
    setSearchParams(next, { replace: true });
  };
  const closeDrawer = () =>
    navigate({ pathname: `/status-pages/${pageId}/history`, search: location.search }, { replace: true });

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageFilterBand
          className="grid gap-2 md:grid-cols-2 xl:grid-cols-[minmax(220px,1fr)_150px_160px_190px_190px_190px]"
          onSubmit={(event) => {
            event.preventDefault();
            updateFilter('q', search.trim());
          }}
        >
          <label className="relative min-w-0">
            <span className="sr-only">{t('filters.search')}</span>
            <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
            <FormInput
              value={search}
              onChange={(event) => setSearch(event.currentTarget.value)}
              placeholder={t('filters.search_placeholder')}
              className="pl-9"
            />
          </label>
          <FormSelect
            ariaLabel={t('filters.type')}
            value={kind ?? ''}
            onChange={(value) => updateFilter('type', value)}
            options={[
              { value: '', label: t('filters.all_types') },
              { value: 'incident', label: t('kind.incident') },
              { value: 'maintenance', label: t('kind.maintenance') },
            ]}
          />
          <DateTimePicker
            aria-label={t('filters.from')}
            value={microsToLocalDateTime(from)}
            onChange={(value) => updateFilter('from', localDateTimeToMicros(value))}
            placeholder={t('filters.from')}
          />
          <DateTimePicker
            aria-label={t('filters.to')}
            value={microsToLocalDateTime(to)}
            onChange={(value) => updateFilter('to', localDateTimeToMicros(value))}
            placeholder={t('filters.to')}
          />
          <FormSelect
            ariaLabel={t('filters.status')}
            value={status ?? ''}
            onChange={(value) => updateFilter('status', value)}
            options={[
              { value: '', label: t('filters.all_statuses') },
              { value: 'resolved', label: t('incident_status.resolved') },
              { value: 'completed', label: t('incident_status.completed') },
              { value: 'cancelled', label: t('incident_status.cancelled') },
            ]}
          />
          <FormSelect
            ariaLabel={t('filters.component')}
            value={component ?? ''}
            onChange={(value) => updateFilter('component', value)}
            options={[
              { value: '', label: t('filters.all_components') },
              ...snapshot.components.map((item) => ({ value: item.id, label: item.name })),
            ]}
          />
        </StatusPageFilterBand>

        {query.isLoading ? (
          <ProductState variant="loading" className={statusPageFlatStateClassName} />
        ) : query.isError ? (
          <ProductState
            variant="error"
            error={query.error}
            className={statusPageFlatStateClassName}
          />
        ) : (
          <StatusPageListSurface>
          <DataTable
            className={statusPageFlatTableClassName}
            rows={query.data?.items ?? []}
            rowKey={(event) => event.id}
            emptyLabel={t('states.no_history')}
            onRowClick={(event) =>
              navigate({ pathname: `/status-pages/${pageId}/history/${event.id}`, search: location.search })
            }
            columns={[
              {
                key: 'type',
                header: t('columns.type'),
                width: 140,
                cell: (event) => t(`kind.${event.kind}`),
              },
              {
                key: 'title',
                header: t('columns.title'),
                cell: (event) => (
                  <div className="min-w-0">
                    <div className="truncate text-tx-0">{event.title}</div>
                    <div className="mt-0.5 truncate text-xs font-normal text-tx-3">
                      {affectedComponentNames(event, snapshot.components).join(', ') || t('values.no_components')}
                    </div>
                  </div>
                ),
              },
              {
                key: 'status',
                header: t('columns.status'),
                width: 140,
                cell: (event) => (
                  <Pill tone={INCIDENT_TONE[event.status]}>{incidentStatusLabel(t, event.status)}</Pill>
                ),
              },
              {
                key: 'duration',
                header: t('columns.duration'),
                width: 150,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">
                    {formatDurationMicros(
                      t,
                      event.started_at,
                      event.ended_at ?? event.updated_at,
                    )}
                  </span>
                ),
              },
              {
                key: 'date',
                header: t('columns.ended'),
                width: 190,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">
                    {formatMicrosActive(event.ended_at ?? event.updated_at)}
                  </span>
                ),
              },
            ]}
          />
          {shouldShowStatusPagePagination(query.data) && (
            <ResultPagination
              page={page}
              pageCount={Math.max(1, Math.ceil(query.data.total / query.data.per_page))}
              pageSize={query.data.per_page}
              pageSizeOptions={[25]}
              pageLabel={t('pagination.page', {
                page,
                pages: Math.max(1, Math.ceil(query.data.total / query.data.per_page)),
              })}
              ariaLabel={t('pagination.label')}
              pageSizeAriaLabel={t('pagination.page_size')}
              firstAriaLabel={t('pagination.first')}
              previousAriaLabel={t('pagination.previous')}
              nextAriaLabel={t('pagination.next')}
              lastAriaLabel={t('pagination.last')}
              onPageChange={(nextPage) => updateFilter('page', String(nextPage))}
              onPageSizeChange={() => undefined}
            />
          )}
          </StatusPageListSurface>
        )}
      </StatusPageCanvas>

      <StatusPageEventDrawer
        kind={selected?.kind ?? 'incident'}
        eventId={eventId}
        onClose={closeDrawer}
      />
    </PageBody>
  );
}

function parseKind(value: string | null): StatusPageIncidentKind | undefined {
  return value === 'incident' || value === 'maintenance' ? value : undefined;
}

function parseStatus(value: string | null): PublicIncidentStatus | undefined {
  return value === 'resolved' || value === 'completed' || value === 'cancelled'
    ? value
    : undefined;
}

function parseMicros(value: string | null): number | undefined {
  if (!value) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined;
}

function microsToLocalDateTime(value: number | undefined): string {
  if (!value) return '';
  const date = new Date(Math.floor(value / 1_000));
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function localDateTimeToMicros(value: string): string {
  if (!value) return '';
  const millis = new Date(value).getTime();
  return Number.isFinite(millis) ? String(millis * 1_000) : '';
}
