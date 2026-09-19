import { Check, Database, TestTube2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, Dot, Pill } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { cn } from '@/shell/lib/cn';
import { LoadingState } from '@/shell/LoadingState';
import type { SeverityThreshold } from '@/types/alerting';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { COMPARISON_LABEL } from '../alertRuleModel';

export function WorkbenchSteps() {
  const { t } = useTranslation('alerts');
  return (
    <div className="flex min-h-12 items-center gap-2 overflow-x-auto border-b border-bd-0 bg-bg-0 px-4 sm:px-6">
      {[
        t('workbench.steps.identity'),
        t('workbench.steps.condition'),
        t('workbench.steps.delivery'),
      ].map((label, index) => (
        <React.Fragment key={label}>
          {index > 0 && <span className="h-px w-8 shrink-0 bg-bd-1" />}
          <span className="flex shrink-0 items-center gap-2 font-sans text-xs font-strong text-tx-2">
            <span className="grid h-5 w-5 place-items-center rounded-full bg-bg-2 font-mono text-type-micro text-tx-1">
              {index + 1}
            </span>
            {label}
          </span>
        </React.Fragment>
      ))}
    </div>
  );
}

export function WorkbenchSection({
  id,
  number,
  title,
  description,
  children,
}: {
  id: string;
  number: string;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section id={id} data-alert-workbench-section className="min-w-0 bg-transparent">
      <header className="flex items-start gap-3 border-b border-bd-0 py-3.5">
        <span className="mt-0.5 font-mono text-xs font-semibold text-indigo-soft">
          {number}
        </span>
        <div>
          <h2 className="font-sans text-sm font-display-strong text-tx-0">{title}</h2>
          <p className="mt-0.5 text-xs leading-relaxed text-tx-2">{description}</p>
        </div>
      </header>
      <div className="space-y-5 pb-6 pt-5">{children}</div>
    </section>
  );
}

export function QueryPreview({
  pending,
  attempted,
  stale,
  error,
  points,
  timestamps,
  from,
  to,
  threshold,
  currentValue,
  estimatedEpisodes,
  scannedRows,
  tookMs,
  onRun,
}: {
  pending: boolean;
  attempted: boolean;
  stale: boolean;
  error: unknown;
  points: Array<{ value: number }>;
  timestamps: number[];
  from?: number;
  to?: number;
  threshold: SeverityThreshold | null;
  currentValue?: number;
  estimatedEpisodes: number | null;
  scannedRows?: number;
  tookMs?: number;
  onRun: () => void;
}) {
  const { i18n, t } = useTranslation('alerts');
  const hasData = points.length > 0;
  const docsLocale = (i18n.resolvedLanguage ?? i18n.language)
    .toLowerCase()
    .startsWith('zh')
    ? 'zh-Hans'
    : 'en-US';
  return (
    <section data-alert-query-preview className="min-w-0 bg-transparent">
      <header className="flex min-h-12 items-center gap-3 border-b border-bd-0 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="font-sans text-sm font-display-strong text-tx-0">
            {t('workbench.preview.title')}
          </h2>
          <p className="mt-0.5 text-xs text-tx-2">{t('workbench.preview.window')}</p>
        </div>
        <ChromeButton size="sm" onClick={onRun} disabled={pending}>
          <TestTube2 className="h-3.5 w-3.5" />
          {pending ? t('workbench.actions.testing') : t('workbench.actions.test')}
        </ChromeButton>
      </header>
      <div className="pt-4">
        {pending ? (
          <div className="flex h-[220px] items-center justify-center">
            <LoadingState variant="list" rows={3} />
          </div>
        ) : error ? (
          <ErrorState
            error={error}
            title={t('workbench.preview.failed')}
            onRetry={onRun}
            help={{
              label: t('workbench.preview.query_docs'),
              href: `https://docs.molesignal.io/${docsLocale}/query`,
            }}
            className="min-h-[220px] justify-center border-0 bg-transparent"
          />
        ) : stale ? (
          <PreviewMessage
            iconClassName="text-blue-soft"
            title={t('workbench.preview.stale')}
            description={t('workbench.preview.stale_description')}
          />
        ) : !attempted ? (
          <PreviewMessage
            title={t('workbench.preview.not_run')}
            description={t('workbench.preview.not_run_description')}
          />
        ) : !hasData ? (
          <PreviewMessage
            title={t('workbench.preview.no_data')}
            description={t('workbench.preview.no_data_description')}
          />
        ) : (
          <>
            <TimeSeriesChart
              series={[
                {
                  id: 'query-value',
                  name: t('workbench.preview.query_value'),
                  color: 'var(--chart-1)',
                  data: points.map((point) => point.value),
                  timestamps,
                },
              ]}
              height={220}
              options={{
                drawStyle: 'line',
                thresholds: threshold
                  ? [
                      {
                        value: threshold.threshold,
                        label: t('workbench.preview.threshold'),
                        color: 'var(--yellow)',
                      },
                    ]
                  : [],
              }}
              {...(from !== undefined && to !== undefined
                ? { xDomain: [from, to] as [number, number] }
                : {})}
              showLegend
            />
            <dl className="mt-4 grid grid-cols-2 gap-x-3 sm:grid-cols-4 xl:grid-cols-2 min-[1560px]:grid-cols-4">
              <PreviewStat
                label={t('workbench.preview.current')}
                value={formatNumber(currentValue)}
              />
              <PreviewStat
                label={t('workbench.preview.threshold')}
                value={
                  threshold
                    ? `${COMPARISON_LABEL[threshold.operator]} ${threshold.threshold}`
                    : '—'
                }
              />
              <PreviewStat
                label={t('workbench.preview.estimated')}
                value={estimatedEpisodes === null ? '—' : String(estimatedEpisodes)}
              />
              <PreviewStat
                label={t('workbench.preview.cost')}
                value={
                  scannedRows === undefined || tookMs === undefined
                    ? '—'
                    : `${scannedRows.toLocaleString()} · ${tookMs}ms`
                }
              />
            </dl>
          </>
        )}
      </div>
    </section>
  );
}

