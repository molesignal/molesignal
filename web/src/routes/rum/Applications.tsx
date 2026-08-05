import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import * as rumApi from '@/api/rum';
import type { SessionRow } from '@/api/rum';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, TimeRangeChip } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import { RumFilterSelect, RumListPage, RumSectionHeader } from './RumLayout';

const ALL = '__all__';

interface ApplicationSummary {
  application: string;
  environments: string[];
  versions: string[];
  users: number;
  sessions: number;
  errorFreeRate: number;
  lcpP75: number;
}

export function Applications() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const [environment, setEnvironment] = React.useState(ALL);
  const [version, setVersion] = React.useState(ALL);
  const query = useQuery({
    queryKey: ['rum', 'applications', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.listSessions({ org_id: orgId, ...range, limit: 500 }),
    enabled: Boolean(orgId),
  });
  const sessions = React.useMemo(() => query.data?.items ?? [], [query.data]);
  const filtered = sessions.filter(
    (session) =>
      (environment === ALL || session.environment === environment) &&
      (version === ALL || session.version === version),
  );
  const rows = summarizeApplications(filtered, t('scope.unknown_app'));
  const state = productStateFor(
    queryStateFor({
      isLoading: query.isLoading,
      isError: query.isError,
      data: sessions,
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
          <TimeRangeChip value={formatWindowSummary(window)} />
          <ChromeButton onClick={() => query.refetch()}>{t('refresh')}</ChromeButton>
        </>
      }
      filterBar={
        <>
          <RumFilterSelect
            label={t('scope.environment')}
            value={environment}
            options={valueOptions(
              sessions,
              'environment',
              t('scope.all_environments'),
            )}
            onChange={setEnvironment}
          />
          <RumFilterSelect
            label={t('scope.version')}
            value={version}
            options={valueOptions(sessions, 'version', t('scope.all_versions'))}
            onChange={setVersion}
          />
        </>
      }
      state={state}
    >
      <section>
        <RumSectionHeader
          title={t('applications.list_title')}
          description={t('applications.result_count', { count: rows.length })}
        />
        <DataTable
          rows={rows}
          columns={columns(t)}
          rowKey={(row) => row.application}
          emptyLabel={t('applications.no_filter_results')}
        />
      </section>
    </RumListPage>
  );
}

function summarizeApplications(
  sessions: SessionRow[],
  unknownLabel: string,
): ApplicationSummary[] {
  const groups = new Map<string, SessionRow[]>();
  for (const session of sessions) {
    const key = session.application ?? unknownLabel;
    groups.set(key, [...(groups.get(key) ?? []), session]);
  }
  return Array.from(groups.entries())
    .map(([application, rows]) => {
      const errorFree = rows.filter(
        (row) => (row.error_count ?? 0) === 0 && row.failed_request_count === 0,
      ).length;
      return {
        application,
        environments: unique(rows.map((row) => row.environment)),
        versions: unique(rows.map((row) => row.version)),
        users: new Set(
          rows
            .map((row) => row.user_id ?? row.session_id)
            .filter((value) => value.length > 0),
        ).size,
        sessions: rows.length,
        errorFreeRate: rows.length === 0 ? 0 : errorFree / rows.length,
        lcpP75: percentile(
          rows
            .map((row) => row.lcp_ms)
            .filter((value): value is number => value !== undefined),
          0.75,
        ),
      };
    })
    .sort((left, right) => right.sessions - left.sessions);
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
      cell: (row) => row.versions.slice(0, 3).join(', ') || '—',
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
  rows: SessionRow[],
  field: 'environment' | 'version',
  allLabel: string,
) {
  return [
    { value: ALL, label: allLabel },
    ...unique(rows.map((row) => row[field])).map((value) => ({
      value,
      label: value,
    })),
  ];
}

function unique(values: Array<string | undefined>): string[] {
  return Array.from(
    new Set(values.filter((value): value is string => Boolean(value))),
  ).sort();
}

function percentile(values: number[], fraction: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((left, right) => left - right);
  return (
    sorted[
      Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)
    ] ?? 0
  );
}
