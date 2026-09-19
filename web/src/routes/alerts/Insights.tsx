import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import type { AlertInsights } from '@/api/incidents';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { cn } from '@/shell/lib/cn';
import { LoadingState } from '@/shell/LoadingState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import type { AlertRule, Incident, Severity } from '@/types/alerting';

import { IncidentTimeline } from './insights/IncidentTimeline';
import { AlertsSubNav } from './Layout';

const WINDOW_OPTIONS = [
  { value: 86_400, labelKey: 'last_24h' },
  { value: 604_800, labelKey: 'last_7d' },
  { value: 2_592_000, labelKey: 'last_30d' },
] as const;

type WindowSecs = (typeof WINDOW_OPTIONS)[number]['value'];
type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];

const SEVERITY_ORDER: Severity[] = ['critical', 'error', 'warning', 'info'];
const SEVERITY_STYLE: Record<Severity, { bar: string; dot: string; text: string }> = {
  critical: { bar: 'bg-red', dot: 'bg-red', text: 'text-red-soft' },
  error: { bar: 'bg-orange', dot: 'bg-orange', text: 'text-orange-soft' },
  warning: { bar: 'bg-yellow', dot: 'bg-yellow', text: 'text-yellow-soft' },
  info: { bar: 'bg-blue', dot: 'bg-blue', text: 'text-blue-soft' },
};

