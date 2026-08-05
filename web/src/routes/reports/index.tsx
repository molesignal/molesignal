import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BarChart3,
  Check,
  CircleAlert,
  Clock3,
  Download,
  LayoutTemplate,
  Link2,
  Loader2,
  Plus,
  Search,
  Trash2,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as reportsApi from '@/api/reports';
import * as reportTemplatesApi from '@/api/reports/templates';
import * as savedViewsApi from '@/api/savedViews';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ResourceShareDialog } from '@/sharing/ResourceShareDialog';
import {
  Card,
  ChromeButton,
  Pill,
  QueryInput,
  uiLabelClass,
  uiLabelStrongClass,
} from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  nextRunAtMicros,
  normalizeMicros,
  parseRecipient,
  sanitizeFilename,
} from './reportModel';
import {
  HistoryTable,
  RecipientIcon,
  ScheduleTable,
} from './ReportTables';
import {
  adaptReport,
  builtInTemplates,
  draftFromSeed,
  FORMAT_OPTIONS,
  getSourceName,
  normalizeFormat,
  RANGE_OPTIONS,
  rangeLabel,
  type DeliveryRow,
  type DisplayReport,
  type ReportDraft,
  type ReportTab,
  reportInputFromDisplay,
  reportInputFromDraft,
  SCHEDULE_OPTIONS,
  scheduleLabel,
  type SourceKind,
  type TemplateDraft,
  templateDraftFromPreset,
  templateIcon,
  type TemplatePreset,
  validateDraft,
  type WorkbenchSeed,
  type WorkbenchStep,
} from './reportTypes';

