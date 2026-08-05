import { useQuery } from '@tanstack/react-query';
import { ArrowDownRight, ArrowUpRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { WebVitalsPoint } from '@/api/rum';
import { productStateFor } from '@/product/states';
import { Pill, TimeRangeChip, type PillTone } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';
import { TimeSeriesSparkline } from '@/viz/timeseries/TimeSeriesChart';

import { windowToMicros } from '../_helpers';
import { RumListPage, RumSectionHeader } from '../RumLayout';

type VitalKey = 'lcp_ms' | 'fid_ms' | 'cls' | 'ttfb_ms';
type Grade = 'good' | 'needs_improvement' | 'poor';

interface VitalSpec {
  key: VitalKey;
  label: string;
  unit: string;
  good: number;
  poor: number;
}

const VITALS: VitalSpec[] = [
  { key: 'lcp_ms', label: 'LCP', unit: 'ms', good: 2_500, poor: 4_000 },
  { key: 'fid_ms', label: 'FID', unit: 'ms', good: 100, poor: 300 },
  { key: 'cls', label: 'CLS', unit: '', good: 0.1, poor: 0.25 },
  { key: 'ttfb_ms', label: 'TTFB', unit: 'ms', good: 800, poor: 1_800 },
];

export function WebVitals() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const [activeKey, setActiveKey] = React.useState<VitalKey>('lcp_ms');

  const query = useQuery({
    queryKey: ['rum', 'webvitals-detail', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.webVitalsSeries({ org_id: orgId, ...range, limit: 1_000 }),
    enabled: !!orgId,
  });

  const data = query.data ?? [];
  const state = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data,
  });
  const pageState = productStateFor(state, {
    error: query.error,
    emptyTitle: t('performance.no_data'),
  });
  const activeSpec = VITALS.find((vital) => vital.key === activeKey) ?? VITALS[0]!;
  const activeValues = valuesFor(data, activeSpec.key);
  const distribution = distributionFor(activeValues, activeSpec);
  const pageRows = rankByDimension(data, activeSpec, 'page');
  const dimensionRows = [
    ...rankByDimension(data, activeSpec, 'browser').slice(0, 3),
    ...rankByDimension(data, activeSpec, 'version').slice(0, 3),
    ...rankByDimension(data, activeSpec, 'country').slice(0, 3),
  ]
    .sort((a, b) => b.p75 - a.p75)
    .slice(0, 8);

  return (
    <RumListPage
      title={t('performance.web_vitals')}
      subtitle={t('performance.web_vitals_subtitle') as string}
      toolbar={<TimeRangeChip value={formatWindowSummary(window)} />}
      performance
      state={pageState}
    >
      <div className="grid border-y border-bd-0 sm:grid-cols-2 xl:grid-cols-4">
        {VITALS.map((spec, index) => {
          const values = valuesFor(data, spec.key);
          const p75 = percentile(values, 0.75);
          const dist = distributionFor(values, spec);
          const grade = gradeFor(p75, spec);
          return (
            <button
              type="button"
              key={spec.key}
              onClick={() => setActiveKey(spec.key)}
              className={cn(
                'min-h-[126px] border-b border-bd-0 p-4 text-left transition-colors duration-fast hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo sm:[&:nth-child(odd)]:border-r xl:border-b-0 xl:border-r',
                index === VITALS.length - 1 && 'xl:border-r-0',
                activeKey === spec.key && 'bg-indigo-dim',
              )}
            >
              <span className="flex items-center justify-between gap-3">
                <span className="text-sm font-display text-tx-0">{spec.label}</span>
                <Pill tone={toneFor(grade)}>{t(`experience.${grade}`)}</Pill>
              </span>
              <span className="mt-2 block font-sans text-2xl font-display-strong tabular-nums text-tx-0">
                {formatVital(p75, spec)}
                <span className="ml-1 text-xs font-strong text-tx-3">{t('performance.p75')}</span>
              </span>
              <DistributionBar distribution={dist} compact />
            </button>
          );
        })}
      </div>

      <div className="grid gap-6 xl:grid-cols-12">
        <section className="min-w-0 xl:col-span-8">
          <RumSectionHeader
            title={t('performance.distribution_title', { metric: activeSpec.label })}
            description={t('performance.distribution_description')}
          />
          <div className="grid gap-6 py-5 lg:grid-cols-[minmax(0,1fr)_220px]">
            <VitalDistribution distribution={distribution} spec={activeSpec} />
            <div className="border-l-0 border-bd-0 lg:border-l lg:pl-6">
              <div className="type-caption font-sans font-strong text-tx-3">
                {t('performance.current_p75')}
              </div>
              <div className="mt-2 font-sans text-3xl font-display-strong tabular-nums text-tx-0">
                {formatVital(percentile(activeValues, 0.75), activeSpec)}
              </div>
              <div className="mt-3">
                <Pill tone={toneFor(gradeFor(percentile(activeValues, 0.75), activeSpec))}>
                  {t(`experience.${gradeFor(percentile(activeValues, 0.75), activeSpec)}`)}
                </Pill>
              </div>
              <dl className="mt-5 grid grid-cols-[1fr_auto] gap-x-3 gap-y-2 border-t border-bd-0 pt-4 text-xs">
                <dt className="text-tx-3">{t('performance.good_target')}</dt>
                <dd className="m-0 font-mono font-strong text-tx-1">
                  ≤ {formatVital(activeSpec.good, activeSpec)}
                </dd>
                <dt className="text-tx-3">{t('performance.samples')}</dt>
                <dd className="m-0 font-mono font-strong text-tx-1">
                  {activeValues.length.toLocaleString()}
                </dd>
              </dl>
            </div>
          </div>
          <div className="border-t border-bd-0 pt-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <span className="text-xs font-strong text-tx-2">
                {t('performance.trend_title', { metric: activeSpec.label })}
              </span>
              <span className="text-xs text-tx-3">{formatWindowSummary(window)}</span>
            </div>
            <VitalSparkline values={activeValues} spec={activeSpec} />
          </div>
        </section>

        <section className="min-w-0 xl:col-span-4">
          <RumSectionHeader
            title={t('performance.worst_pages')}
            description={t('performance.worst_pages_description', {
              metric: activeSpec.label,
            })}
          />
          <RankedRows rows={pageRows.slice(0, 7)} spec={activeSpec} empty={t('performance.no_page_data')} />
        </section>

        <section className="min-w-0 xl:col-span-12">
          <RumSectionHeader
            title={t('performance.impact_dimensions')}
            description={t('performance.impact_dimensions_description')}
          />
          <div className="grid gap-x-8 sm:grid-cols-2 xl:grid-cols-4">
            {dimensionRows.length === 0 ? (
              <div className="col-span-full grid min-h-32 place-items-center text-sm text-tx-3">
                {t('performance.no_dimension_data')}
              </div>
            ) : (
              dimensionRows.map((row) => (
                <div
                  key={`${row.dimension}-${row.label}`}
                  className="grid min-h-[64px] grid-cols-[minmax(0,1fr)_auto] items-center gap-4 border-b border-bd-0 py-3"
                >
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-strong text-tx-0">{row.label}</span>
                    <span className="mt-1 block text-xs text-tx-3">
                      {t(`performance.dimension.${row.dimension}`)} ·{' '}
                      {t('performance.sample_count', { count: row.samples })}
                    </span>
                  </span>
                  <span className="text-right">
                    <span className="block font-mono text-sm font-strong text-tx-0">
                      {formatVital(row.p75, activeSpec)}
                    </span>
                    <GradeDelta grade={row.grade} />
                  </span>
                </div>
              ))
            )}
          </div>
        </section>
      </div>
    </RumListPage>
  );
}

