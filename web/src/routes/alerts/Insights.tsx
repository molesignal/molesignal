import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { KpiStrip, type KpiStripItem } from '@/admin/page/Primitives';
import * as incidentsApi from '@/api/incidents';
import type { AlertInsights } from '@/api/incidents';
import { Pill, type PillTone } from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';

import { AlertsSubNav } from './Layout';

const SEVERITY_TONE: Record<string, PillTone> = {
  info: 'blue',
  warning: 'yellow',
  error: 'red',
  critical: 'red',
};

export function AlertsInsights() {
  const { t } = useTranslation('alerts');
  const q = useQuery({
    queryKey: ['alerts-insights'],
    queryFn: () => incidentsApi.insights(),
  });
  const data = q.data;
  const state = queryStateFor({
    isLoading: q.isLoading,
    isError: q.isError,
    data: data && data.total > 0 ? data : null,
  });

  const kpis: KpiStripItem[] = data
    ? [
        { label: t('insights.kpis.total', { defaultValue: 'Total incidents' }), value: String(data.total) },
        {
          label: t('insights.kpis.mttr', { defaultValue: 'MTTR' }),
          value: data.mttr_secs > 0 ? formatDuration(data.mttr_secs) : '—',
          sub: t('insights.kpis.mttr_sub', { defaultValue: 'mean across resolved' }),
        },
        {
          label: t('insights.kpis.active', { defaultValue: 'Active' }),
          value: String(data.active),
          sub: t('insights.kpis.active_sub', { defaultValue: 'open + acknowledged' }),
          tone: data.active > 0 ? 'danger' : 'good',
        },
        {
          label: t('insights.kpis.closed', { defaultValue: 'Closed' }),
          value: String(data.closed),
          sub: t('insights.kpis.closed_sub', { defaultValue: 'resolved + closed' }),
          tone: 'good',
        },
        {
          label: t('insights.kpis.noise', { defaultValue: 'Noise rate' }),
          value: `${(data.noise_rate * 100).toFixed(1)}%`,
          sub: t('insights.kpis.noise_sub', { defaultValue: 'resolved < 60s' }),
          tone: data.noise_rate > 0.3 ? 'warn' : 'neutral',
        },
      ]
    : [];

  return (
    <>
      <PageHeader
        title={t('insights.title', { defaultValue: 'Alert insights' })}
        subtitle={t('insights.subtitle', { defaultValue: 'Aggregate view over recent incidents' })}
      />
      <AlertsSubNav />
      <PageBody>
        {state === 'loading' && <LoadingState variant="list" rows={4} />}
        {state === 'error' && (
          <ErrorState
            error={q.error}
            title={t('insights.error_title', { defaultValue: 'Cannot compute insights' })}
            onRetry={() => void q.refetch()}
          />
        )}
        {state === 'empty' && (
          <EmptyState
            strategy="query-first"
            title={t('insights.empty_title', { defaultValue: 'No incidents yet' })}
            description={t('insights.empty_description', {
              defaultValue:
                'Insights summarize MTTR, noise, busiest hours, and which rules and services fire most. They populate as alerts fire and resolve.',
            })}
          />
        )}
        {state === null && data && (
          <div className="flex flex-col gap-5">
            <KpiStrip items={kpis} layout="inline" />
            <HeatmapSection data={data} t={t} />
            <div className="grid gap-4 md:grid-cols-2">
              <TopList
                title={t('insights.top_services', { defaultValue: 'Top affected services' })}
                items={data.top_services}
                empty={t('insights.no_services', { defaultValue: 'No services attributed.' })}
              />
              <TopList
                title={t('insights.top_rules', { defaultValue: 'Top firing rules' })}
                items={data.top_rules}
                empty={t('insights.no_rules', { defaultValue: 'No rules attributed.' })}
              />
            </div>
            <SeveritySection data={data} t={t} />
          </div>
        )}
      </PageBody>
    </>
  );
}

type TFn = ReturnType<typeof useTranslation<'alerts'>>['t'];

function HeatmapSection({ data, t }: { data: AlertInsights; t: TFn }) {
  const max = Math.max(1, ...data.by_hour);
  return (
    <Section title={t('insights.heatmap_title', { defaultValue: 'Incidents by hour of day (UTC)' })}>
      <div className="flex items-end gap-[3px]">
        {data.by_hour.map((count, hour) => (
          <div key={hour} className="flex flex-1 flex-col items-center gap-1" title={`${hour}:00 · ${count}`}>
            <div className="flex h-8 w-full items-end overflow-hidden rounded-sm bg-bg-2">
              <div
                className="w-full rounded-sm bg-indigo"
                style={{ height: `${count === 0 ? 0 : 20 + (count / max) * 80}%` }}
                aria-hidden
              />
            </div>
            {hour % 6 === 0 && <span className="font-sans text-xs tabular-nums text-tx-3">{hour}</span>}
          </div>
        ))}
      </div>
    </Section>
  );
}

function TopList({
  title,
  items,
  empty,
}: {
  title: string;
  items: Array<{ key: string; count: number }>;
  empty: string;
}) {
  const max = Math.max(1, ...items.map((i) => i.count));
  return (
    <Section title={title}>
      {items.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">{empty}</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {items.map((item) => (
            <li key={item.key} className="flex items-center gap-2">
              <span className="w-40 shrink-0 truncate font-sans text-xs text-tx-1" title={item.key}>
                {item.key}
              </span>
              <span className="h-2 flex-1 overflow-hidden rounded-full bg-bg-2">
                <span
                  className="block h-full rounded-full bg-indigo"
                  style={{ width: `${(item.count / max) * 100}%` }}
                  aria-hidden
                />
              </span>
              <span className="w-8 shrink-0 text-right font-sans text-xs tabular-nums text-tx-2">{item.count}</span>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

function SeveritySection({ data, t }: { data: AlertInsights; t: TFn }) {
  const entries = Object.entries(data.by_severity);
  if (entries.length === 0) return null;
  return (
    <Section title={t('insights.by_severity', { defaultValue: 'By severity' })}>
      <ul className="flex flex-wrap gap-2">
        {entries.map(([sev, count]) => (
          <li key={sev}>
            <Pill tone={SEVERITY_TONE[sev] ?? 'dim'}>
              {sev} · {count}
            </Pill>
          </li>
        ))}
      </ul>
    </Section>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="font-sans text-xs font-strong uppercase tracking-wider text-tx-3">{title}</h3>
      {children}
    </section>
  );
}

function formatDuration(secs: number): string {
  if (secs < 60) return `${secs.toFixed(0)}s`;
  if (secs < 3600) return `${(secs / 60).toFixed(1)}m`;
  return `${(secs / 3600).toFixed(1)}h`;
}
