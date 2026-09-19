import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import {
  listApplicationSummaries,
  type ApplicationSummary,
} from '@/api/rum/applications';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, TimeRangeChip } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import {
  RumFilterSelect,
  RumListPage,
  RumSectionHeader,
  RumSurface,
} from './RumLayout';

const ALL = '__all__';

function VersionTopics({ versions }: { versions: string[] }) {
  if (versions.length === 0) return <>—</>;

  return (
    <span className="flex max-w-[28rem] flex-wrap gap-1.5">
      {versions.map((version) => (
        <span
          key={version}
          title={version}
          className="inline-flex min-h-6 max-w-64 items-center truncate rounded-full bg-indigo-dim px-2.5 py-0.5 font-mono text-xs font-strong text-indigo"
        >
          {version}
        </span>
      ))}
    </span>
  );
}

export function Applications() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const [environment, setEnvironment] = React.useState(ALL);
  const [version, setVersion] = React.useState(ALL);
  const hasScope = environment !== ALL || version !== ALL;
  const baseQuery = useQuery({
    queryKey: [
      'rum',
      'applications',
      'base',
      orgId,
      range.from_micros,
      range.to_micros,
    ],
    queryFn: () => listApplicationSummaries({ org_id: orgId, ...range }),
    enabled: Boolean(orgId),
  });
  const scopedQuery = useQuery({
    queryKey: [
      'rum',
      'applications',
      'scope',
      orgId,
      range.from_micros,
      range.to_micros,
      environment,
      version,
    ],
    queryFn: () =>
      listApplicationSummaries({
        org_id: orgId,
        ...range,
        ...(environment !== ALL ? { environment } : {}),
        ...(version !== ALL ? { version } : {}),
      }),
    enabled: Boolean(orgId) && hasScope,
  });
  const query = hasScope ? scopedQuery : baseQuery;
  const baseRows = React.useMemo(() => baseQuery.data ?? [], [baseQuery.data]);
  const unknownApplication = t('scope.unknown_app');
  const rows = React.useMemo(
    () =>
      (query.data ?? []).map((row) =>
        row.application ? row : { ...row, application: unknownApplication },
      ),
    [query.data, unknownApplication],
  );
  const tableColumns = React.useMemo(() => columns(t), [t]);
  const state = productStateFor(
    queryStateFor({
      isLoading: query.isLoading,
      isError: query.isError,
      data: rows,
    }),
    {
      error: query.error,
      emptyTitle: t('applications.empty_title'),
      emptyDescription: t('applications.empty_description'),
    },
  );

  return (
    <RumListPage
      title={t('applications.title')}
      subtitle={t('applications.subtitle')}
      toolbar={
        <>
          <TimeRangeChip value={formatWindowSummary(window)} className="border-0" />
          <ChromeButton
            onClick={() => {
              void baseQuery.refetch();
              if (hasScope) void scopedQuery.refetch();
            }}
          >
            {t('refresh')}
          </ChromeButton>
        </>
      }
      state={state}
    >
      <RumSurface className="space-y-2 p-4">
        <RumSectionHeader
          title={t('applications.list_title')}
          description={t('applications.result_count', { count: rows.length })}
          action={
            <div className="flex flex-wrap items-end gap-3">
              <RumFilterSelect
                label={t('scope.environment')}
                value={environment}
                options={valueOptions(
                  baseRows,
                  'environments',
                  t('scope.all_environments'),
                )}
                onChange={setEnvironment}
              />
              <RumFilterSelect
                label={t('scope.version')}
                value={version}
                options={valueOptions(
                  baseRows,
                  'versions',
                  t('scope.all_versions'),
                )}
                onChange={setVersion}
              />
            </div>
          }
        />
        <DataTable
          className="[&_tbody_tr]:h-auto [&_tbody_td]:py-2"
          rows={rows}
          columns={tableColumns}
          rowKey={(row) => row.application}
          emptyLabel={t('applications.no_filter_results')}
        />
      </RumSurface>
    </RumListPage>
  );
}

function columns(
  t: ReturnType<typeof useTranslation>['t'],
): DataTableColumn<ApplicationSummary>[] {
  return [
    {
      key: 'application',
      header: t('applications.columns.application'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.application}</span>
          <span className="mt-1 block text-xs text-tx-3">
            {row.environments.join(', ') || '—'}
          </span>
        </span>
      ),
    },
    {
      key: 'versions',
      header: t('applications.columns.versions'),
      cell: (row) => <VersionTopics versions={row.versions} />,
    },
    {
      key: 'users',
      header: t('applications.columns.users'),
      cell: (row) => row.users.toLocaleString(),
    },
    {
      key: 'sessions',
      header: t('applications.columns.sessions'),
      cell: (row) => row.sessions.toLocaleString(),
    },
    {
      key: 'error-free',
      header: t('applications.columns.error_free'),
      cell: (row) => (
        <Pill tone={row.errorFreeRate >= 0.99 ? 'green' : row.errorFreeRate >= 0.95 ? 'yellow' : 'red'}>
          {(row.errorFreeRate * 100).toFixed(1)}%
        </Pill>
      ),
    },
    {
      key: 'lcp',
      header: t('applications.columns.lcp'),
      cell: (row) => formatDurationMs(row.lcpP75),
    },
  ];
}

function valueOptions(
  rows: ApplicationSummary[],
  field: 'environments' | 'versions',
  allLabel: string,
) {
  return [
    { value: ALL, label: allLabel },
    ...unique(rows.flatMap((row) => row[field])).map((value) => ({
      value,
      label: value,
    })),
  ];
}

function unique(values: string[]): string[] {
  return Array.from(new Set(values)).sort();
}