function DistributionBar({
  distribution,
  compact,
}: {
  distribution: Distribution;
  compact?: boolean;
}) {
  return (
    <span
      className={cn(
        'mt-3 flex w-full overflow-hidden rounded-full bg-bg-3',
        compact ? 'h-1.5' : 'h-2.5',
      )}
      aria-hidden
    >
      <span className="bg-green" style={{ width: `${distribution.good}%` }} />
      <span className="bg-yellow" style={{ width: `${distribution.needs}%` }} />
      <span className="bg-red" style={{ width: `${distribution.poor}%` }} />
    </span>
  );
}

interface Distribution {
  good: number;
  needs: number;
  poor: number;
}

function VitalDistribution({
  distribution,
  spec,
}: {
  distribution: Distribution;
  spec: VitalSpec;
}) {
  const { t } = useTranslation('rum');
  const rows = [
    {
      key: 'good',
      label: t('experience.good'),
      value: distribution.good,
      color: 'bg-green',
      range: `≤ ${formatVital(spec.good, spec)}`,
    },
    {
      key: 'needs',
      label: t('experience.needs_improvement'),
      value: distribution.needs,
      color: 'bg-yellow',
      range: `${formatVital(spec.good, spec)} – ${formatVital(spec.poor, spec)}`,
    },
    {
      key: 'poor',
      label: t('experience.poor'),
      value: distribution.poor,
      color: 'bg-red',
      range: `> ${formatVital(spec.poor, spec)}`,
    },
  ];
  return (
    <div className="space-y-4">
      {rows.map((row) => (
        <div key={row.key} className="grid grid-cols-[92px_minmax(0,1fr)_54px] items-center gap-3">
          <span>
            <span className="block text-xs font-strong text-tx-1">{row.label}</span>
            <span className="mt-0.5 block font-mono text-xs text-tx-3">{row.range}</span>
          </span>
          <span className="h-7 overflow-hidden rounded-sm bg-bg-3">
            <span
              className={cn('block h-full min-w-px rounded-sm opacity-90', row.color)}
              style={{ width: `${row.value}%` }}
            />
          </span>
          <span className="text-right font-mono text-sm font-strong text-tx-0">
            {row.value.toFixed(0)}%
          </span>
        </div>
      ))}
    </div>
  );
}

