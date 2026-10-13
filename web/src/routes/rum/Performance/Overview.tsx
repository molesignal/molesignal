import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { WebVitalsPoint } from '@/api/rum';
import { ProductState, productStateFor } from '@/product/states';
import { Card, CardBody, CardHeader, TimeRangeChip } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { formatDurationMs, windowToMicros } from '../_helpers';
import { RumListPage } from '../RumLayout';

export function Overview() {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const window = useTimeStore((s) => s.window);
  const setWindow = useTimeStore((s) => s.setWindow);
  const range = React.useMemo(() => windowToMicros(window), [window]);
  const selectTimeRange = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      setWindow({
        mode: 'absolute',
        from: new Date(from / 1000).toISOString(),
        to: new Date(to / 1000).toISOString(),
      }),
    [setWindow],
  );

  const q = useQuery({
    queryKey: ['rum', 'webvitals', orgId, range.from_micros, range.to_micros],
    queryFn: () => rumApi.webVitalsSeries({ org_id: orgId, ...range }),
    enabled: !!orgId,
  });

  const data = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data });
  const chartState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('performance.no_data'),
  });

  // Each vital is said once, as its 75th percentile over its trend: the measure the thresholds
  // are set for. A row of averages above them gave every vital a second, different figure.
  return (
    <RumListPage toolbar={<TimeRangeChip className="border-0" />} performance>
      <Card className="border-0 bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]">
        <CardHeader title={t('performance.chart_title')} className="border-b-0" />
        <CardBody>
          {chartState ? (
            <div className="py-2">
              <ProductState {...chartState} compact />
            </div>
          ) : (
            <WebVitalsTrendChart
              data={data}
              specs={[
                { key: 'lcp_ms', label: 'LCP', color: 'var(--chart-1)', good: 2500, poor: 4000, unit: 'ms' },
                { key: 'fid_ms', label: 'FID', color: 'var(--chart-2)', good: 100, poor: 300, unit: 'ms' },
                { key: 'cls', label: 'CLS', color: 'var(--chart-4)', good: 0.1, poor: 0.25, unit: '' },
                { key: 'ttfb_ms', label: 'TTFB', color: 'var(--chart-3)', good: 800, poor: 1800, unit: 'ms' },
              ]}
              labels={{ p75: t('performance.p75'), latest: t('performance.latest'), target: t('performance.good_target') }}
              range={range}
              onRangeSelect={selectTimeRange}
            />
          )}
        </CardBody>
      </Card>
    </RumListPage>
  );
}


interface VitalTrendSpec {
  key: 'lcp_ms' | 'fid_ms' | 'cls' | 'ttfb_ms';
  label: string;
  color: string;
  good: number;
  poor: number;
  unit: string;
}

function WebVitalsTrendChart({
  data,
  specs,
  labels,
  range,
  onRangeSelect,
}: {
  data: WebVitalsPoint[];
  specs: VitalTrendSpec[];
  labels: { p75: string; latest: string; target: string };
  range: { from_micros: number; to_micros: number };
  onRangeSelect: (range: { from: number; to: number }) => void;
}) {
  return (
    <div className="space-y-3">
      <div className="grid gap-2 xl:grid-cols-4">
        {specs.map((spec) => {
          const values = valuesFor(data, spec.key);
          const latest = values[values.length - 1] ?? 0;
          const p75 = percentile(values, 0.75);
          return (
            <div key={spec.key} className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2">
              <div className="flex items-center justify-between gap-2">
                <span className="font-sans text-xs font-semibold text-tx-2">{spec.label}</span>
                <span className="h-2 w-2 rounded-full" style={{ backgroundColor: spec.color }} />
              </div>
              <div className="mt-1 flex items-baseline gap-2">
                <span className="font-sans text-xl font-semibold leading-6 text-tx-0">{formatVital(p75, spec)}</span>
                <span className="font-sans text-xs font-medium text-tx-3">{labels.p75}</span>
              </div>
              <div className="mt-1 font-sans text-xs text-tx-3">
                {labels.latest} {formatVital(latest, spec)} · {labels.target} {formatVital(spec.good, spec)}
              </div>
            </div>
          );
        })}
      </div>

      <div className="grid gap-3 xl:grid-cols-2">
        {specs.map((spec) => (
          <div
            key={spec.key}
            className="rounded-md border border-bd-0 bg-bg-0 p-3"
          >
            <TimeSeriesChart
              title={spec.label}
              series={[
                {
                  id: `web-vital-${spec.key}`,
                  name: spec.label,
                  color: spec.color,
                  data: data.map((point) => {
                    const value = point[spec.key];
                    return typeof value === 'number' && Number.isFinite(value)
                      ? value
                      : null;
                  }),
                  timestamps: data.map((point) => point.ts_micros),
                  ...(spec.unit ? { unit: spec.unit } : {}),
                },
              ]}
              xDomain={[range.from_micros, range.to_micros]}
              height={170}
              ariaLabel={`${spec.label} ${labels.p75}`}
              options={{
                drawStyle: 'area',
                fillOpacity: 0.08,
                showPoints: 'auto',
                legendMode: 'hidden',
                leftAxis: {
                  min: 0,
                  softMax: spec.poor,
                  ...(spec.unit ? { unit: spec.unit } : {}),
                },
                bands: [
                  { from: 0, to: spec.good, color: 'rgba(34, 197, 94, 0.05)' },
                  { from: spec.good, to: spec.poor, color: 'rgba(234, 179, 8, 0.05)' },
                  { from: spec.poor, color: 'rgba(239, 68, 68, 0.04)' },
                ],
                thresholds: [
                  {
                    value: spec.good,
                    label: labels.target,
                    color: 'var(--green)',
                  },
                  {
                    value: spec.poor,
                    color: 'var(--red)',
                  },
                ],
              }}
              showLegend={false}
              onRangeSelect={onRangeSelect}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

function valuesFor(data: WebVitalsPoint[], key: VitalTrendSpec['key']): number[] {
  return data.map((p) => p[key]).filter((v): v is number => typeof v === 'number' && Number.isFinite(v));
}

function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * p) - 1));
  return sorted[index] ?? 0;
}

/** A vital as the other RUM tabs write it: `2.7 s`, `96 ms`, a shift as a bare figure. */
function formatVital(value: number, spec: VitalTrendSpec): string {
  if (spec.key === 'cls') return value.toFixed(3);
  return formatDurationMs(value);
}
