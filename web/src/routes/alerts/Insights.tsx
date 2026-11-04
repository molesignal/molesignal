import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as alertSourcesApi from '@/api/alertSources';
import type { AlertSource } from '@/api/alertSources';
import * as incidentsApi from '@/api/incidents';
import type { AlertInsights } from '@/api/incidents';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { MANUAL_RULE_ID } from '@/shell/incident/origin';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { Readout } from '@/shell/readout/Readout';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import type { AlertRule, Incident, Severity } from '@/types/alerting';

import { AlertsPage } from './Frame';
import { formatDuration, formatPercent } from './insights/format';
import { IncidentTimeline } from './insights/IncidentTimeline';
import { OncallLoadSection } from './insights/OncallLoad';
import { ResponseSection } from './insights/Response';
import { SectionHeading } from './insights/SectionHeading';

const WINDOW_OPTIONS = [
  { value: 86_400, labelKey: 'last_24h' },
  { value: 604_800, labelKey: 'last_7d' },
  { value: 2_592_000, labelKey: 'last_30d' },
] as const;

type WindowSecs = (typeof WINDOW_OPTIONS)[number]['value'];
type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];

const SEVERITY_ORDER: Severity[] = ['critical', 'error', 'warning', 'info'];
const SEVERITY_STYLE: Record<Severity, { bar: string; dot: string; text: string }> = {
  critical: { bar: 'bg-red', dot: 'bg-red', text: 'text-red' },
  error: { bar: 'bg-orange', dot: 'bg-orange', text: 'text-orange' },
  warning: { bar: 'bg-yellow', dot: 'bg-yellow', text: 'text-yellow' },
  info: { bar: 'bg-blue', dot: 'bg-blue', text: 'text-blue' },
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
  // Incidents an alert source opened count under the source, whose id stands where a rule's would.
  const sourcesQuery = useQuery({
    queryKey: ['alert-sources'],
    queryFn: () => alertSourcesApi.list(),
    staleTime: 30_000,
    retry: false,
  });
  const incidentsQuery = useQuery({
    queryKey: ['alerts', 'insights-incidents', windowSecs],
    queryFn: () => incidentsApi.list({ scope: 'all', windowSecs: windowSecs }),
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
  const sourcesById = React.useMemo(
    // Names are a nicety here: an answer that is not a list names nothing rather than breaking the page.
    () => new Map((Array.isArray(sourcesQuery.data) ? sourcesQuery.data : []).map((source) => [source.id, source])),
    [sourcesQuery.data],
  );

  const pageState: ProductStateProps | null =
    state === 'loading'
      ? { variant: 'loading' }
      : state === 'error'
        ? {
            variant: 'error',
            title: t('insights.error_title'),
            error: insightsQuery.error ?? incidentsQuery.error,
            action: (
              <ChromeButton
                onClick={() => {
                  void insightsQuery.refetch();
                  void incidentsQuery.refetch();
                }}
              >
                {t('center.retry')}
              </ChromeButton>
            ),
          }
        : state === 'empty'
          ? { variant: 'empty', title: t('insights.empty_title'), description: t('insights.empty_description') }
          : null;

  return (
    <AlertsPage
      // What the tab is looking at is said where the tabs are, not in the page's header.
      tabsEnd={
        <Select
          value={String(windowSecs)}
          onValueChange={(value) => setWindowSecs(Number(value) as WindowSecs)}
        >
          <SelectTrigger className="h-8 w-[150px]" aria-label={t('insights.window.label')}>
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
      {...(pageState ? { state: pageState } : { contentSurface: false })}
    >
      {!pageState && data && incidentsQuery.data && (
        <InsightsDashboard
          data={data}
          incidents={incidentsQuery.data}
          locale={i18n.resolvedLanguage ?? i18n.language}
          rulesById={rulesById}
          sourcesById={sourcesById}
          t={t}
          windowSecs={windowSecs}
          windowLabel={t(
            `insights.window.${WINDOW_OPTIONS.find((option) => option.value === windowSecs)?.labelKey ?? 'last_7d'}`,
          )}
        />
      )}
    </AlertsPage>
  );
}

function InsightsDashboard({
  data,
  incidents,
  locale,
  rulesById,
  sourcesById,
  t,
  windowLabel,
  windowSecs,
}: {
  data: AlertInsights;
  incidents: Incident[];
  locale: string;
  rulesById: Map<string, AlertRule>;
  sourcesById: Map<string, AlertSource>;
  t: TFn;
  windowLabel: string;
  windowSecs: WindowSecs;
}) {
  const recoveredRate = data.total > 0 ? data.closed / data.total : 0;
  const rapidRecoveryCount = Math.round(data.closed * data.noiseRate);
  const headline = data.active
    ? t('insights.summary.active_headline', { total: data.total, active: data.active })
    : t('insights.summary.recovered_headline', { total: data.total });

  const ruleItems = data.topRules.map((item) => {
    const rule = rulesById.get(item.key);
    const source = rule ? undefined : sourcesById.get(item.key);

    if (source) {
      return {
        ...item,
        label: source.name,
        secondary: t(`alert_sources.kinds.${source.kind}`),
        to: `/alerts/sources/${encodeURIComponent(source.id)}`,
      };
    }

    // Every incident opened by hand counts under one row.
    if (!rule && item.key === MANUAL_RULE_ID) {
      return { ...item, label: t('sources.manual'), secondary: t('insights.rules.manual_secondary') };
    }

    return {
      ...item,
      label: rule?.name ?? t('insights.rules.unknown_rule'),
      secondary: rule
        ? [rule.query.stream?.name, rule.query.stream?.streamType].filter(Boolean).join(' · ')
        : shortIdentifier(item.key),
      ...(rule ? { to: `/alerts/rules/${encodeURIComponent(rule.id)}/edit` } : {}),
    };
  });
  const serviceItems = data.topServices.map((item) => ({ ...item, label: item.key }));

  return (
    <div className="mx-auto w-full max-w-[2200px] space-y-[12px]">
      <Statement
        label={t('insights.summary.label')}
        tone={data.active ? 'var(--yellow)' : 'var(--green)'}
        eyebrow={windowLabel}
        title={headline}
        titleClassName={data.active ? undefined : 'text-green'}
        detail={t(
          data.active ? 'insights.summary.active_description' : 'insights.summary.recovered_description',
        )}
        readouts={
          <>
            <Readout
              className={STATEMENT_CELL}
              riseIndex={1}
              label={t('insights.summary.recovered')}
              value={formatPercent(recoveredRate)}
              tone={data.active > 0 ? undefined : 'good'}
              detail={t('insights.summary.recovered_of', { closed: data.closed, total: data.total })}
            />
            <Readout
              className={STATEMENT_CELL}
              riseIndex={2}
              label={t('insights.summary.mttr')}
              value={data.mttrSecs > 0 ? formatDuration(data.mttrSecs, t) : '—'}
              detail={t('insights.summary.mttr_description')}
            />
            <Readout
              className={STATEMENT_CELL}
              riseIndex={3}
              label={t('insights.summary.noise')}
              value={String(rapidRecoveryCount)}
              tone={data.noiseRate > 0.3 ? 'warn' : undefined}
              detail={t('insights.summary.noise_description', { rate: formatPercent(data.noiseRate) })}
            />
          </>
        }
      />

      <section className="grid gap-10 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)] xl:grid-cols-[minmax(0,2fr)_minmax(280px,1fr)]">
        <IncidentTimeline
          incidents={incidents}
          locale={locale}
          t={t}
          windowSecs={windowSecs}
        />
        <SeverityBreakdown data={data} t={t} />
      </section>

      <ResponseSection response={data.response} t={t} />

      <OncallLoadSection windowSecs={windowSecs} t={t} />

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

function SeverityBreakdown({ data, t }: { data: AlertInsights; t: TFn }) {
  const entries = SEVERITY_ORDER.map((severity) => ({
    severity,
    count: data.bySeverity[severity] ?? 0,
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
        <div className="mt-4 rounded-md bg-[var(--control-surface)] p-3">
          <p className="font-sans text-sm font-strong text-tx-1">{emptyTitle}</p>
          <p className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-3">{emptyDescription}</p>
        </div>
      ) : (
        <ol className="mt-1.5 space-y-1">
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
                  {item.secondary && <p className="mt-0.5 truncate text-xs text-tx-2">{item.secondary}</p>}
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

function shortIdentifier(value: string): string {
  return value.length > 12 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value;
}
