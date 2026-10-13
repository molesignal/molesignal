import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import * as rumApi from '@/api/rum';
import type { ExperienceGrade, WebVitalsPoint } from '@/api/rum';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, TimeRangeChip, type PillTone } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import {
  RumFilterSelect,
  RumListPage,
  RumSectionHeader,
  RumSurface,
} from './RumLayout';

const ALL = '__all__';

interface PageSummary {
  page: string;
  sessions: number;
  samples: number;
  /** The 75th percentile of each vital; `null` where no sample of the page carries it. */
  lcpP75: number | null;
  inpP75: number | null;
  clsP75: number | null;
  grade: ExperienceGrade;
}

export function Pages() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const [application, setApplication] = React.useState(ALL);
  const [environment, setEnvironment] = React.useState(ALL);
  const [version, setVersion] = React.useState(ALL);
  const query = useQuery({
    queryKey: ['rum', 'pages', orgId, range.from_micros, range.to_micros],
    queryFn: () =>
      rumApi.webVitalsSeries({ org_id: orgId, ...range, limit: 1_000 }),
    enabled: Boolean(orgId),
  });
  const points = React.useMemo(() => query.data ?? [], [query.data]);
  const filtered = points.filter(
    (point) =>
      (application === ALL || point.application === application) &&
      (environment === ALL || point.environment === environment) &&
      (version === ALL || point.version === version),
  );
  const rows = summarizePages(filtered);
  const state = productStateFor(
    queryStateFor({
      isLoading: query.isLoading,
      isError: query.isError,
      data: points,
    }),
    {
      error: query.error,
      emptyTitle: t('pages.empty_title'),
      emptyDescription: t('pages.empty_description'),
    },
  );

  return (
    <RumListPage
      toolbar={
        <>
          <TimeRangeChip className="border-0" />
          <ChromeButton onClick={() => query.refetch()}>{t('refresh')}</ChromeButton>
        </>
      }
      filterBar={
        <>
          <RumFilterSelect
            label={t('scope.application')}
            value={application}
            options={valueOptions(points, 'application', t('scope.all_apps'))}
            onChange={setApplication}
          />
          <RumFilterSelect
            label={t('scope.environment')}
            value={environment}
            options={valueOptions(
              points,
              'environment',
              t('scope.all_environments'),
            )}
            onChange={setEnvironment}
          />
          <RumFilterSelect
            label={t('scope.version')}
            value={version}
            options={valueOptions(points, 'version', t('scope.all_versions'))}
            onChange={setVersion}
          />
        </>
      }
      state={state}
    >
      <RumSurface className="space-y-4 p-4">
        <RumSectionHeader
          title={t('pages.list_title')}
          description={t('pages.result_count', { count: rows.length })}
        />
        <DataTable
          rows={rows}
          columns={columns(t)}
          rowKey={(row) => row.page}
          emptyLabel={t('pages.no_filter_results')}
        />
      </RumSurface>
    </RumListPage>
  );
}

function summarizePages(points: WebVitalsPoint[]): PageSummary[] {
  const groups = new Map<string, WebVitalsPoint[]>();
  for (const point of points) {
    const page = point.page ?? '—';
    groups.set(page, [...(groups.get(page) ?? []), point]);
  }
  return Array.from(groups.entries())
    .map(([page, values]) => {
      const lcpP75 = metricPercentile(values, 'lcp_ms');
      const inpP75 = metricPercentile(values, 'inp_ms');
      const clsP75 = metricPercentile(values, 'cls');
      return {
        page,
        sessions: new Set(
          values
            .map((value) => value.session_id)
            .filter((value): value is string => Boolean(value)),
        ).size,
        samples: values.length,
        lcpP75,
        inpP75,
        clsP75,
        grade: webVitalGrade(lcpP75, inpP75, clsP75),
      };
    })
    .sort((left, right) => (right.lcpP75 ?? -1) - (left.lcpP75 ?? -1));
}

function columns(
  t: ReturnType<typeof useTranslation>['t'],
): DataTableColumn<PageSummary>[] {
  return [
    {
      key: 'page',
      header: t('pages.columns.page'),
      cell: (row) => (
        <span className="font-mono text-xs font-strong text-tx-0">{row.page}</span>
      ),
    },
    {
      key: 'grade',
      header: t('pages.columns.experience'),
      cell: (row) => (
        <Pill tone={gradeTone(row.grade)}>{t(`experience.${row.grade}`)}</Pill>
      ),
    },
    {
      key: 'sessions',
      header: t('pages.columns.sessions'),
      cell: (row) => row.sessions.toLocaleString(),
    },
    {
      key: 'lcp',
      header: t('pages.columns.lcp'),
      cell: (row) => formatDurationMs(row.lcpP75 ?? undefined),
    },
    {
      key: 'inp',
      header: t('pages.columns.inp'),
      // A vital no sample carries was not measured: it is not a vital of zero.
      cell: (row) => formatDurationMs(row.inpP75 ?? undefined),
    },
    {
      key: 'cls',
      header: t('pages.columns.cls'),
      cell: (row) => (row.clsP75 === null ? '—' : row.clsP75.toFixed(3)),
    },
    {
      key: 'samples',
      header: t('pages.columns.samples'),
      cell: (row) => row.samples.toLocaleString(),
    },
  ];
}

function valueOptions(
  rows: WebVitalsPoint[],
  field: 'application' | 'environment' | 'version',
  allLabel: string,
) {
  const values = Array.from(
    new Set(
      rows
        .map((row) => row[field])
        .filter((value): value is string => Boolean(value)),
    ),
  ).sort();
  return [
    { value: ALL, label: allLabel },
    ...values.map((value) => ({ value, label: value })),
  ];
}

export function metricPercentile(
  points: WebVitalsPoint[],
  key: 'lcp_ms' | 'inp_ms' | 'cls',
): number | null {
  const values = points
    .map((point) => point[key])
    .filter((value): value is number => value !== undefined && value !== null)
    .sort((left, right) => left - right);
  if (values.length === 0) return null;
  return values[Math.min(values.length - 1, Math.ceil(values.length * 0.75) - 1)] ?? null;
}

/** How a page fares by the vitals that were measured on it; one that was not measured does not count. */
export function webVitalGrade(
  lcp: number | null,
  inp: number | null,
  cls: number | null,
): ExperienceGrade {
  if ((lcp ?? 0) > 4_000 || (inp ?? 0) > 500 || (cls ?? 0) > 0.25) return 'poor';
  if ((lcp ?? 0) > 2_500 || (inp ?? 0) > 200 || (cls ?? 0) > 0.1) return 'needs_improvement';
  if (lcp !== null || inp !== null || cls !== null) return 'good';
  return 'unknown';
}

function gradeTone(grade: ExperienceGrade): PillTone {
  if (grade === 'good') return 'green';
  if (grade === 'needs_improvement') return 'yellow';
  if (grade === 'poor') return 'red';
  return 'neutral';
}