export function Reports() {
  const { t } = useTranslation('reports');
  const createTemplateAccess = useActionAccess({
    permission: 'reports.create',
  });
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const queryClient = useQueryClient();
  const [searchParams] = useSearchParams();
  const linkedReportId = searchParams.get('report');
  const openedLinkedReportRef = React.useRef<string | null>(null);
  const [tab, setTab] = React.useState<ReportTab>('schedules');
  const [scheduleSearch, setScheduleSearch] = React.useState('');
  const [scheduleStatus, setScheduleStatus] = React.useState('all');
  const [scheduleSource, setScheduleSource] = React.useState('all');
  const [historySearch, setHistorySearch] = React.useState('');
  const [historyResult, setHistoryResult] = React.useState('all');
  const [workbenchSeed, setWorkbenchSeed] = React.useState<WorkbenchSeed | null>(null);
  const [exportOpen, setExportOpen] = React.useState(false);
  const [exportReportId, setExportReportId] = React.useState<string | null>(null);
  const [exportingId, setExportingId] = React.useState<string | null>(null);
  const [failedDelivery, setFailedDelivery] = React.useState<DeliveryRow | null>(null);
  const [sharingReport, setSharingReport] =
    React.useState<DisplayReport | null>(null);
  const [templateEditor, setTemplateEditor] = React.useState<TemplatePreset | 'new' | null>(
    null,
  );

  const listQuery = useQuery({
    queryKey: ['reports', 'list'],
    queryFn: () => reportsApi.list(),
  });
  const dashboardsQuery = useQuery({
    queryKey: ['dashboards', 'list'],
    queryFn: () => dashboardsApi.list(),
  });
  const savedViewsQuery = useQuery({
    queryKey: ['saved-views', 'list'],
    queryFn: () => savedViewsApi.list(),
  });
  const templatesQuery = useQuery({
    queryKey: ['reports', 'templates'],
    queryFn: () => reportTemplatesApi.list(),
    enabled: tab === 'templates',
  });

  const reports = React.useMemo(
    () => (listQuery.data ?? []).map(adaptReport),
    [listQuery.data],
  );
  const dashboardNames = React.useMemo(
    () => new Map((dashboardsQuery.data ?? []).map((dashboard) => [dashboard.id, dashboard.title])),
    [dashboardsQuery.data],
  );
  const savedViewNames = React.useMemo(
    () => new Map((savedViewsQuery.data ?? []).map((view) => [view.id, view.name])),
    [savedViewsQuery.data],
  );

  React.useEffect(() => {
    if (
      !linkedReportId ||
      openedLinkedReportRef.current === linkedReportId ||
      !reports.some((report) => report.id === linkedReportId)
    ) {
      return;
    }
    openedLinkedReportRef.current = linkedReportId;
    setTab('schedules');
    setExportReportId(linkedReportId);
    setExportOpen(true);
  }, [linkedReportId, reports]);

  const deliveryQueries = useQueries({
    queries: reports.map((report) => ({
      queryKey: ['reports', 'deliveries', report.id],
      queryFn: () => reportsApi.deliveries(report.id),
      staleTime: 30_000,
    })),
  });
  const deliveryRows = React.useMemo(() => {
    const rows: DeliveryRow[] = [];
    deliveryQueries.forEach((query, index) => {
      const report = reports[index];
      if (!report) return;
      (query.data ?? []).forEach((delivery) => rows.push({ delivery, report }));
    });
    return rows.sort(
      (left, right) =>
        (normalizeMicros(right.delivery.attempted_at) ?? 0) -
        (normalizeMicros(left.delivery.attempted_at) ?? 0),
    );
  }, [deliveryQueries, reports]);

  const latestDeliveryByReport = React.useMemo(() => {
    const result = new Map<string, reportsApi.ReportDelivery>();
    deliveryRows.forEach((row) => {
      if (!result.has(row.report.id)) result.set(row.report.id, row.delivery);
    });
    return result;
  }, [deliveryRows]);

  const templates = React.useMemo(() => {
    const builtIns = builtInTemplates(t);
    const remote: TemplatePreset[] = (templatesQuery.data ?? [])
      .filter((template) => !template.is_builtin)
      .map((template) => ({
        id: `remote:${template.id}`,
        serverId: template.id,
        isBuiltin: false,
        name: template.name,
        description: template.description,
        sourceKind: template.target_type === 'saved_view' ? 'saved_view' : 'dashboard',
        format: normalizeFormat(template.format),
        rangePreset: template.time_range_preset,
        icon: template.target_type === 'saved_view' ? 'query' : 'platform',
      }));
    return [...builtIns, ...remote];
  }, [t, templatesQuery.data]);

  const filteredReports = React.useMemo(() => {
    const query = scheduleSearch.trim().toLocaleLowerCase();
    return reports.filter((report) => {
      const sourceName = getSourceName(report, dashboardNames, savedViewNames);
      const matchesQuery =
        !query ||
        report.title.toLocaleLowerCase().includes(query) ||
        sourceName.toLocaleLowerCase().includes(query);
      const matchesStatus =
        scheduleStatus === 'all' ||
        (scheduleStatus === 'enabled' ? report.enabled : !report.enabled);
      const matchesSource = scheduleSource === 'all' || report.sourceKind === scheduleSource;
      return matchesQuery && matchesStatus && matchesSource;
    });
  }, [
    dashboardNames,
    reports,
    savedViewNames,
    scheduleSearch,
    scheduleSource,
    scheduleStatus,
  ]);

  const filteredHistory = React.useMemo(() => {
    const query = historySearch.trim().toLocaleLowerCase();
    return deliveryRows.filter((row) => {
      const recipient = row.delivery.recipient_target ?? '';
      const matchesQuery =
        !query ||
        row.report.title.toLocaleLowerCase().includes(query) ||
        recipient.toLocaleLowerCase().includes(query);
      const matchesResult =
        historyResult === 'all' || row.delivery.status === historyResult;
      return matchesQuery && matchesResult;
    });
  }, [deliveryRows, historyResult, historySearch]);

  const nowMicros = Date.now() * 1_000;
  const nextDayMicros = nowMicros + 24 * 60 * 60 * 1_000_000;
  const deliveriesNextDay = reports.filter((report) => {
    const nextRun = nextRunAtMicros(report.raw, nowMicros);
    return nextRun !== null && nextRun <= nextDayMicros;
  }).length;
  const thirtyDaysAgo = nowMicros - 30 * 24 * 60 * 60 * 1_000_000;
  const completedLastThirtyDays = deliveryRows.filter((row) => {
    const attemptedAt = normalizeMicros(row.delivery.attempted_at) ?? 0;
    return (
      attemptedAt >= thirtyDaysAgo &&
      (row.delivery.status === 'sent' || row.delivery.status === 'failed')
    );
  });
  const sentLastThirtyDays = completedLastThirtyDays.filter(
    (row) => row.delivery.status === 'sent',
  ).length;
  const failedCount = deliveryRows.filter((row) => row.delivery.status === 'failed').length;
  const successRate = completedLastThirtyDays.length
    ? `${Math.round((sentLastThirtyDays / completedLastThirtyDays.length) * 100)}%`
    : '—';

  const toggleMutation = useMutation({
    mutationFn: ({ report, enabled }: { report: DisplayReport; enabled: boolean }) =>
      reportsApi.update(report.id, reportInputFromDisplay(report, enabled)),
    onSuccess: (_data, variables) => {
      toast.success(variables.enabled ? t('toast.enabled') : t('toast.paused'));
      void queryClient.invalidateQueries({ queryKey: ['reports', 'list'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const downloadReport = React.useCallback(
    async (report: DisplayReport) => {
      setExportingId(report.id);
      try {
        await reportsApi.downloadPreview(
          report.id,
          `${sanitizeFilename(report.title)}.${report.format}`,
        );
        toast.success(t('toast.exported'));
      } catch (error) {
        toast.error(toApiError(error).message);
        throw error;
      } finally {
        setExportingId(null);
      }
    },
    [t],
  );

  const pageState = listQuery.isLoading
    ? ({ variant: 'loading' } as const)
    : listQuery.isError
      ? ({ variant: 'error', error: listQuery.error } as const)
      : null;
  const anyHistoryLoading =
    reports.length > 0 && deliveryQueries.some((query) => query.isLoading);
  const anyHistoryError = deliveryQueries.some((query) => query.isError);

  const actionBar =
    tab === 'schedules' ? (
      <ReportFilters>
        <QueryInput
          value={scheduleSearch}
          onChange={setScheduleSearch}
          placeholder={t('filters.search_schedules')}
          className="w-full sm:max-w-[360px]"
        />
        <FilterSelect
          ariaLabel={t('table.status')}
          value={scheduleStatus}
          onChange={setScheduleStatus}
          options={[
            { value: 'all', label: t('filters.all_statuses') },
            { value: 'enabled', label: t('filters.enabled') },
            { value: 'paused', label: t('filters.paused') },
          ]}
        />
        <FilterSelect
          ariaLabel={t('source.dashboard')}
          value={scheduleSource}
          onChange={setScheduleSource}
          options={[
            { value: 'all', label: t('filters.all_sources') },
            { value: 'dashboard', label: t('filters.dashboard') },
            { value: 'saved_view', label: t('filters.saved_view') },
          ]}
        />
      </ReportFilters>
    ) : tab === 'history' ? (
      <ReportFilters>
        <QueryInput
          value={historySearch}
          onChange={setHistorySearch}
          placeholder={t('filters.search_history')}
          className="w-full sm:max-w-[360px]"
        />
        <FilterSelect
          ariaLabel={t('history.columns.result')}
          value={historyResult}
          onChange={setHistoryResult}
          options={[
            { value: 'all', label: t('filters.all_results') },
            { value: 'sent', label: t('filters.sent') },
            { value: 'failed', label: t('filters.failed') },
            { value: 'pending', label: t('filters.pending') },
          ]}
        />
      </ReportFilters>
    ) : tab === 'templates' ? (
      <div className="flex w-full justify-end">
        <ChromeButton
          variant="primary"
          disabled={createTemplateAccess.disabled}
          disabledReason={createTemplateAccess.reason}
          onClick={() => setTemplateEditor('new')}
        >
          <Plus className="h-4 w-4" />
          {t('templates.new_template')}
        </ChromeButton>
      </div>
    ) : undefined;

  return (
    <>
      <ListPage
        title={t('title')}
        subtitle={t('subtitle')}
        toolbar={
          <>
            <ChromeButton
              onClick={() => {
                if (reports.length) {
                  setExportReportId(null);
                  setExportOpen(true);
                } else {
                  setWorkbenchSeed({ report: null, template: null });
                }
              }}
              disabled={listQuery.isLoading}
            >
              <Download className="h-4 w-4" />
              {t('actions.instant_export')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={scheduleAccess.disabled}
              disabledReason={scheduleAccess.reason}
              onClick={() => setWorkbenchSeed({ report: null, template: null })}
            >
              <Plus className="h-4 w-4" />
              {t('actions.new_report')}
            </ChromeButton>
          </>
        }
        kpis={
          reports.length
            ? [
                {
                  label: t('kpis.running'),
                  value: String(reports.filter((report) => report.enabled).length),
                  tone: 'good',
                },
                { label: t('kpis.next_24h'), value: String(deliveriesNextDay) },
                {
                  label: t('kpis.success_rate'),
                  value: successRate,
                  ...(sentLastThirtyDays > 0 ? { tone: 'good' as const } : {}),
                },
                {
                  label: t('kpis.failed'),
                  value: String(failedCount),
                  tone: failedCount > 0 ? 'danger' : 'good',
                },
              ]
            : undefined
        }
        filters={
          <div className="flex w-full items-center gap-1 overflow-x-auto">
            {(['schedules', 'history', 'templates'] as const).map((candidate) => (
              <button
                key={candidate}
                type="button"
                onClick={() => setTab(candidate)}
                className={cn(
                  'relative h-9 shrink-0 border-b-2 px-3 font-sans text-sm font-strong transition-colors',
                  tab === candidate
                    ? 'border-indigo text-tx-0'
                    : 'border-transparent text-tx-2 hover:text-tx-0',
                )}
              >
                {t(`tabs.${candidate}`)}
                {candidate !== 'templates' && (
                  <span className="ml-2 rounded-full bg-bg-3 px-1.5 py-0.5 text-xs text-tx-2">
                    {candidate === 'schedules' ? reports.length : deliveryRows.length}
                  </span>
                )}
              </button>
            ))}
          </div>
        }
        actionBar={actionBar}
        state={pageState}
        bodyClassName="space-y-4"
      >
        {tab === 'schedules' &&
          (reports.length === 0 ? (
            <ReportStarter
              templates={templates.slice(0, 3)}
              onUseTemplate={(template) =>
                setWorkbenchSeed({ report: null, template })
              }
              onCustom={() => setWorkbenchSeed({ report: null, template: null })}
            />
          ) : filteredReports.length === 0 ? (
            <ProductState
              variant="empty"
              title={t('empty.no_results_title')}
              description={t('empty.no_results_description')}
            />
          ) : (
            <ScheduleTable
              reports={filteredReports}
              dashboardNames={dashboardNames}
              savedViewNames={savedViewNames}
              latestDeliveryByReport={latestDeliveryByReport}
              exportingId={exportingId}
              sharingId={null}
              toggling={toggleMutation.isPending}
              onExport={(report) => void downloadReport(report)}
              onEdit={(report) => setWorkbenchSeed({ report, template: null })}
              onShare={setSharingReport}
              onToggle={(report, enabled) => toggleMutation.mutate({ report, enabled })}
            />
          ))}

        {tab === 'history' &&
          (anyHistoryLoading && deliveryRows.length === 0 ? (
            <QueryState state="loading" />
          ) : reports.length === 0 || deliveryRows.length === 0 ? (
            <ProductState
              variant="empty"
              title={t('history.empty_title')}
              description={t('history.empty_description')}
            />
          ) : (
            <div className="space-y-3">
              {anyHistoryError && (
                <div
                  role="status"
                  className="flex items-center gap-2 rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 text-sm text-yellow-soft"
                >
                  <CircleAlert className="h-4 w-4" />
                  {t('history.partial_error')}
                </div>
              )}
              {filteredHistory.length === 0 ? (
                <ProductState
                  variant="empty"
                  title={t('empty.no_results_title')}
                  description={t('empty.no_results_description')}
                />
              ) : (
                <HistoryTable
                  rows={filteredHistory}
                  onViewError={setFailedDelivery}
                />
              )}
            </div>
          ))}

        {tab === 'templates' && (
          <TemplateLibrary
            templates={templates}
            apiError={templatesQuery.isError}
            onUse={(template) => setWorkbenchSeed({ report: null, template })}
            onEdit={setTemplateEditor}
          />
        )}
      </ListPage>

      <ReportWorkbench
        open={workbenchSeed !== null}
        seed={workbenchSeed}
        dashboards={dashboardsQuery.data ?? []}
        savedViews={savedViewsQuery.data ?? []}
        onClose={() => setWorkbenchSeed(null)}
        onDownload={downloadReport}
      />
      <QuickExportDialog
        open={exportOpen}
        reports={reports}
        initialReportId={exportReportId}
        exportingId={exportingId}
        onOpenChange={setExportOpen}
        onDownload={downloadReport}
      />
      <DeliveryErrorDialog
        row={failedDelivery}
        onClose={() => setFailedDelivery(null)}
      />
      <TemplateEditorDialog
        open={templateEditor !== null}
        template={templateEditor === 'new' ? null : templateEditor}
        onClose={() => setTemplateEditor(null)}
      />
      {sharingReport && (
        <ResourceShareDialog
          open
          onOpenChange={(open) => {
            if (!open) setSharingReport(null);
          }}
          resourceType="report"
          resourceId={sharingReport.id}
          title={sharingReport.title}
          reportFormat={sharingReport.format}
        />
      )}
    </>
  );
}

function ReportFilters({ children }: { children: React.ReactNode }) {
  return <div className="flex w-full flex-wrap items-center gap-2">{children}</div>;
}

function FilterSelect({
  ariaLabel,
  value,
  onChange,
  options,
}: {
  ariaLabel: string;
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      className="h-9 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

function FieldGroup({
  label,
  hint,
  required,
  children,
}: {
  label: string;
  hint?: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <fieldset className="m-0 min-w-0 border-0 p-0">
      <legend className={cn('mb-1.5 flex items-center gap-1', uiLabelClass)}>
        {label}
        {required && (
          <span className="text-red" aria-hidden>
            *
          </span>
        )}
      </legend>
      {children}
      {hint && <div className="mt-1.5 text-xs leading-relaxed text-tx-3">{hint}</div>}
    </fieldset>
  );
}

function ReportStarter({
  templates,
  onUseTemplate,
  onCustom,
}: {
  templates: TemplatePreset[];
  onUseTemplate: (template: TemplatePreset) => void;
  onCustom: () => void;
}) {
  const { t } = useTranslation('reports');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  return (
    <Card className="overflow-hidden">
      <div className="border-b border-bd-0 px-6 py-7 text-center">
        <div className="mx-auto grid h-12 w-12 place-items-center rounded-xl border border-indigo/25 bg-indigo-dim text-indigo-soft">
          <LayoutTemplate className="h-6 w-6" />
        </div>
        <h2 className="mb-0 mt-4 font-sans text-xl font-display-strong tracking-tight text-tx-0">
          {t('empty.title')}
        </h2>
        <p className="mx-auto mb-0 mt-2 max-w-2xl text-sm leading-relaxed text-tx-2">
          {t('empty.description')}
        </p>
      </div>
      <div className="grid gap-3 p-4 md:grid-cols-2 xl:grid-cols-4">
        {templates.map((template) => (
          <StarterCard
            key={template.id}
            template={template}
            onClick={() => onUseTemplate(template)}
          />
        ))}
        <DisabledControl
          disabled={scheduleAccess.disabled}
          reason={scheduleAccess.reason}
          className="w-full"
        >
          <button
            type="button"
            disabled={scheduleAccess.disabled}
            aria-disabled={scheduleAccess.disabled || undefined}
            onClick={onCustom}
            className="group min-h-[184px] w-full rounded-lg border border-dashed border-bd-1 bg-bg-1 p-5 text-left transition-colors enabled:hover:border-indigo/50 enabled:hover:bg-bg-2 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          >
            <span className="grid h-9 w-9 place-items-center rounded-lg border border-bd-0 bg-bg-2 text-tx-2">
              <Plus className="h-4 w-4" />
            </span>
            <span className="mt-5 block font-sans text-sm font-bold text-tx-0">
              {t('empty.custom_title')}
            </span>
            <span className="mt-2 block text-xs leading-relaxed text-tx-2">
              {t('empty.custom_description')}
            </span>
          </button>
        </DisabledControl>
      </div>
    </Card>
  );
}

function StarterCard({
  template,
  onClick,
}: {
  template: TemplatePreset;
  onClick: () => void;
}) {
  const { t } = useTranslation('reports');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const Icon = templateIcon(template.icon);
  return (
    <DisabledControl
      disabled={scheduleAccess.disabled}
      reason={scheduleAccess.reason}
      className="w-full"
    >
      <button
        type="button"
        disabled={scheduleAccess.disabled}
        aria-disabled={scheduleAccess.disabled || undefined}
        onClick={onClick}
        className="group min-h-[184px] w-full rounded-lg border border-bd-0 bg-bg-2 p-5 text-left transition-colors enabled:hover:border-indigo/40 enabled:hover:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
      >
        <span className="grid h-9 w-9 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
          <Icon className="h-4 w-4" />
        </span>
        <span className="mt-5 block font-sans text-sm font-bold text-tx-0">
          {template.name}
        </span>
        <span className="mt-2 line-clamp-2 block text-xs leading-relaxed text-tx-2">
          {template.description}
        </span>
        <span className="mt-4 inline-flex items-center gap-1 text-xs font-bold text-indigo-soft">
          {t('actions.use_template')}
          <span aria-hidden>→</span>
        </span>
      </button>
    </DisabledControl>
  );
}

function TemplateLibrary({
  templates,
  apiError,
  onUse,
  onEdit,
}: {
  templates: TemplatePreset[];
  apiError: boolean;
  onUse: (template: TemplatePreset) => void;
  onEdit: (template: TemplatePreset) => void;
}) {
  const { t } = useTranslation('reports');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const editAccess = useActionAccess({ permission: 'reports.edit' });
  return (
    <div className="space-y-3">
      {apiError && (
        <div
          role="status"
          className="flex items-center gap-2 rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 text-sm text-yellow-soft"
        >
          <CircleAlert className="h-4 w-4" />
          {t('templates.api_warning')}
        </div>
      )}
      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {templates.map((template) => {
          const Icon = templateIcon(template.icon);
          return (
            <Card key={template.id} className="flex min-h-[230px] flex-col p-5">
              <div className="flex items-start justify-between gap-3">
                <div className="grid h-10 w-10 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
                  <Icon className="h-5 w-5" />
                </div>
                <div className="flex flex-wrap justify-end gap-1.5">
                  <Pill tone={template.isBuiltin ? 'dim' : 'indigo'}>
                    {template.isBuiltin
                      ? t('templates.builtin_badge')
                      : t('templates.custom_badge')}
                  </Pill>
                  <Pill>
                    {t('templates.source_badge', {
                      source: t(`source.${template.sourceKind}`),
                    })}
                  </Pill>
                </div>
              </div>
              <h3 className="mb-0 mt-5 font-sans text-base font-bold text-tx-0">
                {template.name}
              </h3>
              <p className="mb-0 mt-2 flex-1 text-sm leading-relaxed text-tx-2">
                {template.description}
              </p>
              <div className="mt-5 flex items-center gap-2 border-t border-bd-0 pt-4">
                <Pill tone="dim">{template.format.toUpperCase()}</Pill>
                <Pill tone="dim">{rangeLabel(template.rangePreset, t)}</Pill>
                {!template.isBuiltin && (
                  <ChromeButton
                    size="sm"
                    className="ml-auto"
                    disabled={editAccess.disabled}
                    disabledReason={editAccess.reason}
                    onClick={() => onEdit(template)}
                  >
                    {t('actions.edit')}
                  </ChromeButton>
                )}
                <ChromeButton
                  size="sm"
                  className={cn(template.isBuiltin && 'ml-auto')}
                  disabled={scheduleAccess.disabled}
                  disabledReason={scheduleAccess.reason}
                  onClick={() => onUse(template)}
                >
                  {t('actions.use_template')}
                </ChromeButton>
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
}

function TemplateEditorDialog({
  open,
  template,
  onClose,
}: {
  open: boolean;
  template: TemplatePreset | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const createAccess = useActionAccess({ permission: 'reports.create' });
  const editAccess = useActionAccess({ permission: 'reports.edit' });
  const deleteAccess = useActionAccess({ permission: 'reports.delete' });
  const saveAccess = template ? editAccess : createAccess;
  const queryClient = useQueryClient();
  const [draft, setDraft] = React.useState<TemplateDraft>(() =>
    templateDraftFromPreset(template),
  );
  const availableFormats = FORMAT_OPTIONS.filter(
    (option) =>
      option.value !== 'png' &&
      (draft.sourceKind === 'saved_view' || option.value === 'pdf'),
  );

  React.useEffect(() => {
    if (open) setDraft(templateDraftFromPreset(template));
  }, [open, template]);

  const updateDraft = <Key extends keyof TemplateDraft>(
    key: Key,
    value: TemplateDraft[Key],
  ) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };
  const selectSourceKind = (sourceKind: SourceKind) => {
    setDraft((current) => ({
      ...current,
      sourceKind,
      format:
        sourceKind === 'dashboard' || current.format === 'png'
          ? 'pdf'
          : current.format,
    }));
  };

  const saveMutation = useMutation({
    mutationFn: () => {
      const input: reportTemplatesApi.ReportTemplateInput = {
        name: draft.name.trim(),
        description: draft.description.trim(),
        target_type: draft.sourceKind,
        format: draft.format,
        time_range_preset: draft.rangePreset,
      };
      return template?.serverId
        ? reportTemplatesApi.update(template.serverId, input)
        : reportTemplatesApi.create(input);
    },
    onSuccess: () => {
      toast.success(
        template ? t('templates.toast_updated') : t('templates.toast_created'),
      );
      void queryClient.invalidateQueries({ queryKey: ['reports', 'templates'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const deleteMutation = useMutation({
    mutationFn: () => reportTemplatesApi.remove(template!.serverId!),
    onSuccess: () => {
      toast.success(t('templates.toast_deleted'));
      void queryClient.invalidateQueries({ queryKey: ['reports', 'templates'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = !draft.name.trim();
  const formDisabled =
    saveAccess.disabled ||
    saveMutation.isPending ||
    deleteMutation.isPending;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!saveAccess.allowed || saveMutation.isPending) return;
    if (invalid) {
      toast.error(t('templates.errors.name_required'));
      return;
    }
    saveMutation.mutate();
  };

  const removeTemplate = () => {
    if (
      !deleteAccess.allowed ||
      !template?.serverId ||
      !window.confirm(t('templates.confirm_delete', { name: template.name }))
    ) {
      return;
    }
    deleteMutation.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent className="flex max-h-[min(820px,calc(100vh-16px))] w-[min(720px,calc(100vw-16px))] max-w-none flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="border-b border-bd-0 px-6 py-5">
          <DialogTitle>
            {template ? t('templates.edit_title') : t('templates.new_title')}
          </DialogTitle>
          <DialogDescription className="mt-1">
            {t('templates.editor_description')}
          </DialogDescription>
        </DialogHeader>

        <form
          id="report-template-form"
          onSubmit={submit}
          className="min-h-0 flex-1 overflow-y-auto px-6 py-5"
        >
          <fieldset
            disabled={formDisabled}
            aria-disabled={formDisabled || undefined}
            className="m-0 min-w-0 space-y-6 border-0 p-0"
          >
          {saveAccess.disabled && (
            <div
              role="status"
              className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2 text-xs text-tx-2"
            >
              {saveAccess.reason}
            </div>
          )}
          <section
            aria-labelledby="report-template-basic-information"
            className="space-y-4"
          >
            <h3
              id="report-template-basic-information"
              className={uiLabelStrongClass}
            >
              {t('templates.basic_information')}
            </h3>
            <FormField label={t('templates.fields.name')} required>
              <FormInput
                value={draft.name}
                onChange={(event) => updateDraft('name', event.target.value)}
                placeholder={t('templates.placeholders.name')}
                required
              />
            </FormField>
            <FormField label={t('templates.fields.description_optional')}>
              <FormTextarea
                value={draft.description}
                onChange={(event) => updateDraft('description', event.target.value)}
                placeholder={t('templates.placeholders.description')}
                rows={3}
              />
            </FormField>
          </section>

          <FieldGroup label={t('templates.fields.source_type')}>
            <p className="mb-2 mt-0 text-xs leading-relaxed text-tx-3">
              {t('templates.source_hint')}
            </p>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {(['dashboard', 'saved_view'] as const).map((kind) => (
                <button
                  key={kind}
                  type="button"
                  aria-pressed={draft.sourceKind === kind}
                  onClick={() => selectSourceKind(kind)}
                  className={cn(
                    'relative min-h-[92px] rounded-lg border p-4 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                    draft.sourceKind === kind
                      ? 'border-indigo bg-indigo-dim'
                      : 'border-bd-1 bg-bg-2 enabled:hover:border-bd-2 enabled:hover:bg-bg-3',
                  )}
                >
                  <span className="flex items-start gap-3">
                    <span
                      className={cn(
                        'grid h-9 w-9 shrink-0 place-items-center rounded-md border',
                        draft.sourceKind === kind
                          ? 'border-indigo/30 bg-bg-1 text-indigo-soft'
                          : 'border-bd-0 bg-bg-1 text-tx-2',
                      )}
                    >
                      {kind === 'dashboard' ? (
                        <LayoutTemplate className="h-4 w-4" />
                      ) : (
                        <Search className="h-4 w-4" />
                      )}
                    </span>
                    <span className="min-w-0 pr-6">
                      <span className="block font-sans text-sm font-bold text-tx-0">
                        {t(`source.${kind}`)}
                      </span>
                      <span className="mt-1 block text-xs leading-relaxed text-tx-2">
                        {t(`templates.source_descriptions.${kind}`)}
                      </span>
                    </span>
                  </span>
                  {draft.sourceKind === kind && (
                    <span className="absolute right-3 top-3 grid h-5 w-5 place-items-center rounded-full bg-indigo text-white">
                      <Check className="h-3 w-3" strokeWidth={2.5} />
                    </span>
                  )}
                </button>
              ))}
            </div>
            <div className="mt-2 flex items-start gap-2 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5 text-xs leading-relaxed text-tx-2">
              <Link2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-tx-3" />
              <span>{t('templates.source_binding_hint')}</span>
            </div>
          </FieldGroup>

          <section
            aria-labelledby="report-template-default-output"
            className="space-y-4"
          >
            <h3 id="report-template-default-output" className={uiLabelStrongClass}>
              {t('templates.default_output')}
            </h3>
            <FieldGroup label={t('templates.fields.format')}>
              <div
                className={cn(
                  'grid grid-cols-1 gap-2',
                  draft.sourceKind === 'saved_view' && 'sm:grid-cols-3',
                )}
              >
                {availableFormats.map((option) => {
                  const Icon = option.icon;
                  const selected = draft.format === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      aria-pressed={selected}
                      onClick={() => updateDraft('format', option.value)}
                      className={cn(
                        'relative min-h-[76px] rounded-lg border p-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                        selected
                          ? 'border-indigo bg-indigo-dim'
                          : 'border-bd-1 bg-bg-2 enabled:hover:border-bd-2 enabled:hover:bg-bg-3',
                      )}
                    >
                      <span className="flex items-center gap-2">
                        <Icon
                          className={cn(
                            'h-4 w-4',
                            selected ? 'text-indigo-soft' : 'text-tx-3',
                          )}
                        />
                        <span className="font-sans text-sm font-bold text-tx-0">
                          {option.value.toUpperCase()}
                        </span>
                      </span>
                      <span className="mt-2 block text-xs leading-relaxed text-tx-2">
                        {t(`templates.format_descriptions.${option.value}`)}
                      </span>
                      {selected && (
                        <Check className="absolute right-3 top-3 h-3.5 w-3.5 text-indigo-soft" />
                      )}
                    </button>
                  );
                })}
              </div>
              {draft.sourceKind === 'dashboard' && (
                <p className="mb-0 mt-2 text-xs text-tx-3">
                  {t('templates.dashboard_format_hint')}
                </p>
              )}
            </FieldGroup>
            <FormField label={t('templates.fields.time_range')}>
              <FormSelect
                value={draft.rangePreset}
                onChange={(value) => updateDraft('rangePreset', value)}
                options={RANGE_OPTIONS.map((option) => ({
                  value: option.value,
                  label: t(option.labelKey),
                }))}
              />
            </FormField>
            {draft.rangePreset === 'custom' && (
              <p className="mb-0 text-xs leading-relaxed text-tx-3">
                {t('templates.custom_range_hint')}
              </p>
            )}
          </section>
          </fieldset>
        </form>

        <DialogFooter className="flex items-center justify-between space-x-0 border-t border-bd-0 bg-bg-2 px-6 py-4">
          <div>
            {template?.serverId && (
              <ChromeButton
                onClick={removeTemplate}
                disabled={deleteMutation.isPending || deleteAccess.disabled}
                disabledReason={
                  !deleteMutation.isPending ? deleteAccess.reason : undefined
                }
                className="text-red-soft hover:text-red-soft"
              >
                {deleteMutation.isPending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Trash2 className="h-4 w-4" />
                )}
                {t('templates.delete_template')}
              </ChromeButton>
            )}
          </div>
          <div className="flex items-center gap-2">
            <ChromeButton onClick={onClose}>{t('actions.cancel')}</ChromeButton>
            <ChromeButton
              variant="primary"
              type="submit"
              form="report-template-form"
              disabled={
                saveMutation.isPending ||
                saveAccess.disabled ||
                invalid
              }
              disabledReason={
                !saveMutation.isPending
                  ? saveAccess.reason ??
                    (invalid ? tc('access.form_invalid') : undefined)
                  : undefined
              }
            >
              {saveMutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              {template ? t('templates.save_changes') : t('templates.create_template')}
            </ChromeButton>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ReportWorkbench({
  open,
  seed,
  dashboards,
  savedViews,
  onClose,
  onDownload,
}: {
  open: boolean;
  seed: WorkbenchSeed | null;
  dashboards: Awaited<ReturnType<typeof dashboardsApi.list>>;
  savedViews: Awaited<ReturnType<typeof savedViewsApi.list>>;
  onClose: () => void;
  onDownload: (report: DisplayReport) => Promise<void>;
}) {
  const { t } = useTranslation('reports');
  const { t: tc } = useTranslation('common');
  const scheduleAccess = useActionAccess({ permission: 'reports.schedule' });
  const deleteAccess = useActionAccess({ permission: 'reports.delete' });
  const queryClient = useQueryClient();
  const [step, setStep] = React.useState<WorkbenchStep>('content');
  const [draft, setDraft] = React.useState<ReportDraft>(() => draftFromSeed(seed));
  const [recipientInput, setRecipientInput] = React.useState('');
  const [recipientError, setRecipientError] = React.useState<string | null>(null);
  const report = seed?.report ?? null;
  const isEdit = report !== null;
  const customCron = !SCHEDULE_OPTIONS.some((option) => option.value === draft.cron);

  React.useEffect(() => {
    if (!open) return;
    setStep('content');
    setDraft(draftFromSeed(seed));
    setRecipientInput('');
    setRecipientError(null);
  }, [open, seed]);

  const sourceOptions =
    draft.sourceKind === 'dashboard'
      ? dashboards.map((dashboard) => ({ value: dashboard.id, label: dashboard.title }))
      : savedViews.map((view) => ({ value: view.id, label: view.name }));
  const sourceTitle =
    sourceOptions.find((option) => option.value === draft.sourceId)?.label ??
    t('source.missing');

  const saveMutation = useMutation({
    mutationFn: () => {
      const input = reportInputFromDraft(draft);
      return report
        ? reportsApi.update(report.id, input)
        : reportsApi.create(input);
    },
    onSuccess: () => {
      toast.success(isEdit ? t('toast.updated') : t('toast.created'));
      void queryClient.invalidateQueries({ queryKey: ['reports', 'list'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const deleteMutation = useMutation({
    mutationFn: () => reportsApi.remove(report!.id),
    onSuccess: () => {
      toast.success(t('toast.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['reports', 'list'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = validateDraft(draft) !== null;
  const formDisabled =
    scheduleAccess.disabled ||
    saveMutation.isPending ||
    deleteMutation.isPending;

  const updateDraft = <Key extends keyof ReportDraft>(
    key: Key,
    value: ReportDraft[Key],
  ) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const addRecipient = () => {
    const result = parseRecipient(recipientInput);
    if (!result.recipient) {
      setRecipientError(t(`errors.recipient_${result.error}`));
      return;
    }
    if (
      draft.recipients.some(
        (recipient) =>
          recipient.kind === result.recipient?.kind &&
          recipient.target === result.recipient?.target,
      )
    ) {
      toast.info(t('toast.recipient_exists'));
      return;
    }
    updateDraft('recipients', [...draft.recipients, result.recipient]);
    setRecipientInput('');
    setRecipientError(null);
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!scheduleAccess.allowed || saveMutation.isPending) return;
    const validationError = validateDraft(draft);
    if (validationError) {
      toast.error(t(validationError));
      setStep(
        validationError === 'errors.name_required' || validationError === 'errors.source_required'
          ? 'content'
          : 'delivery',
      );
      return;
    }
    saveMutation.mutate();
  };

  const removeReport = () => {
    if (
      !deleteAccess.allowed ||
      !report ||
      !window.confirm(t('confirm.delete', { name: report.title }))
    ) {
      return;
    }
    deleteMutation.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent
        hideClose
        className="flex h-[min(900px,calc(100vh-24px))] w-[min(1480px,calc(100vw-24px))] max-w-none flex-col gap-0 overflow-hidden p-0"
      >
        <div className="flex items-start gap-4 border-b border-bd-0 px-6 py-5">
          <DialogHeader className="min-w-0 flex-1">
            <DialogTitle className="font-sans text-xl font-display-strong tracking-tight text-tx-0">
              {isEdit ? t('workbench.edit_title') : t('workbench.new_title')}
            </DialogTitle>
            <DialogDescription className="mt-1 text-sm text-tx-2">
              {t('workbench.subtitle')}
            </DialogDescription>
          </DialogHeader>
          <div className="flex items-center gap-2">
            {report && (
              <ChromeButton
                onClick={() => void onDownload(report)}
                disabled={saveMutation.isPending}
              >
                <Download className="h-4 w-4" />
                {t('actions.export')}
              </ChromeButton>
            )}
            <button
              type="button"
              onClick={onClose}
              aria-label={t('actions.close')}
              className="grid h-9 w-9 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        <div className="grid min-h-0 flex-1 lg:grid-cols-[minmax(500px,0.9fr)_minmax(440px,1.1fr)]">
          <form
            id="report-workbench-form"
            onSubmit={submit}
            className="flex min-h-0 flex-col bg-bg-1"
          >
            <div className="grid grid-cols-3 border-b border-bd-0 px-6">
              {(['content', 'schedule', 'delivery'] as const).map((candidate, index) => (
                <button
                  key={candidate}
                  type="button"
                  onClick={() => setStep(candidate)}
                  className={cn(
                    'relative flex min-h-[58px] items-center gap-2 border-b-2 px-2 text-left font-sans text-xs font-strong',
                    step === candidate
                      ? 'border-indigo text-tx-0'
                      : 'border-transparent text-tx-2 hover:text-tx-0',
                  )}
                >
                  <span
                    className={cn(
                      'grid h-5 w-5 shrink-0 place-items-center rounded-full border text-xs',
                      step === candidate
                        ? 'border-indigo bg-indigo text-white'
                        : 'border-bd-1 bg-bg-2 text-tx-2',
                    )}
                  >
                    {index + 1}
                  </span>
                  <span className="truncate">{t(`workbench.steps.${candidate}`)}</span>
                </button>
              ))}
            </div>

            <fieldset
              disabled={formDisabled}
              aria-disabled={formDisabled || undefined}
              className="m-0 min-h-0 flex-1 overflow-y-auto border-0 px-6 py-6"
            >
              {scheduleAccess.disabled && (
                <div
                  role="status"
                  className="mb-5 rounded-md border border-bd-0 bg-bg-2 px-3 py-2 text-xs text-tx-2"
                >
                  {scheduleAccess.reason}
                </div>
              )}
              <div className="mb-6">
                <div className="font-sans text-lg font-bold text-tx-0">
                  {t(`workbench.steps.${step}`)}
                </div>
                <p className="mb-0 mt-1 text-sm leading-relaxed text-tx-2">
                  {t(`workbench.step_descriptions.${step}`)}
                </p>
              </div>

              {step === 'content' && (
                <div className="space-y-5">
                  <FormField label={t('workbench.fields.name')} required>
                    <FormInput
                      value={draft.title}
                      onChange={(event) => updateDraft('title', event.target.value)}
                      placeholder={t('workbench.placeholders.name')}
                      required
                    />
                  </FormField>
                  <FormField label={t('workbench.fields.description')}>
                    <FormTextarea
                      value={draft.description}
                      onChange={(event) => updateDraft('description', event.target.value)}
                      placeholder={t('workbench.placeholders.description')}
                      rows={3}
                    />
                  </FormField>
                  <FieldGroup label={t('workbench.fields.source_type')} required>
                    <div className="grid grid-cols-2 gap-2">
                      {(['dashboard', 'saved_view'] as const).map((kind) => (
                        <button
                          key={kind}
                          type="button"
                          aria-pressed={draft.sourceKind === kind}
                          onClick={() => {
                            updateDraft('sourceKind', kind);
                            updateDraft('sourceId', '');
                          }}
                          className={cn(
                            'h-10 rounded-md border px-3 text-left font-sans text-sm font-strong transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                            draft.sourceKind === kind
                              ? 'border-indigo bg-indigo-dim text-indigo-soft'
                              : 'border-bd-1 bg-bg-2 text-tx-1 enabled:hover:bg-bg-3',
                          )}
                        >
                          {t(`source.${kind}`)}
                        </button>
                      ))}
                    </div>
                  </FieldGroup>
                  <FormField
                    label={t('workbench.fields.source')}
                    {...(sourceOptions.length === 0
                      ? { hint: t('workbench.hints.source_empty') }
                      : {})}
                    required
                  >
                    <FormSelect
                      value={draft.sourceId}
                      onChange={(value) => updateDraft('sourceId', value)}
                      placeholder={t('workbench.placeholders.source')}
                      options={sourceOptions}
                    />
                  </FormField>
                  <FormField label={t('workbench.fields.time_range')}>
                    <FormSelect
                      value={draft.rangePreset}
                      onChange={(value) => updateDraft('rangePreset', value)}
                      options={RANGE_OPTIONS.map((option) => ({
                        value: option.value,
                        label: t(option.labelKey),
                      }))}
                    />
                  </FormField>
                </div>
              )}

              {step === 'schedule' && (
                <div className="space-y-5">
                  <FieldGroup label={t('workbench.fields.frequency')}>
                    <div className="grid grid-cols-2 gap-2">
                      {SCHEDULE_OPTIONS.map((option) => (
                        <button
                          key={option.value}
                          type="button"
                          aria-pressed={draft.cron === option.value}
                          onClick={() => updateDraft('cron', option.value)}
                          className={cn(
                            'flex h-11 items-center gap-2 rounded-md border px-3 text-left font-sans text-sm font-strong transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                            draft.cron === option.value
                              ? 'border-indigo bg-indigo-dim text-indigo-soft'
                              : 'border-bd-1 bg-bg-2 text-tx-1 enabled:hover:bg-bg-3',
                          )}
                        >
                          <Clock3 className="h-4 w-4" />
                          {t(option.labelKey)}
                        </button>
                      ))}
                      <button
                        type="button"
                        aria-pressed={customCron}
                        onClick={() => updateDraft('cron', '0 9 * * 1')}
                        className={cn(
                          'flex h-11 items-center gap-2 rounded-md border px-3 text-left font-sans text-sm font-strong transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                          customCron
                            ? 'border-indigo bg-indigo-dim text-indigo-soft'
                            : 'border-bd-1 bg-bg-2 text-tx-1 enabled:hover:bg-bg-3',
                        )}
                      >
                        <CircleAlert className="h-4 w-4" />
                        {t('schedule.custom')}
                      </button>
                    </div>
                  </FieldGroup>
                  {customCron && (
                    <FormField
                      label={t('workbench.fields.cron')}
                      hint={t('schedule.advanced_hint')}
                    >
                      <FormInput
                        value={draft.cron}
                        onChange={(event) => updateDraft('cron', event.target.value)}
                        placeholder={t('workbench.placeholders.cron')}
                      />
                    </FormField>
                  )}
                  <FormField label={t('workbench.fields.timezone')}>
                    <FormSelect
                      value={draft.timezone}
                      onChange={(value) => updateDraft('timezone', value)}
                      options={[
                        'Asia/Shanghai',
                        'UTC',
                        'Asia/Tokyo',
                        'Europe/London',
                        'America/New_York',
                      ]}
                    />
                  </FormField>
                  <div className="rounded-lg border border-indigo/20 bg-indigo-dim px-4 py-3">
                    <div className={uiLabelStrongClass}>
                      {t('workbench.fields.frequency')}
                    </div>
                    <div className="mt-1.5 text-sm text-indigo-soft">
                      {t('schedule.timezone_suffix', {
                        schedule: scheduleLabel(draft.cron, t),
                        timezone: draft.timezone,
                      })}
                    </div>
                  </div>
                </div>
              )}

              {step === 'delivery' && (
                <div className="space-y-6">
                  <FieldGroup
                    label={t('workbench.fields.recipients')}
                    hint={t('workbench.hints.recipients')}
                    required
                  >
                    <div className="flex gap-2">
                      <FormInput
                        value={recipientInput}
                        onChange={(event) => {
                          setRecipientInput(event.target.value);
                          setRecipientError(null);
                        }}
                        onKeyDown={(event) => {
                          if (event.key !== 'Enter') return;
                          event.preventDefault();
                          addRecipient();
                        }}
                        aria-label={t('workbench.fields.recipients')}
                        placeholder={t('workbench.placeholders.recipient')}
                      />
                      <ChromeButton onClick={addRecipient}>
                        <Plus className="h-4 w-4" />
                        {t('actions.add')}
                      </ChromeButton>
                    </div>
                    {recipientError && (
                      <span className="text-xs text-red-soft">{recipientError}</span>
                    )}
                  </FieldGroup>

                  {draft.recipients.length > 0 && (
                    <div className="space-y-2">
                      {draft.recipients.map((recipient) => (
                        <div
                          key={`${recipient.kind}:${recipient.target}`}
                          className="flex items-center gap-3 rounded-lg border border-bd-0 bg-bg-2 px-3 py-2.5"
                        >
                          <RecipientIcon kind={recipient.kind} />
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-strong text-tx-0">
                              {recipient.target}
                            </div>
                            <div className="mt-0.5 text-xs text-tx-3">
                              {t(`workbench.recipient_kinds.${recipient.kind}`, {
                                defaultValue: recipient.kind,
                              })}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() =>
                              updateDraft(
                                'recipients',
                                draft.recipients.filter(
                                  (item) =>
                                    !(
                                      item.kind === recipient.kind &&
                                      item.target === recipient.target
                                    ),
                                ),
                              )
                            }
                            aria-label={`${t('actions.remove')} ${recipient.target}`}
                            className="grid h-8 w-8 place-items-center rounded-md text-tx-3 enabled:hover:bg-bg-3 enabled:hover:text-red-soft disabled:cursor-not-allowed disabled:opacity-60"
                          >
                            <X className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      ))}
                    </div>
                  )}

                  <FieldGroup label={t('workbench.fields.format')}>
                    <div className="grid grid-cols-2 gap-2">
                      {FORMAT_OPTIONS.map((option) => {
                        const Icon = option.icon;
                        return (
                          <button
                            key={option.value}
                            type="button"
                            aria-pressed={draft.format === option.value}
                            onClick={() => updateDraft('format', option.value)}
                            className={cn(
                              'rounded-lg border p-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                              draft.format === option.value
                                ? 'border-indigo bg-indigo-dim'
                                : 'border-bd-1 bg-bg-2 enabled:hover:bg-bg-3',
                            )}
                          >
                            <div className="flex items-center gap-2">
                              <Icon
                                className={cn(
                                  'h-4 w-4',
                                  draft.format === option.value
                                    ? 'text-indigo-soft'
                                    : 'text-tx-2',
                                )}
                              />
                              <span className="font-sans text-sm font-bold text-tx-0">
                                {option.value.toUpperCase()}
                              </span>
                            </div>
                            <div className="mt-2 text-xs leading-relaxed text-tx-2">
                              {t(option.descriptionKey)}
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  </FieldGroup>

                  <div className="flex items-start justify-between gap-6 rounded-lg border border-bd-0 bg-bg-2 p-4">
                    <div>
                      <div className={uiLabelStrongClass}>
                        {t('workbench.fields.enabled')}
                      </div>
                      <p className="mb-0 mt-1 text-xs leading-relaxed text-tx-3">
                        {t('workbench.hints.enabled')}
                      </p>
                    </div>
                    <Switch
                      checked={draft.enabled}
                      onCheckedChange={(enabled) => updateDraft('enabled', enabled)}
                      aria-label={t('workbench.fields.enabled')}
                    />
                  </div>
                </div>
              )}
            </fieldset>
          </form>

          <LiveReportPreview
            draft={draft}
            sourceTitle={sourceTitle}
            className="hidden lg:flex"
          />
        </div>

        <DialogFooter className="flex items-center justify-between space-x-0 border-t border-bd-0 bg-bg-2 px-6 py-4">
          <div>
            {report && (
              <ChromeButton
                onClick={removeReport}
                disabled={deleteMutation.isPending || deleteAccess.disabled}
                disabledReason={
                  !deleteMutation.isPending ? deleteAccess.reason : undefined
                }
                className="text-red-soft hover:text-red-soft"
              >
                {deleteMutation.isPending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Trash2 className="h-4 w-4" />
                )}
                {t('actions.delete')}
              </ChromeButton>
            )}
          </div>
          <div className="flex items-center gap-2">
            <ChromeButton onClick={onClose}>{t('actions.cancel')}</ChromeButton>
            <ChromeButton
              variant="primary"
              type="submit"
              form="report-workbench-form"
              disabled={
                saveMutation.isPending ||
                scheduleAccess.disabled ||
                invalid
              }
              disabledReason={
                !saveMutation.isPending
                  ? scheduleAccess.reason ??
                    (invalid ? tc('access.form_invalid') : undefined)
                  : undefined
              }
            >
              {saveMutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              {isEdit ? t('actions.save_changes') : t('actions.save_and_enable')}
            </ChromeButton>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function LiveReportPreview({
  draft,
  sourceTitle,
  className,
}: {
  draft: ReportDraft;
  sourceTitle: string;
  className?: string;
}) {
  const { t } = useTranslation('reports');
  return (
    <aside
      className={cn(
        'min-h-0 flex-col border-l border-bd-0 bg-bg-0 px-8 py-6',
        className,
      )}
    >
      <div className="mb-4 flex items-center justify-between">
        <div className={uiLabelStrongClass}>{t('workbench.preview.title')}</div>
        <Pill tone="dim">{t('workbench.preview.sample_badge')}</Pill>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border border-bd-1 bg-bg-1 p-7 shadow-login">
        <div className="flex items-center justify-between border-b border-bd-0 pb-5">
          <div className="flex items-center gap-2">
            <span className="grid h-8 w-8 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
              <BarChart3 className="h-4 w-4" />
            </span>
            <div>
              <div className="font-sans text-sm font-bold text-tx-0">MoleSignal</div>
              <div className="text-xs text-tx-3">{t('workbench.preview.report_label')}</div>
            </div>
          </div>
          <Pill tone="indigo">{draft.format.toUpperCase()}</Pill>
        </div>

        <div className="py-6">
          <h3 className="m-0 font-sans text-2xl font-display-strong tracking-tight text-tx-0">
            {draft.title || t('workbench.placeholders.name')}
          </h3>
          {draft.description && (
            <p className="mb-0 mt-2 max-w-2xl text-sm leading-relaxed text-tx-2">
              {draft.description}
            </p>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            <Pill tone="dim">
              {t('workbench.preview.source_label')} · {sourceTitle}
            </Pill>
            <Pill tone="dim">
              {t('workbench.preview.range_label')} · {rangeLabel(draft.rangePreset, t)}
            </Pill>
          </div>
        </div>

        <div className="grid grid-cols-3 gap-3">
          {[
            [t('workbench.preview.availability'), '99.95%'],
            [t('workbench.preview.latency'), '184 ms'],
            [t('workbench.preview.alerts'), '3'],
          ].map(([label, value]) => (
            <div key={label} className="rounded-lg border border-bd-0 bg-bg-2 p-4">
              <div className={uiLabelClass}>{label}</div>
              <div className="mt-2 font-sans text-xl font-display-strong text-tx-0">
                {value}
              </div>
            </div>
          ))}
        </div>

        <div className="mt-5 rounded-lg border border-bd-0 p-4">
          <div className={uiLabelStrongClass}>{t('workbench.preview.trend')}</div>
          <TimeSeriesChart
            className="mt-5"
            series={[
              {
                id: 'report-preview-trend',
                name: t('workbench.preview.trend'),
                color: 'var(--indigo)',
                data: [28, 42, 38, 62, 54, 78, 72, 88, 68, 92, 82, 96],
              },
            ]}
            height={112}
            ariaLabel={t('workbench.preview.trend')}
            options={{
              drawStyle: 'bar',
              showPoints: 'never',
              tooltipMode: 'hidden',
              legendMode: 'hidden',
              showXAxis: false,
              showYAxis: false,
              leftAxis: { min: 0, max: 100, showGrid: false },
            }}
            showLegend={false}
          />
        </div>

        <div className="mt-6 flex items-center justify-between border-t border-bd-0 pt-4 text-xs text-tx-3">
          <span>
            {t('workbench.preview.footer', {
              format: draft.format.toUpperCase(),
              schedule: scheduleLabel(draft.cron, t),
            })}
          </span>
          <span>{draft.timezone}</span>
        </div>
      </div>
    </aside>
  );
}

function QuickExportDialog({
  open,
  reports,
  initialReportId,
  exportingId,
  onOpenChange,
  onDownload,
}: {
  open: boolean;
  reports: DisplayReport[];
  initialReportId: string | null;
  exportingId: string | null;
  onOpenChange: (open: boolean) => void;
  onDownload: (report: DisplayReport) => Promise<void>;
}) {
  const { t } = useTranslation('reports');
  const [selectedId, setSelectedId] = React.useState('');
  const selected = reports.find((report) => report.id === selectedId) ?? reports[0] ?? null;

  React.useEffect(() => {
    if (!open) return;
    setSelectedId(
      initialReportId &&
        reports.some((report) => report.id === initialReportId)
        ? initialReportId
        : (reports[0]?.id ?? ''),
    );
  }, [initialReportId, open, reports]);

  const download = async () => {
    if (!selected) return;
    try {
      await onDownload(selected);
      onOpenChange(false);
    } catch {
      // The shared downloader already surfaces the API error.
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(520px,calc(100vw-24px))]">
        <DialogHeader>
          <DialogTitle>{t('export_dialog.title')}</DialogTitle>
          <DialogDescription>{t('export_dialog.description')}</DialogDescription>
        </DialogHeader>
        {selected ? (
          <div className="space-y-4 py-2">
            <FormField label={t('export_dialog.report')}>
              <FormSelect
                value={selected.id}
                onChange={setSelectedId}
                options={reports.map((report) => ({
                  value: report.id,
                  label: report.title,
                }))}
              />
            </FormField>
            <div className="grid grid-cols-2 gap-3">
              <div className="rounded-lg border border-bd-0 bg-bg-2 p-3">
                <div className={uiLabelClass}>{t('export_dialog.format')}</div>
                <div className="mt-1.5 font-sans text-sm font-bold uppercase text-tx-0">
                  {selected.format}
                </div>
              </div>
              <div className="rounded-lg border border-bd-0 bg-bg-2 p-3">
                <div className={uiLabelClass}>{t('export_dialog.range')}</div>
                <div className="mt-1.5 font-sans text-sm font-bold text-tx-0">
                  {rangeLabel(selected.rangePreset, t)}
                </div>
              </div>
            </div>
          </div>
        ) : (
          <p className="text-sm text-tx-2">{t('export_dialog.empty')}</p>
        )}
        <DialogFooter>
          <ChromeButton onClick={() => onOpenChange(false)}>
            {t('actions.cancel')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            onClick={() => void download()}
            disabled={!selected || exportingId !== null}
          >
            {exportingId ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            {t('export_dialog.download')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function DeliveryErrorDialog({
  row,
  onClose,
}: {
  row: DeliveryRow | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('reports');
  const attemptedAt = normalizeMicros(row?.delivery.attempted_at);
  return (
    <Dialog open={row !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[min(560px,calc(100vw-24px))]">
        <DialogHeader>
          <DialogTitle>{t('history.error_title')}</DialogTitle>
          <DialogDescription>{t('history.error_description')}</DialogDescription>
        </DialogHeader>
        {row && (
          <dl className="grid grid-cols-[130px_minmax(0,1fr)] gap-x-4 gap-y-3 py-2 text-sm">
            <dt className="text-tx-3">{t('history.error_report')}</dt>
            <dd className="m-0 font-strong text-tx-0">{row.report.title}</dd>
            <dt className="text-tx-3">{t('history.error_recipient')}</dt>
            <dd className="m-0 break-all text-tx-1">
              {row.delivery.recipient_target ?? '—'}
            </dd>
            <dt className="text-tx-3">{t('history.error_time')}</dt>
            <dd className="m-0 tabular-nums text-tx-1">
              {attemptedAt ? formatMicrosActive(attemptedAt) : '—'}
            </dd>
            <dt className="text-tx-3">{t('history.error_message')}</dt>
            <dd className="m-0 rounded-md border border-red/20 bg-red-dim p-3 font-mono text-xs leading-relaxed text-red-soft">
              {row.delivery.error || t('history.no_error_message')}
            </dd>
          </dl>
        )}
        <DialogFooter>
          <ChromeButton onClick={onClose}>{t('actions.close')}</ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