function PreviewMessage({
  title,
  description,
  iconClassName,
}: {
  title: React.ReactNode;
  description: React.ReactNode;
  iconClassName?: string;
}) {
  return (
    <div className="flex min-h-[220px] flex-col items-center justify-center text-center">
      <Database className={cn('h-7 w-7 text-tx-3', iconClassName)} />
      <div className="mt-3 text-sm font-strong text-tx-0">{title}</div>
      <div className="mt-1 max-w-sm text-xs leading-relaxed text-tx-2">
        {description}
      </div>
    </div>
  );
}

function PreviewStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 px-1 py-2.5">
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className="mt-1 truncate font-mono text-sm font-semibold text-tx-0">{value}</dd>
    </div>
  );
}

export function ValidationSummary({
  identityReady,
  queryReady,
  thresholdsReady,
  runbookReady,
}: {
  identityReady: boolean;
  queryReady: boolean;
  thresholdsReady: boolean;
  runbookReady: boolean;
}) {
  const { t } = useTranslation('alerts');
  const items = [
    { ready: identityReady, label: t('workbench.validation.identity') },
    { ready: queryReady, label: t('workbench.validation.query') },
    { ready: thresholdsReady, label: t('workbench.validation.thresholds') },
    { ready: runbookReady, label: t('workbench.validation.runbook') },
  ];
  return (
    <section data-alert-validation className="min-w-0 bg-transparent">
      <header className="flex min-h-12 items-center gap-2 border-b border-bd-0 py-3">
        <Check className="h-4 w-4 text-green-soft" />
        <h2 className="font-sans text-sm font-display-strong text-tx-0">
          {t('workbench.validation.title')}
        </h2>
      </header>
      <ul className="space-y-1 pt-3">
        {items.map((item) => (
          <li key={item.label} className="flex min-h-9 items-center gap-2 py-1.5">
            {item.ready ? <Dot tone="green" /> : <Dot tone="dim" />}
            <span className={cn('text-xs', item.ready ? 'text-tx-1' : 'text-tx-3')}>
              {item.label}
            </span>
            <span className="ml-auto">
              <Pill tone={item.ready ? 'green' : 'dim'}>
                {item.ready
                  ? t('workbench.validation.ready')
                  : t('workbench.validation.pending')}
              </Pill>
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function formatNumber(value?: number): string {
  if (value === undefined) return '—';
  if (Math.abs(value) >= 1000) {
    return value.toLocaleString(undefined, { maximumFractionDigits: 2 });
  }
  return value.toLocaleString(undefined, { maximumFractionDigits: 4 });
}
