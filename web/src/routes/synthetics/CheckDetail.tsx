import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Edit3, Pause, Play, RefreshCw, Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, NavLink, useLocation, useNavigate, useParams } from 'react-router-dom';

import { DataTable, KpiStrip, MetadataStrip, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type {
  MonitorRevision,
  ProbeLocation,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import { KindLabel, Section, StatePill, SyntheticsPage, WorkspaceBoundary } from './components';
import {
  activeRevision,
  formatDuration,
  formatPercent,
  formatTimestamp,
  monitorTarget,
  operationalResults,
  resultDurationMicros,
  scheduleText,
  successRate,
} from './model';
import { ResultDrawer } from './ResultDrawer';

const DETAIL_TABS = ['overview', 'results', 'configuration', 'revisions'] as const;

export function CheckDetail() {
  const { t, i18n } = useTranslation('synthetics');
  const { monitorId, resultId } = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.manage', access);
  const detailQuery = useQuery({
    queryKey: ['synthetics', 'monitor', monitorId],
    queryFn: () => syntheticsApi.getMonitor(monitorId!),
    enabled: Boolean(monitorId),
  });
  const resultQuery = useQuery({
    queryKey: ['synthetics', 'monitor', monitorId, 'results', 100],
    queryFn: () => syntheticsApi.listResults(monitorId!, { limit: 100 }),
    enabled: Boolean(monitorId),
  });
  const locationsQuery = useQuery({
    queryKey: ['synthetics', 'locations'],
    queryFn: syntheticsApi.listLocations,
  });
  const detail = detailQuery.data;
  const monitor = detail?.monitor;
  const revision = activeRevision(detail);
  const results = resultQuery.data ?? [];
  const currentTab = detailTab(location.pathname);
  const selectedResult = results.find((result) => result.id === resultId);
  const selectedRevision = detail?.revisions.find(
    (item) => item.id === selectedResult?.monitor_revision_id,
  );
  const isDraftRevision = Boolean(
    monitor?.draft_revision_id && monitor.draft_revision_id === revision?.id,
  );
  const canPublishRevision = Boolean(
    revision && (revision.spec.kind === 'heartbeat' || revision.last_test_passed_at),
  );
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics'] });
  const run = useMutation({
    mutationFn: () =>
      isDraftRevision
        ? syntheticsApi.testRevision(monitorId!, revision!.id)
        : syntheticsApi.runMonitor(monitorId!),
    onSuccess: () => toast.success(t('checks.run_queued')),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const publish = useMutation({
    mutationFn: () => syntheticsApi.publishRevision(monitorId!, revision!.id),
    onSuccess: async () => {
      toast.success(t('checks.published'));
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const lifecycle = useMutation({
    mutationFn: () =>
      monitor?.lifecycle === 'paused'
        ? syntheticsApi.resumeMonitor(monitorId!)
        : syntheticsApi.pauseMonitor(monitorId!),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const pending = detailQuery.isPending || resultQuery.isPending || locationsQuery.isPending;
  const error = detailQuery.error ?? resultQuery.error ?? locationsQuery.error;

  return (
    <SyntheticsPage
      title={monitor?.name ?? t('checks.title')}
      subtitle={monitor ? `${t(`kinds.${monitor.kind}`)} · ${monitorTarget(revision?.spec)}` : t('states.loading')}
      toolbar={
        monitor && (
          <div className="flex items-center gap-2">
            <ChromeButton onClick={() => void Promise.all([detailQuery.refetch(), resultQuery.refetch()])}>
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('actions.refresh')}
            </ChromeButton>
            {canManage && (
              <>
                {monitor.active_revision_id && (
                  <ChromeButton onClick={() => lifecycle.mutate()} disabled={lifecycle.isPending}>
                    {monitor.lifecycle === 'paused' ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
                    {monitor.lifecycle === 'paused' ? t('actions.resume') : t('actions.pause')}
                  </ChromeButton>
                )}
                <ChromeButton onClick={() => navigate(`/synthetics/checks/${monitor.id}/edit`)}>
                  <Edit3 aria-hidden className="h-3.5 w-3.5" />
                  {t('actions.edit')}
                </ChromeButton>
                {revision?.spec.kind !== 'heartbeat' && (
                  <ChromeButton
                    variant={isDraftRevision ? 'default' : 'primary'}
                    onClick={() => run.mutate()}
                    disabled={run.isPending || (!isDraftRevision && monitor.lifecycle !== 'active')}
                  >
                    <Play aria-hidden className="h-3.5 w-3.5" />
                    {t(isDraftRevision ? 'actions.test_revision' : 'actions.run_now')}
                  </ChromeButton>
                )}
                {isDraftRevision && (
                  <ChromeButton
                    variant="primary"
                    onClick={() => publish.mutate()}
                    disabled={!canPublishRevision || publish.isPending}
                    disabledReason={!canPublishRevision ? t('checks.publish_requires_test') : undefined}
                  >
                    <Upload aria-hidden className="h-3.5 w-3.5" />
                    {t('actions.publish')}
                  </ChromeButton>
                )}
              </>
            )}
          </div>
        )
      }
      bodyClassName="pt-0"
    >
      <WorkspaceBoundary pending={pending} error={error} onRetry={() => void Promise.all([detailQuery.refetch(), resultQuery.refetch(), locationsQuery.refetch()])}>
        {monitor && revision && (
          <>
            <DetailNavigation monitorId={monitor.id} currentTab={currentTab} />
            {currentTab === 'overview' && (
              <OverviewTab
                monitor={monitor}
                revision={revision}
                results={results}
                locations={locationsQuery.data ?? []}
                locale={i18n.language}
              />
            )}
            {currentTab === 'results' && (
              <Section title={t('detail.results')} description={t('results.subtitle')}>
                <div className="overflow-x-auto">
                  <DataTable
                    rows={results}
                    columns={resultColumns(locationsQuery.data ?? [], i18n.language, t)}
                    rowKey={(result) => result.id}
                    onRowClick={(result) => navigate(`/synthetics/checks/${monitor.id}/results/${result.id}`)}
                    emptyLabel={t('states.no_results')}
                    className="min-w-[760px]"
                  />
                </div>
              </Section>
            )}
            {currentTab === 'configuration' && <ConfigurationTab revision={revision} locations={locationsQuery.data ?? []} />}
            {currentTab === 'revisions' && (
              <RevisionsTab
                revisions={detail.revisions}
                activeId={monitor.active_revision_id}
                draftId={monitor.draft_revision_id}
                locale={i18n.language}
              />
            )}
          </>
        )}
      </WorkspaceBoundary>
      <ResultDrawer
        result={selectedResult}
        monitor={monitor}
        revision={selectedRevision}
        locations={locationsQuery.data ?? []}
        onClose={() => navigate(`/synthetics/checks/${monitorId}/results`, { replace: true })}
      />
    </SyntheticsPage>
  );
}

function DetailNavigation({ monitorId, currentTab }: { monitorId: string; currentTab: (typeof DETAIL_TABS)[number] }) {
  const { t } = useTranslation('synthetics');
  return <nav aria-label={t('navigation_label')} className="-mx-6 flex min-w-0 gap-1 overflow-x-auto border-b border-bd-0 bg-bg-1 px-6"><Link to="/synthetics/checks" className="mr-2 inline-flex h-11 items-center text-xs font-strong text-tx-2 hover:text-tx-0">← {t('checks.title')}</Link>{DETAIL_TABS.map((tab) => <NavLink key={tab} to={`/synthetics/checks/${monitorId}${tab === 'overview' ? '' : `/${tab}`}`} className={cn('inline-flex h-11 shrink-0 items-center border-b-2 px-3 text-xs font-strong', currentTab === tab ? 'border-indigo text-tx-0' : 'border-transparent text-tx-2 hover:text-tx-0')}>{t(`detail.${tab}`)}</NavLink>)}</nav>;
}

function OverviewTab({ monitor, revision, results, locations, locale }: { monitor: SyntheticMonitor; revision: MonitorRevision; results: SyntheticResult[]; locations: ProbeLocation[]; locale: string }) {
  const { t } = useTranslation('synthetics');
  const metricResults = operationalResults(results);
  const latest = metricResults[0];
  const passed = successRate(metricResults);
  const locationResults = locations.map((location) => ({ location, result: metricResults.find((item) => item.location_id === location.id) })).filter((item) => revision.location_ids.includes(item.location.id));
  const correlationLinks: Array<[string, string]> = [[t('actions.view_trace'), '/traces'], [t('actions.view_logs'), '/logs'], [t('actions.view_metrics'), '/metrics'], [t('actions.open_incident'), '/alerts/incidents'], [t('actions.ask_agent'), '/agent']];
  return <div className="space-y-5 pt-5"><KpiStrip items={[{ label: t('detail.current_state'), value: <StatePill state={monitor.state} />, sub: t(`states.${monitor.lifecycle}`) }, { label: t('checks.columns.success_rate'), value: formatPercent(passed), sub: t('units.results', { count: metricResults.length }), tone: passed !== undefined && passed < 0.99 ? 'warn' : 'good' }, { label: t('checks.columns.latency'), value: formatDuration(resultDurationMicros(latest)), sub: latest ? formatTimestamp(latest.started_at, locale) : t('states.no_results') }, { label: t('checks.columns.interval'), value: scheduleText(revision.schedule), sub: t('detail.thresholds', { failures: revision.consecutive_failures, recoveries: revision.consecutive_recoveries }) }]} /><div className="grid gap-5 xl:grid-cols-[1.2fr_0.8fr]"><Section title={t('detail.location_matrix')}><div className="grid gap-2 p-4 sm:grid-cols-2">{locationResults.map(({ location, result }) => <div key={location.id} className="flex items-center justify-between gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 py-3"><div><div className="text-sm font-strong text-tx-0">{location.name}</div><div className="mt-0.5 text-type-micro text-tx-3">{location.code} · {formatDuration(resultDurationMicros(result))}</div></div><StatePill state={result?.outcome ?? 'unknown'} compact /></div>)}</div></Section><Section title={t('detail.correlation')}><div className="grid gap-2 p-4">{correlationLinks.map(([label, to]) => <Link key={to} to={to} className="flex min-h-10 items-center justify-between rounded-md border border-bd-0 bg-bg-2 px-3 text-xs font-strong text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3">{label}<span aria-hidden>→</span></Link>)}</div></Section></div></div>;
}

function ConfigurationTab({ revision, locations }: { revision: MonitorRevision; locations: ProbeLocation[] }) {
  const { t } = useTranslation('synthetics');
  return <div className="space-y-5 pt-5"><MetadataStrip items={[{ label: t('checks.columns.type'), value: <KindLabel kind={revision.spec.kind} /> }, { label: t('checks.columns.target'), value: monitorTarget(revision.spec) }, { label: t('checks.columns.interval'), value: scheduleText(revision.schedule) }, { label: t('editor.timeout'), value: `${revision.timeout_millis} ms` }]} /><div className="grid gap-5 xl:grid-cols-2"><Section title={t('editor.locations')}><div className="flex flex-wrap gap-2 p-4">{revision.location_ids.map((id) => <span key={id} className="rounded-full border border-bd-0 bg-bg-2 px-2.5 py-1 text-xs text-tx-1">{locations.find((location) => location.id === id)?.name ?? id}</span>)}</div></Section><Section title={t('editor.schedule')}><dl className="grid grid-cols-2 gap-3 p-4 text-xs"><dt className="text-tx-3">{t('editor.retries')}</dt><dd className="text-right text-tx-1">{revision.max_retries}</dd><dt className="text-tx-3">{t('editor.failure_threshold')}</dt><dd className="text-right text-tx-1">{revision.consecutive_failures}</dd><dt className="text-tx-3">{t('editor.recovery_threshold')}</dt><dd className="text-right text-tx-1">{revision.consecutive_recoveries}</dd><dt className="text-tx-3">{t('editor.alert_on_degraded')}</dt><dd className="text-right text-tx-1">{t(revision.alert_on_degraded ? 'common.yes' : 'common.no')}</dd></dl></Section></div><Section title={t('detail.configuration')}><pre className="max-h-[420px] overflow-auto p-4 font-code text-xs leading-relaxed text-tx-1">{JSON.stringify('configuration' in revision.spec ? revision.spec.configuration : revision.spec, null, 2)}</pre></Section></div>;
}

function RevisionsTab({ revisions, activeId, draftId, locale }: { revisions: MonitorRevision[]; activeId: string | undefined; draftId: string | undefined; locale: string }) {
  const { t } = useTranslation('synthetics');
  return <div className="pt-5"><Section title={t('detail.revision_history')}><DataTable rows={revisions} columns={[{ key: 'number', header: t('detail.revision'), cell: (revision) => `#${revision.number}` }, { key: 'status', header: t('checks.columns.status'), cell: (revision) => revision.id === activeId ? <span className="text-green">{t('states.active')}</span> : revision.id === draftId ? <span className="text-yellow-soft">{t('states.draft')}</span> : t('detail.previous_revision') }, { key: 'created', header: t('detail.created'), cell: (revision) => formatTimestamp(revision.created_at, locale) }, { key: 'hash', header: t('detail.content_hash'), cell: (revision) => <span className="font-code text-xs">{revision.content_hash.slice(0, 16)}</span> }]} rowKey={(revision) => revision.id} /></Section></div>;
}

function resultColumns(locations: ProbeLocation[], locale: string, t: TFunction<'synthetics'>): DataTableColumn<SyntheticResult>[] { return [{ key: 'outcome', header: t('results.columns.outcome'), cell: (result) => <StatePill state={result.outcome} compact /> }, { key: 'run_type', header: t('results.columns.run_type'), cell: (result) => t(result.is_test ? 'results.test_run' : 'results.operational_run') }, { key: 'location', header: t('results.columns.location'), cell: (result) => locations.find((location) => location.id === result.location_id)?.name ?? result.location_id }, { key: 'started', header: t('results.columns.started'), cell: (result) => formatTimestamp(result.started_at, locale) }, { key: 'duration', header: t('results.columns.duration'), cell: (result) => formatDuration(resultDurationMicros(result)) }, { key: 'attempts', header: t('results.columns.attempts'), cell: (result) => result.attempts.length }]; }

function detailTab(pathname: string): (typeof DETAIL_TABS)[number] { if (pathname.includes('/configuration')) return 'configuration'; if (pathname.includes('/revisions')) return 'revisions'; if (pathname.includes('/results')) return 'results'; return 'overview'; }