export function AlertsInsights() {
  const { t, i18n } = useTranslation('alerts');
  const [windowSecs, setWindowSecs] = React.useState<WindowSecs>(604_800);
  const insightsQuery = useQuery({
    queryKey: ['alerts-insights', windowSecs],
    queryFn: () => incidentsApi.insights(windowSecs),
  });
  const rulesQuery = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: alertsApi.list,
    staleTime: 30_000,
    retry: false,
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'insights-incidents', windowSecs],
    queryFn: () => incidentsApi.list({ scope: 'all', window_secs: windowSecs }),
    staleTime: 30_000,
    retry: false,
  });
  const data = insightsQuery.data;
  const state = queryStateFor({
    isLoading: insightsQuery.isLoading || incidentsQuery.isLoading,
    isError: insightsQuery.isError || incidentsQuery.isError,
    data: data && data.total > 0 && incidentsQuery.data ? data : null,
  });
  const rulesById = React.useMemo(
    () => new Map((rulesQuery.data ?? []).map((rule) => [rule.id, rule])),
    [rulesQuery.data],
  );

  return (
    <>
      <PageHeader
        title={t('insights.title')}
        subtitle={t('insights.subtitle')}
        toolbar={
          <Select
            value={String(windowSecs)}
            onValueChange={(value) => setWindowSecs(Number(value) as WindowSecs)}
          >
            <SelectTrigger className="w-[156px]" aria-label={t('insights.window.label')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent align="end">
              {WINDOW_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={String(option.value)}>
                  {t(`insights.window.${option.labelKey}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }
      />
      <AlertsSubNav />
      <PageBody>
        {state === 'loading' && <LoadingState variant="list" rows={5} />}
        {state === 'error' && (
          <ErrorState
            error={insightsQuery.error ?? incidentsQuery.error}
            title={t('insights.error_title')}
            onRetry={() => {
              void insightsQuery.refetch();
              void incidentsQuery.refetch();
            }}
          />
        )}
        {state === 'empty' && (
          <EmptyState
            strategy="query-first"
            title={t('insights.empty_title')}
            description={t('insights.empty_description')}
          />
        )}
        {state === null && data && incidentsQuery.data && (
          <InsightsDashboard
            data={data}
            incidents={incidentsQuery.data}
            locale={i18n.resolvedLanguage ?? i18n.language}
            rulesById={rulesById}
            t={t}
            windowSecs={windowSecs}
            windowLabel={t(
              `insights.window.${WINDOW_OPTIONS.find((option) => option.value === windowSecs)?.labelKey ?? 'last_7d'}`,
            )}
          />
        )}
      </PageBody>
    </>
  );
}

function InsightsDashboard({
  data,
  incidents,
  locale,
  rulesById,
  t,
  windowLabel,
  windowSecs,
}: {
  data: AlertInsights;
  incidents: Incident[];
  locale: string;
  rulesById: Map<string, AlertRule>;
  t: TFn;
  windowLabel: string;
  windowSecs: WindowSecs;
}) {
  const recoveredRate = data.total > 0 ? data.closed / data.total : 0;
  const rapidRecoveryCount = Math.round(data.closed * data.noise_rate);
  const headline = data.active
    ? t('insights.summary.active_headline', { total: data.total, active: data.active })
    : t('insights.summary.recovered_headline', { total: data.total });

  const ruleItems = data.top_rules.map((item) => {
    const rule = rulesById.get(item.key);
    return {
      ...item,
      label: rule?.name ?? t('insights.rules.unknown_rule'),
      secondary: rule
        ? [rule.query.stream?.name, rule.query.stream?.stream_type].filter(Boolean).join(' · ')
        : shortIdentifier(item.key),
      ...(rule ? { to: `/alerts/rules/${encodeURIComponent(rule.id)}/edit` } : {}),
    };
  });
  const serviceItems = data.top_services.map((item) => ({ ...item, label: item.key }));

  return (
    <div className="mx-auto w-full max-w-[2200px] space-y-[12px]">
      <section aria-labelledby="insights-summary" className="rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
        <div className="flex flex-col gap-2 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <p className="font-sans text-xs font-strong uppercase tracking-wider text-tx-3">{windowLabel}</p>
            <h2 id="insights-summary" className="mt-1 font-sans text-2xl font-display-strong text-tx-0">
              {headline}
            </h2>
            <p className="mt-1 max-w-2xl font-sans text-sm text-tx-2">
              {t(
                data.active
                  ? 'insights.summary.active_description'
                  : 'insights.summary.recovered_description',
              )}
            </p>
          </div>
        </div>

        <dl className="mt-6 grid gap-[12px] sm:grid-cols-2 xl:grid-cols-4">
          <SummaryMetric
            label={t('insights.summary.total')}
            value={String(data.total)}
            description={t('insights.summary.total_description')}
          />
          <SummaryMetric
            label={t('insights.summary.mttr')}
            value={data.mttr_secs > 0 ? formatDuration(data.mttr_secs, t) : '—'}
            description={t('insights.summary.mttr_description')}
          />
          <SummaryMetric
            label={t('insights.summary.status')}
            value={t('insights.summary.status_value', { active: data.active, closed: data.closed })}
            description={t('insights.summary.status_description', {
              percent: formatPercent(recoveredRate),
            })}
            tone={data.active > 0 ? 'warn' : 'good'}
          />
          <SummaryMetric
            label={t('insights.summary.noise')}
            value={t('insights.summary.noise_value', { count: rapidRecoveryCount })}
            description={t('insights.summary.noise_description', {
              rate: formatPercent(data.noise_rate),
            })}
            tone={data.noise_rate > 0.3 ? 'warn' : 'neutral'}
          />
        </dl>
      </section>

      <section className="grid gap-10 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)] xl:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
        <IncidentTimeline
          incidents={incidents}
          locale={locale}
          t={t}
          windowSecs={windowSecs}
        />
        <SeverityBreakdown data={data} t={t} />
      </section>

      <section className="grid gap-12 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)] xl:grid-cols-2">
        <RankedList
          sectionId="insights-services"
          title={t('insights.services.title')}
          description={t('insights.services.description')}
          items={serviceItems}
          total={data.total}
          emptyTitle={t('insights.services.empty_title')}
          emptyDescription={t('insights.services.empty_description')}
          t={t}
        />
        <RankedList
          sectionId="insights-rules"
          title={t('insights.rules.title')}
          description={t('insights.rules.description')}
          items={ruleItems}
          total={data.total}
          emptyTitle={t('insights.rules.empty_title')}
          emptyDescription={t('insights.rules.empty_description')}
          t={t}
        />
      </section>
    </div>
  );
}

function SummaryMetric({
  label,
  value,
  description,
  tone = 'neutral',
}: {
  label: string;
  value: string;
  description: string;
  tone?: 'neutral' | 'good' | 'warn';
}) {
  return (
    <div className="rounded-md bg-[var(--control-surface)] p-3">
      <dt className="font-sans text-xs font-strong uppercase tracking-wider text-tx-3">{label}</dt>
      <dd
        className={cn(
          'mt-1 font-sans text-xl font-display-strong tabular-nums text-tx-0',
          tone === 'good' && 'text-green-soft',
          tone === 'warn' && 'text-orange-soft',
        )}
      >
        {value}
      </dd>
      <p className="mt-1 font-sans text-xs text-tx-2">{description}</p>
    </div>
  );
}

function SeverityBreakdown({ data, t }: { data: AlertInsights; t: TFn }) {
  const entries = SEVERITY_ORDER.map((severity) => ({
    severity,
    count: data.by_severity[severity] ?? 0,
  })).filter(({ count }) => count > 0);

  return (
    <section aria-labelledby="insights-severity">
      <SectionHeading
        id="insights-severity"
        title={t('insights.severity_breakdown.title')}
        description={t('insights.severity_breakdown.description')}
      />
      {entries.length === 0 ? (
        <p className="mt-5 font-sans text-sm text-tx-3">—</p>
      ) : (
        <>
          <div className="mt-6 flex h-2 overflow-hidden rounded-full bg-bg-2" aria-hidden>
            {entries.map(({ severity, count }) => (
              <span
                key={severity}
                className={SEVERITY_STYLE[severity].bar}
                style={{ width: `${(count / data.total) * 100}%` }}
              />
            ))}
          </div>
          <ul className="mt-5 space-y-3">
            {entries.map(({ severity, count }) => (
              <li key={severity} className="flex items-center gap-2 font-sans text-sm">
                <span className={cn('h-2 w-2 rounded-full', SEVERITY_STYLE[severity].dot)} aria-hidden />
                <span className={cn('font-strong', SEVERITY_STYLE[severity].text)}>
                  {t(`severity.${severity}`)}
                </span>
                <span className="ml-auto tabular-nums text-tx-2">
                  {count} · {formatPercent(count / data.total)}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

interface RankedItem {
  key: string;
  count: number;
  label: string;
  secondary?: string;
  to?: string;
}

function RankedList({
  sectionId,
  title,
  description,
  items,
  total,
  emptyTitle,
  emptyDescription,
  t,
}: {
  sectionId: string;
  title: string;
  description: string;
  items: RankedItem[];
  total: number;
  emptyTitle: string;
  emptyDescription: string;
  t: TFn;
}) {
  return (
    <section aria-labelledby={sectionId}>
      <SectionHeading id={sectionId} title={title} description={description} />
      {items.length === 0 ? (
        <div className="mt-5 rounded-md bg-[var(--control-surface)] p-3">
          <p className="font-sans text-sm font-strong text-tx-1">{emptyTitle}</p>
          <p className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-3">{emptyDescription}</p>
        </div>
      ) : (
        <ol className="mt-3 space-y-1">
          {items.map((item) => {
            const percent = item.count / Math.max(1, total);
            const label = item.to ? (
              <Link
                to={item.to}
                className="rounded font-strong text-tx-1 transition-colors hover:text-indigo focus-visible:bg-bg-2 focus-visible:text-indigo"
              >
                {item.label}
              </Link>
            ) : (
              <span className="font-strong text-tx-1">{item.label}</span>
            );
            return (
              <li key={item.key} className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 gap-y-2 py-2">
                <div className="min-w-0 font-sans text-sm">
                  <div className="truncate">{label}</div>
                  {item.secondary && <p className="mt-0.5 truncate text-xs text-tx-3">{item.secondary}</p>}
                </div>
                <span className="font-sans text-xs tabular-nums text-tx-2">
                  {t('insights.rankings.count_and_share', {
                    count: item.count,
                    percent: formatPercent(percent),
                  })}
                </span>
                <span className="col-span-2 h-1.5 overflow-hidden rounded-full bg-bg-2" aria-hidden>
                  <span className="block h-full bg-indigo" style={{ width: `${Math.min(100, percent * 100)}%` }} />
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}

function SectionHeading({
  id,
  title,
  description,
  aside,
}: {
  id: string;
  title: string;
  description: string;
  aside?: string;
}) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
      <div>
        <h3 id={id} className="font-sans text-base font-display-strong text-tx-0">
          {title}
        </h3>
        <p className="mt-0.5 font-sans text-xs text-tx-3">{description}</p>
      </div>
      {aside && <p className="shrink-0 font-sans text-xs tabular-nums text-tx-2">{aside}</p>}
    </div>
  );
}

function formatDuration(secs: number, t: TFn): string {
  const rounded = Math.max(0, Math.round(secs));
  if (rounded < 60) return t('insights.duration.seconds', { count: rounded });
  if (rounded < 3_600) {
    const minutes = Math.floor(rounded / 60);
    const seconds = rounded % 60;
    return seconds
      ? t('insights.duration.minutes_seconds', { minutes, seconds })
      : t('insights.duration.minutes', { count: minutes });
  }
  const hours = Math.floor(rounded / 3_600);
  const minutes = Math.floor((rounded % 3_600) / 60);
  return minutes
    ? t('insights.duration.hours_minutes', { hours, minutes })
    : t('insights.duration.hours', { count: hours });
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(value > 0 && value < 0.01 ? 1 : 0)}%`;
}

function shortIdentifier(value: string): string {
  return value.length > 12 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value;
}
