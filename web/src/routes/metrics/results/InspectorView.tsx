import { AlertTriangle, CheckCircle2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { MetricSeriesQuality } from '@/lib/metricsSeries';
import { ChromeButton } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import type { QueryResult } from '@/types/query';

import { formatMetricDuration, formatPercent } from '../model';

interface InspectorViewProps {
  result: QueryResult | undefined;
  statement: string;
  pending: boolean;
  error: unknown;
  metricSeriesCount: number;
  quality: MetricSeriesQuality;
  timeRangeSeconds: number;
  language: string;
  counterRateQuery: boolean;
  onViewRawCounter: () => void;
  onInspectMetricType: () => void;
}

export function InspectorView({
  result,
  statement,
  pending,
  error,
  metricSeriesCount,
  quality,
  timeRangeSeconds,
  language,
  counterRateQuery,
  onViewRawCounter,
  onInspectMetricType,
}: InspectorViewProps) {
  const { t } = useTranslation('metrics');
  if (error) return <QueryState state="error" error={error} />;
  if (pending && !result) return <QueryState state="loading" />;
  if (!result) {
    return <QueryState state="empty" emptyLabel={t('explore.query_stats.empty')} />;
  }

  const stats = [
    [t('explore.query_stats.series'), metricSeriesCount.toLocaleString()],
    [t('explore.query_stats.points'), quality.dataPoints.toLocaleString()],
    [t('explore.query_stats.scanned'), result.scanned_rows.toLocaleString()],
    [t('explore.query_stats.duration'), `${result.took_ms} ms`],
    [
      t('explore.query_stats.range'),
      formatMetricDuration(timeRangeSeconds, language),
    ],
    [
      t('explore.query_stats.step'),
      quality.estimatedStepSeconds === null
        ? '—'
        : formatMetricDuration(quality.estimatedStepSeconds, language),
    ],
  ];

  return (
    <div className="grid grid-cols-1 gap-px bg-bd-0 lg:grid-cols-[minmax(0,1.4fr)_minmax(280px,0.6fr)]">
      <section className="bg-bg-1 p-4">
        <h3 className="font-sans text-sm font-semibold text-tx-0">
          {t('explore.results.query')}
        </h3>
        <pre className="mt-3 overflow-auto border border-bd-0 bg-bg-2 p-3 font-mono text-xs leading-5 text-tx-1">
          {statement}
        </pre>
        <div className="mt-4 grid grid-cols-2 gap-px bg-bd-0 sm:grid-cols-3">
          {stats.map(([label, value]) => (
            <div key={label} className="bg-bg-1 px-3 py-3">
              <div className="type-micro font-sans uppercase tracking-wide text-tx-3">
                {label}
              </div>
              <div className="mt-1 font-mono text-sm font-semibold tabular-nums text-tx-0">
                {value}
              </div>
            </div>
          ))}
        </div>
      </section>
      <section className="bg-bg-1 p-4">
        <div className="flex items-start gap-2">
          {quality.negativePoints > 0 || quality.timestampAnomalies > 0 ? (
            <AlertTriangle
              className="mt-0.5 h-4 w-4 shrink-0 text-orange"
              aria-hidden="true"
            />
          ) : (
            <CheckCircle2
              className="mt-0.5 h-4 w-4 shrink-0 text-green"
              aria-hidden="true"
            />
          )}
          <div>
            <h3 className="font-sans text-sm font-semibold text-tx-0">
              {quality.negativePoints > 0 || quality.timestampAnomalies > 0
                ? t('explore.quality.attention')
                : t('explore.quality.healthy')}
            </h3>
            <p className="mt-0.5 text-xs text-tx-3">
              {t('explore.quality.distinguishes_missing')}
            </p>
          </div>
        </div>
        <div className="mt-4 divide-y divide-bd-0 text-xs">
          <QualityRow
            label={t('explore.quality.negative_points')}
            value={quality.negativePoints.toLocaleString()}
            warning={quality.negativePoints > 0}
          />
          <QualityRow
            label={t('explore.quality.missing_data')}
            value={`${formatPercent(quality.missingRatio)} · ${quality.missingPoints.toLocaleString()}`}
            warning={quality.missingRatio > 0.2}
          />
          <QualityRow
            label={t('explore.quality.timestamp_anomalies')}
            value={quality.timestampAnomalies.toLocaleString()}
            warning={quality.timestampAnomalies > 0}
          />
        </div>
        {counterRateQuery && quality.negativePoints > 0 ? (
          <div className="mt-4 flex flex-wrap gap-1.5">
            <ChromeButton size="sm" onClick={onViewRawCounter}>
              {t('explore.quality.view_anomalies')}
            </ChromeButton>
            <ChromeButton size="sm" onClick={onInspectMetricType}>
              {t('explore.quality.inspect_type')}
            </ChromeButton>
          </div>
        ) : null}
      </section>
    </div>
  );
}

function QualityRow({
  label,
  value,
  warning,
}: {
  label: string;
  value: string;
  warning: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2 first:pt-0 last:pb-0">
      <span className="text-tx-2">{label}</span>
      <span
        className={`tabular-nums ${
          warning ? 'font-semibold text-orange-soft' : 'text-tx-1'
        }`}
      >
        {value}
      </span>
    </div>
  );
}
