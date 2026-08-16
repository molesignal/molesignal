import type { TFunction } from 'i18next';
import { ArrowRight, Plus, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import type { ProbeLocation, SyntheticResult } from '@/api/synthetics';
import { hasPermission, useProductAccess } from '@/product/access';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  Section,
  StatePill,
  SyntheticsCanvas,
  SyntheticsKpiBand,
  SyntheticsPage,
  WorkspaceBoundary,
} from './components';
import { useSyntheticsWorkspace, type CheckRow } from './data';
import { WorldAvailabilityMap } from './map/WorldAvailabilityMap';
import {
  formatDuration,
  formatPercent,
  formatRelativeTimestamp,
  percentile,
  resultDurationMicros,
  successRate,
} from './model';

export function SyntheticsOverview() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.manage', access);
  const workspace = useSyntheticsWorkspace({ resultLimit: 30 });
  const activeRows = workspace.rows.filter((row) => row.monitor.lifecycle === 'active');
  const healthy = activeRows.filter((row) => row.monitor.state === 'healthy').length;
  const failing = activeRows.filter((row) => row.monitor.state === 'failing').length;
  const alerting = activeRows.filter(
    (row) => Boolean(row.revision?.escalation_policy_id),
  ).length;
  const knownResults = workspace.operationalResults.filter(
    (result) => result.outcome !== 'unknown' && result.outcome !== 'skipped',
  );
  const availability = successRate(knownResults);
  const p95 = percentile(
    knownResults.map(resultDurationMicros).filter((value): value is number => value !== undefined),
    0.95,
  );
  const failures = workspace.operationalResults
    .filter((result) => result.outcome === 'failing' || result.outcome === 'degraded')
    .slice(0, 6);
  const slowRows = workspace.rows
    .filter((row) => row.latestLatencyMicros !== undefined)
    .sort((left, right) => (right.latestLatencyMicros ?? 0) - (left.latestLatencyMicros ?? 0))
    .slice(0, 6);

  return (
    <SyntheticsPage
      title={t('title')}
      subtitle={t('subtitle')}
      toolbar={
        <div className="flex items-center gap-2">
          <ChromeButton onClick={() => void workspace.refetch()} disabled={workspace.refetching}>
            <RefreshCw aria-hidden className={workspace.refetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'} />
            {t('actions.refresh')}
          </ChromeButton>
          {canManage && (
            <ChromeButton variant="primary" onClick={() => navigate('/synthetics/checks/new')}>
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('actions.create_check')}
            </ChromeButton>
          )}
        </div>
      }
      bodyClassName="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2"
    >
      <WorkspaceBoundary
        pending={workspace.pending}
        error={workspace.error}
        onRetry={() => void workspace.refetch()}
        flat
      >
        {workspace.rows.length === 0 ? (
          <ProductState
            variant="empty"
            title={t('states.empty_title')}
            description={t('states.empty_description')}
            action={canManage ? <ChromeButton variant="primary" onClick={() => navigate('/synthetics/checks/new')}><Plus className="h-3.5 w-3.5" />{t('actions.create_first_check')}</ChromeButton> : undefined}
            className="rounded-none border-x-0 border-t-0 border-solid border-bd-0 bg-transparent"
          />
        ) : (
          <SyntheticsCanvas>
            <SyntheticsKpiBand
              columns={6}
              items={[
                { label: t('overview.running_checks'), value: activeRows.length, sub: t('overview.total_checks', { count: workspace.rows.length }) },
                { label: t('overview.passed'), value: healthy, sub: formatPercent(activeRows.length ? healthy / activeRows.length : undefined), tone: 'good' },
                { label: t('overview.failed'), value: failing, sub: failing ? t('overview.recent_failures') : t('states.healthy'), tone: failing ? 'danger' : 'good' },
                { label: t('overview.availability'), value: formatPercent(availability), sub: t('overview.coverage_hint', { count: knownResults.length, locations: workspace.locations.length }), tone: availability !== undefined && availability < 0.99 ? 'warn' : 'good' },
                { label: t('overview.p95_latency'), value: formatDuration(p95), sub: t('overview.samples', { count: knownResults.length }) },
                { label: t('overview.alerting'), value: `${alerting}/${activeRows.length}`, sub: t('overview.alerting_coverage'), tone: activeRows.length === 0 ? 'neutral' : alerting === activeRows.length ? 'good' : 'warn' },
              ]}
            />

            <div className="grid grid-cols-1 xl:grid-cols-2">
              <TrendPanel
                title={t('overview.success_trend')}
                description={t('overview.success_trend_description')}
                results={workspace.operationalResults}
                metric="success"
              />
              <TrendPanel
                title={t('overview.latency_trend')}
                description={t('overview.latency_trend_description')}
                results={workspace.operationalResults}
                metric="latency"
                className="xl:border-l"
              />
            </div>

            <div className="grid grid-cols-1 2xl:grid-cols-2">
              <Section
                title={t('overview.recent_failures')}
                description={t('overview.recent_failures_description')}
                action={<SectionLink to="/synthetics/results" label={t('actions.view_all')} />}
                flat
              >
                <DataTable
                  rows={failures}
                  columns={failureColumns(workspace.rows, workspace.locations, i18n.language, t)}
                  rowKey={(row) => row.id}
                  onRowClick={(row) => navigate(`/synthetics/checks/${row.monitor_id}/results/${row.id}`)}
                  emptyLabel={t('states.no_results')}
                  className="rounded-none border-0 bg-transparent"
                />
              </Section>
              <Section
                title={t('overview.slow_checks')}
                description={t('overview.slow_checks_description')}
                action={<SectionLink to="/synthetics/checks" label={t('actions.view_all')} />}
                className="2xl:border-l"
                flat
              >
                <DataTable
                  rows={slowRows}
                  columns={slowColumns(i18n.language, t)}
                  rowKey={(row) => row.monitor.id}
                  onRowClick={(row) => navigate(`/synthetics/checks/${row.monitor.id}`)}
                  emptyLabel={t('states.no_results')}
                  className="rounded-none border-0 bg-transparent"
                />
              </Section>
            </div>

            <div className="grid grid-cols-1 2xl:grid-cols-[1.35fr_0.65fr]">
              <Section
                title={t('overview.global_map')}
                description={t('overview.global_map_description')}
                flat
              >
                <WorldAvailabilityMap
                  locations={workspace.locations}
                  results={workspace.operationalResults}
                />
              </Section>
              <Section
                title={t('overview.reliability_loop')}
                description={t('overview.reliability_loop_hint')}
                className="2xl:border-l"
                flat
              >
                <ReliabilityLoop />
              </Section>
            </div>
          </SyntheticsCanvas>
        )}
      </WorkspaceBoundary>
    </SyntheticsPage>
  );
}