function VitalSparkline({ values, spec }: { values: number[]; spec: VitalSpec }) {
  const max = Math.max(spec.poor * 1.2, ...values, 1);
  return (
    <TimeSeriesSparkline
      data={values}
      color="var(--indigo)"
      fill={false}
      height={128}
      min={0}
      max={max}
      ariaLabel={`${spec.label} trend`}
      bands={[
        { to: spec.good, color: 'var(--green-dim)' },
        { from: spec.good, to: spec.poor, color: 'var(--yellow-dim)' },
        { from: spec.poor, color: 'var(--red-dim)' },
      ]}
      thresholds={[
        { value: spec.good, color: 'var(--green)' },
        { value: spec.poor, color: 'var(--red)' },
      ]}
    />
  );
}

interface RankedRow {
  label: string;
  dimension: 'page' | 'browser' | 'version' | 'country';
  p75: number;
  samples: number;
  grade: Grade;
}

function RankedRows({
  rows,
  spec,
  empty,
}: {
  rows: RankedRow[];
  spec: VitalSpec;
  empty: string;
}) {
  const { t } = useTranslation('rum');
  if (rows.length === 0) {
    return <div className="grid min-h-52 place-items-center text-sm text-tx-3">{empty}</div>;
  }
  const max = Math.max(...rows.map((row) => row.p75), 1);
  return (
    <div className="divide-y divide-bd-0">
      {rows.map((row, index) => (
        <div
          key={row.label}
          className="grid min-h-[62px] grid-cols-[24px_minmax(0,1fr)_80px] items-center gap-3 py-3"
        >
          <span className="font-mono text-xs text-tx-3">{index + 1}</span>
          <span className="min-w-0">
            <span className="block truncate text-sm font-strong text-tx-0">{row.label}</span>
            <span className="mt-1 block h-1.5 overflow-hidden rounded-full bg-bg-3">
              <span
                className={cn(
                  'block h-full rounded-full',
                  row.grade === 'poor'
                    ? 'bg-red'
                    : row.grade === 'needs_improvement'
                      ? 'bg-yellow'
                      : 'bg-green',
                )}
                style={{ width: `${Math.max(5, (row.p75 / max) * 100)}%` }}
              />
            </span>
          </span>
          <span className="text-right">
            <span className="block font-mono text-sm font-strong text-tx-0">
              {formatVital(row.p75, spec)}
            </span>
            <span className="mt-1 block text-xs text-tx-3">
              {t('performance.sample_count', { count: row.samples })}
            </span>
          </span>
        </div>
      ))}
    </div>
  );
}

function GradeDelta({ grade }: { grade: Grade }) {
  const { t } = useTranslation('rum');
  if (grade === 'good') {
    return (
      <span className="mt-1 inline-flex items-center gap-0.5 text-xs text-green-soft">
        <ArrowDownRight className="h-3 w-3" />
        {t('experience.good')}
      </span>
    );
  }
  return (
    <span
      className={cn(
        'mt-1 inline-flex items-center gap-0.5 text-xs',
        grade === 'poor' ? 'text-red-soft' : 'text-yellow-soft',
      )}
    >
      <ArrowUpRight className="h-3 w-3" />
      {t(`experience.${grade}`)}
    </span>
  );
}

function valuesFor(data: WebVitalsPoint[], key: VitalKey): number[] {
  return data
    .map((point) => point[key])
    .filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
}

function percentile(values: number[], fraction: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)] ?? 0;
}

function distributionFor(values: number[], spec: VitalSpec): Distribution {
  if (values.length === 0) return { good: 0, needs: 0, poor: 0 };
  const good = values.filter((value) => value <= spec.good).length;
  const poor = values.filter((value) => value > spec.poor).length;
  const needs = values.length - good - poor;
  return {
    good: (good / values.length) * 100,
    needs: (needs / values.length) * 100,
    poor: (poor / values.length) * 100,
  };
}

function gradeFor(value: number, spec: VitalSpec): Grade {
  if (value > spec.poor) return 'poor';
  if (value > spec.good) return 'needs_improvement';
  return 'good';
}

function toneFor(grade: Grade): PillTone {
  if (grade === 'poor') return 'red';
  if (grade === 'needs_improvement') return 'yellow';
  return 'green';
}

function formatVital(value: number, spec: VitalSpec): string {
  if (spec.key === 'cls') return value.toFixed(3);
  return `${Math.round(value)}${spec.unit}`;
}

function rankByDimension(
  points: WebVitalsPoint[],
  spec: VitalSpec,
  dimension: RankedRow['dimension'],
): RankedRow[] {
  const groups = new Map<string, number[]>();
  for (const point of points) {
    const label = point[dimension];
    const value = point[spec.key];
    if (!label || typeof value !== 'number') continue;
    const group = groups.get(label) ?? [];
    group.push(value);
    groups.set(label, group);
  }
  return Array.from(groups.entries())
    .map(([label, values]) => {
      const p75 = percentile(values, 0.75);
      return {
        label,
        dimension,
        p75,
        samples: values.length,
        grade: gradeFor(p75, spec),
      };
    })
    .sort((a, b) => b.p75 - a.p75);
}