function TrendPanel({ title, description, results, metric, className }: { title: string; description: string; results: SyntheticResult[]; metric: 'success' | 'latency'; className?: string }) {
  const { t } = useTranslation('synthetics');
  const ordered = results.slice(0, 120).reverse();
  const series = [{
    id: `synthetics-${metric}`,
    name: title,
    color: metric === 'success' ? 'var(--green)' : 'var(--indigo)',
    timestamps: ordered.map((result) => result.started_at),
    data: ordered.map((result) => metric === 'success' ? (result.outcome === 'healthy' ? 1 : result.outcome === 'unknown' || result.outcome === 'skipped' ? null : 0) : ((resultDurationMicros(result) ?? 0) / 1000)),
    unit: metric === 'success' ? 'percentunit' : 'ms',
  }];
  return <Section title={title} description={description} {...(className ? { className } : {})} flat><div className="p-4">{ordered.length ? <TimeSeriesChart series={series} height={210} ariaLabel={title} options={{ drawStyle: 'line', showPoints: 'auto', compactAxes: true, leftAxis: { min: 0, ...(metric === 'success' ? { max: 1, unit: 'percentunit' } : { unit: 'ms' }) } }} /> : <div className="grid h-[210px] place-items-center text-xs text-tx-3">{t('states.no_results')}</div>}</div></Section>;
}

function failureColumns(rows: CheckRow[], locations: ProbeLocation[], locale: string, t: TFunction<'synthetics'>): DataTableColumn<SyntheticResult>[] {
  return [
    { key: 'check', header: t('results.columns.check'), cell: (result) => rows.find((row) => row.monitor.id === result.monitor_id)?.monitor.name ?? result.monitor_id },
    { key: 'state', header: t('checks.columns.status'), width: 105, cell: (result) => <StatePill state={result.outcome} compact /> },
    { key: 'location', header: t('results.columns.location'), cell: (result) => locations.find((item) => item.id === result.location_id)?.name ?? result.location_id },
    { key: 'time', header: t('results.columns.started'), width: 110, cell: (result) => formatRelativeTimestamp(result.started_at, locale) },
  ];
}

function slowColumns(locale: string, t: TFunction<'synthetics'>): DataTableColumn<CheckRow>[] {
  return [
    { key: 'check', header: t('results.columns.check'), cell: (row) => row.monitor.name },
    { key: 'state', header: t('checks.columns.status'), width: 105, cell: (row) => <StatePill state={row.monitor.state} compact /> },
    { key: 'latency', header: t('checks.columns.latency'), width: 90, cell: (row) => formatDuration(row.latestLatencyMicros) },
    { key: 'time', header: t('checks.columns.last_run'), width: 110, cell: (row) => formatRelativeTimestamp(row.latestResult?.started_at, locale) },
  ];
}

function ReliabilityLoop() {
  const { t } = useTranslation('synthetics');
  const items: Array<[string, string]> = [[t('overview.loop_synthetic'), '/synthetics/results'], [t('overview.loop_alert'), '/alerts/incidents'], [t('overview.loop_evidence'), '/traces'], [t('overview.loop_agent'), '/agent'], [t('overview.loop_status_page'), '/status-pages']];
  return <div className="divide-y divide-bd-0">{items.map(([label, to]) => <Link key={label} to={to} className="flex min-h-12 items-center justify-between px-4 text-sm font-strong text-tx-1 hover:bg-bg-2 focus-visible:bg-bg-2"><span>{label}</span><ArrowRight className="h-3.5 w-3.5 text-tx-3" /></Link>)}</div>;
}

function SectionLink({ to, label }: { to: string; label: string }) {
  return <Link to={to} className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs font-strong text-indigo-soft hover:bg-bg-2 focus-visible:bg-bg-2">{label}<ArrowRight aria-hidden className="h-3 w-3" /></Link>;
}
