import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Activity,
  BellRing,
  Check,
  CircleAlert,
  Database,
  FileCode2,
  Gauge,
  Plus,
  Save,
  TestTube2,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as queryApi from '@/api/query';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Dot, Pill, uiLabelClass } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { ErrorState } from '@/shell/ErrorState';
import {
  FormField,
  FormInput,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { LoadingState } from '@/shell/LoadingState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';
import type {
  ComparisonOp,
  Severity,
  SeverityThreshold,
  StreamType,
} from '@/types/alerting';
import type { QueryLanguage } from '@/types/query';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  COMPARISON_LABEL,
  estimateTriggerEpisodes,
  extractQueryPoints,
  ruleSeverity,
  seedThresholds,
  severityRank,
  thresholdConflict,
  topThreshold,
} from './alertRuleModel';

type RuleSignal = StreamType;
type SaveMode = 'draft' | 'active';

interface QueryPreset {
  id: string;
  titleKey: string;
  descriptionKey: string;
  stream: string;
  statement: string;
  operator: ComparisonOp;
  threshold: number;
  severity: Severity;
}

const PRESETS: Record<RuleSignal, QueryPreset[]> = {
  metrics: [
    {
      id: 'error-rate',
      titleKey: 'workbench.presets.error_rate.title',
      descriptionKey: 'workbench.presets.error_rate.description',
      stream: 'http_requests_total',
      statement:
        'sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m]))',
      operator: 'gt',
      threshold: 0.05,
      severity: 'warning',
    },
    {
      id: 'latency',
      titleKey: 'workbench.presets.latency.title',
      descriptionKey: 'workbench.presets.latency.description',
      stream: 'http_request_duration_seconds',
      statement:
        'histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))',
      operator: 'gt',
      threshold: 0.8,
      severity: 'warning',
    },
    {
      id: 'cpu',
      titleKey: 'workbench.presets.cpu.title',
      descriptionKey: 'workbench.presets.cpu.description',
      stream: 'process_cpu_usage',
      statement: 'avg(process_cpu_usage)',
      operator: 'gt',
      threshold: 0.85,
      severity: 'warning',
    },
  ],
  logs: [
    {
      id: 'log-errors',
      titleKey: 'workbench.presets.log_errors.title',
      descriptionKey: 'workbench.presets.log_errors.description',
      stream: 'app_logs',
      statement: "SELECT COUNT(*) AS value FROM app_logs WHERE level = 'error'",
      operator: 'gt',
      threshold: 20,
      severity: 'warning',
    },
    {
      id: 'log-no-data',
      titleKey: 'workbench.presets.no_data.title',
      descriptionKey: 'workbench.presets.no_data.description',
      stream: 'app_logs',
      statement: 'SELECT COUNT(*) AS value FROM app_logs',
      operator: 'eq',
      threshold: 0,
      severity: 'warning',
    },
  ],
  traces: [
    {
      id: 'trace-errors',
      titleKey: 'workbench.presets.trace_errors.title',
      descriptionKey: 'workbench.presets.trace_errors.description',
      stream: 'traces',
      statement: "SELECT COUNT(*) AS value FROM traces WHERE status_code = 'ERROR'",
      operator: 'gt',
      threshold: 10,
      severity: 'warning',
    },
    {
      id: 'trace-latency',
      titleKey: 'workbench.presets.trace_latency.title',
      descriptionKey: 'workbench.presets.trace_latency.description',
      stream: 'traces',
      statement: 'SELECT AVG(duration_ns) / 1000000 AS value FROM traces',
      operator: 'gt',
      threshold: 800,
      severity: 'warning',
    },
  ],
};

const SIGNAL_ICON: Record<RuleSignal, React.ReactNode> = {
  metrics: <Gauge className="h-4 w-4" />,
  logs: <FileCode2 className="h-4 w-4" />,
  traces: <Activity className="h-4 w-4" />,
};

export function AlertRuleWorkbench() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const params = useParams();
  const ruleId = params.id;
  const isEdit = Boolean(ruleId);
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });

  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [service, setService] = React.useState('');
  const [signal, setSignal] = React.useState<RuleSignal>('metrics');
  const [queryLanguage, setQueryLanguage] = React.useState<QueryLanguage>('promql');
  const [streamName, setStreamName] = React.useState('http_requests_total');
  const [query, setQuery] = React.useState(PRESETS.metrics[0]!.statement);
  const [periodSecs, setPeriodSecs] = React.useState(60);
  const [thresholds, setThresholds] = React.useState<SeverityThreshold[]>([
    {
      severity: 'warning',
      operator: 'gt',
      threshold: 0.05,
      for_periods: 5,
    },
  ]);
  const [runbook, setRunbook] = React.useState('');
  const initializedRule = React.useRef<string | null>(null);

  const ruleQuery = useQuery({
    queryKey: ['alerts', 'rule', ruleId],
    queryFn: () => alertsApi.get(ruleId!),
    enabled: Boolean(ruleId),
  });
  React.useEffect(() => {
    const rule = ruleQuery.data;
    if (!rule || initializedRule.current === rule.id) return;
    initializedRule.current = rule.id;
    setName(rule.name);
    setDescription(rule.description);
    setService(rule.labels.service ?? rule.labels.svc ?? '');
    setSignal(rule.query.stream?.stream_type ?? 'metrics');
    setQueryLanguage(rule.query.language);
    setStreamName(rule.query.stream?.name ?? '');
    setQuery(rule.query.statement);
    setPeriodSecs(rule.query.period_secs || 60);
    setThresholds(seedThresholds(rule));
    setRunbook(rule.annotations.runbook_url ?? '');
  }, [ruleQuery.data]);

  const preview = useMutation({
    mutationFn: async () => {
      if (!orgId) throw new Error(t('workbench.errors.org_required'));
      if (!streamName.trim()) throw new Error(t('workbench.errors.stream_required'));
      if (!query.trim()) throw new Error(t('workbench.errors.query_required'));
      const to = Date.now() * 1000;
      const from = to - 6 * 60 * 60 * 1_000_000;
      const result = await queryApi.runQuery({
        org_id: orgId,
        language: queryLanguage,
        statement: query.trim(),
        time_range: { start: from, end: to },
        stream: { name: streamName.trim(), stream_type: signal },
        limit: 1000,
      });
      return { result, from, to };
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const points = React.useMemo(
    () => (preview.data ? extractQueryPoints(preview.data.result) : []),
    [preview.data],
  );
  const chartTimestamps = React.useMemo(() => {
    if (!preview.data || points.length === 0) return [];
    return points.map(
      (point, index) =>
        point.timestamp ??
        preview.data!.from +
          ((preview.data!.to - preview.data!.from) * index) / Math.max(points.length - 1, 1),
    );
  }, [points, preview.data]);
  const primaryBand = React.useMemo(
    () =>
      thresholds
        .slice()
        .sort((left, right) => severityRank(left.severity) - severityRank(right.severity))[0] ??
      null,
    [thresholds],
  );
  const currentValue = points.at(-1)?.value;
  const estimatedEpisodes =
    primaryBand && points.length > 0
      ? estimateTriggerEpisodes(
          points.map((point) => point.value),
          primaryBand,
        )
      : null;
  const hasConflict = thresholdConflict(thresholds);
  const runbookValid = !runbook.trim() || isValidHttpUrl(runbook);
  const save = useMutation({
    mutationFn: (mode: SaveMode) => {
      validateBeforeSave({
        name,
        streamName,
        query,
        thresholds,
        hasConflict,
        t,
      });
      const bands = thresholds.map((band) => ({
        ...band,
        threshold: Number(band.threshold),
        for_periods: Math.max(1, Math.floor(band.for_periods)),
      }));
      const lowest =
        bands
          .slice()
          .sort(
            (left, right) => severityRank(left.severity) - severityRank(right.severity),
          )[0] ?? bands[0]!;
      const existing = ruleQuery.data;
      const payload: alertsApi.AlertRuleInput = {
        name: name.trim(),
        description: description.trim(),
        enabled: mode === 'active',
        kind: existing?.kind ?? 'scheduled',
        query: {
          language: queryLanguage,
          statement: query.trim(),
          period_secs: periodSecs,
          stream: { name: streamName.trim(), stream_type: signal },
        },
        trigger: {
          operator: lowest.operator,
          threshold: lowest.threshold,
          for_periods: lowest.for_periods,
          silence_secs: existing?.trigger.silence_secs ?? 300,
        },
        thresholds: bands,
        severity: existing?.severity ?? null,
        labels: {
          ...(existing?.labels ?? {}),
          ...(service.trim() ? { service: service.trim() } : {}),
        },
        annotations: {
          ...withoutLegacyNotifyAnnotations(existing?.annotations ?? {}),
          ...(runbook.trim() ? { runbook_url: runbook.trim() } : {}),
        },
        ...(existing?.escalation_policy_id
          ? { escalation_policy_id: existing.escalation_policy_id }
          : {}),
      };
      return ruleId
        ? alertsApi.update(ruleId, payload)
        : alertsApi.create(payload);
    },
    onSuccess: async (saved, mode) => {
      toast.success(
        mode === 'draft'
          ? t('workbench.toast.draft_saved', { name: saved.name })
          : t('workbench.toast.saved', { name: saved.name }),
      );
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });
      navigate('/alerts/rules');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const chooseSignal = (next: RuleSignal) => {
    setSignal(next);
    setQueryLanguage(next === 'metrics' ? 'promql' : 'sql');
    const first = PRESETS[next][0]!;
    setStreamName(first.stream);
    setQuery(first.statement);
    setThresholds([
      {
        severity: first.severity,
        operator: first.operator,
        threshold: first.threshold,
        for_periods: 5,
      },
    ]);
    preview.reset();
  };

  const applyPreset = (preset: QueryPreset) => {
    setQueryLanguage(signal === 'metrics' ? 'promql' : 'sql');
    setStreamName(preset.stream);
    setQuery(preset.statement);
    setThresholds([
      {
        severity: preset.severity,
        operator: preset.operator,
        threshold: preset.threshold,
        for_periods: 5,
      },
    ]);
    preview.reset();
  };

  if (isEdit && ruleQuery.isLoading) {
    return (
      <>
        <PageHeader
          title={t('workbench.edit_title')}
          backTo="/alerts/rules"
        />
        <PageBody>
          <LoadingState variant="list" rows={6} />
        </PageBody>
      </>
    );
  }

  if (isEdit && ruleQuery.isError) {
    return (
      <>
        <PageHeader
          title={t('workbench.edit_title')}
          backTo="/alerts/rules"
        />
        <PageBody>
          <ErrorState
            title={t('workbench.errors.load_rule')}
            error={ruleQuery.error}
            onRetry={() => void ruleQuery.refetch()}
          />
        </PageBody>
      </>
    );
  }

  return (
    <>
      <PageHeader
        title={isEdit ? t('workbench.edit_named', { name }) : t('workbench.new_title')}
        subtitle={t('workbench.subtitle')}
        backTo="/alerts/rules"
        toolbar={
          <>
            <ChromeButton variant="ghost" onClick={() => navigate('/alerts/rules')}>
              {t('workbench.actions.cancel')}
            </ChromeButton>
            {!isEdit && (
              <ChromeButton
                onClick={() => save.mutate('draft')}
                disabled={save.isPending || manageAccess.disabled}
                disabledReason={!save.isPending ? manageAccess.reason : undefined}
              >
                <Save className="h-4 w-4" />
                {t('workbench.actions.save_draft')}
              </ChromeButton>
            )}
            <ChromeButton
              onClick={() => preview.mutate()}
              disabled={preview.isPending || manageAccess.disabled}
              disabledReason={!preview.isPending ? manageAccess.reason : undefined}
            >
              <TestTube2 className="h-4 w-4" />
              {preview.isPending
                ? t('workbench.actions.testing')
                : t('workbench.actions.test')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              onClick={() => save.mutate('active')}
              disabled={save.isPending || manageAccess.disabled}
              disabledReason={!save.isPending ? manageAccess.reason : undefined}
            >
              <BellRing className="h-4 w-4" />
              {isEdit
                ? t('workbench.actions.save')
                : t('workbench.actions.create')}
            </ChromeButton>
          </>
        }
      />
      <WorkbenchSteps />
      <PageBody className="p-4 sm:p-5 xl:p-6">
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <div className="mx-auto grid max-w-[1680px] grid-cols-1 gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(360px,2fr)]">
          <main className="min-w-0 space-y-4">
            <WorkbenchSection
              id="identity"
              number="01"
              title={t('workbench.sections.identity')}
              description={t('workbench.sections.identity_description')}
            >
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                <FormField label={t('workbench.fields.name')} required>
                  <FormInput
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder={t('workbench.placeholders.name')}
                  />
                </FormField>
                <FormField label={t('workbench.fields.service')}>
                  <FormInput
                    value={service}
                    onChange={(event) => setService(event.target.value)}
                    placeholder={t('workbench.placeholders.service')}
                  />
                </FormField>
              </div>
              <FormField label={t('workbench.fields.description')}>
                <FormTextarea
                  value={description}
                  onChange={(event) => setDescription(event.target.value)}
                  rows={2}
                  placeholder={t('workbench.placeholders.description')}
                />
              </FormField>
            </WorkbenchSection>

            <WorkbenchSection
              id="condition"
              number="02"
              title={t('workbench.sections.condition')}
              description={t('workbench.sections.condition_description')}
            >
              <div>
                <div className={uiLabelClass}>{t('workbench.fields.signal')}</div>
                <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
                  {(['metrics', 'logs', 'traces'] as RuleSignal[]).map((item) => (
                    <button
                      key={item}
                      type="button"
                      onClick={() => chooseSignal(item)}
                      className={cn(
                        'flex min-h-14 items-center gap-3 rounded-md border px-3 text-left transition-colors',
                        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
                        signal === item
                          ? 'border-indigo bg-indigo-dim text-indigo-soft'
                          : 'border-bd-0 bg-bg-2 text-tx-2 hover:border-bd-1 hover:text-tx-0',
                      )}
                    >
                      {SIGNAL_ICON[item]}
                      <span className="font-sans text-sm font-strong">
                        {t(`workbench.signals.${item}`)}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              <div>
                <div className={uiLabelClass}>{t('workbench.quick_start')}</div>
                <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-3">
                  {PRESETS[signal].map((preset) => (
                    <button
                      key={preset.id}
                      type="button"
                      onClick={() => applyPreset(preset)}
                      className="rounded-md border border-bd-0 bg-bg-2 px-3 py-3 text-left hover:border-bd-2 hover:bg-bg-3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
                    >
                      <span className="block font-sans text-xs font-strong text-tx-0">
                        {t(preset.titleKey)}
                      </span>
                      <span className="mt-1 block text-xs leading-relaxed text-tx-3">
                        {t(preset.descriptionKey)}
                      </span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="grid grid-cols-1 gap-4 md:grid-cols-[minmax(0,1fr)_180px]">
                <FormField label={t('workbench.fields.stream')} required>
                  <FormInput
                    value={streamName}
                    onChange={(event) => setStreamName(event.target.value)}
                    placeholder={t('workbench.placeholders.stream')}
                  />
                </FormField>
                <FormField label={t('workbench.fields.interval')}>
                  <FormSelect
                    value={String(periodSecs)}
                    onChange={(value) => setPeriodSecs(Number(value))}
                    options={[
                      { value: '60', label: t('workbench.intervals.1m') },
                      { value: '300', label: t('workbench.intervals.5m') },
                      { value: '600', label: t('workbench.intervals.10m') },
                    ]}
                  />
                </FormField>
              </div>

              <FormField
                label={t('workbench.fields.query')}
                hint={t('workbench.hints.query')}
                required
              >
                {signal === 'metrics' && (
                  <div
                    role="group"
                    className="mb-2 flex w-fit items-center rounded-md border border-bd-0 bg-bg-2 p-0.5"
                    aria-label={t('workbench.fields.query_language')}
                  >
                    {(['promql', 'sql'] as QueryLanguage[]).map((language) => (
                      <button
                        key={language}
                        type="button"
                        aria-pressed={queryLanguage === language}
                        onClick={() => {
                          setQueryLanguage(language);
                          preview.reset();
                        }}
                        className={cn(
                          'rounded px-2.5 py-1 font-mono text-type-micro font-semibold uppercase transition-colors',
                          queryLanguage === language
                            ? 'bg-bg-0 text-indigo-soft shadow-sm'
                            : 'text-tx-3 hover:text-tx-1',
                        )}
                      >
                        {language}
                      </button>
                    ))}
                  </div>
                )}
                <CodeEditor
                  value={query}
                  onChange={setQuery}
                  language={queryLanguage}
                  label={queryLanguage === 'promql' ? 'PromQL' : 'SQL'}
                  ariaLabel={t('workbench.fields.query')}
                  minHeight={180}
                  maxHeight={360}
                  compact
                  resizable
                  showHeader
                  readOnly={manageAccess.disabled}
                />
              </FormField>

              <ThresholdEditor
                thresholds={thresholds}
                periodSecs={periodSecs}
                hasConflict={hasConflict}
                onChange={setThresholds}
              />
            </WorkbenchSection>

            <WorkbenchSection
              id="delivery"
              number="03"
              title={t('workbench.sections.delivery')}
              description={t('workbench.sections.delivery_description')}
            >
              <div className="flex flex-col gap-3 rounded-lg border border-bd-0 bg-bg-2 p-4 sm:flex-row sm:items-center">
                <div className="min-w-0 flex-1">
                  <div className="font-sans text-sm font-display-strong text-tx-0">
                    {t('workbench.notify.title', { defaultValue: 'Notify routing' })}
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-tx-3">
                    {t('workbench.notify.description', {
                      defaultValue: 'This rule emits alert.triggered, alert.acknowledged and alert.resolved events. Notify Policies choose recipients, connectors, templates and fallback routes.',
                    })}
                  </p>
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  <ChromeButton onClick={() => navigate('/settings/notify/policies')}>
                    {t('workbench.notify.manage_policies', { defaultValue: 'Manage policies' })}
                  </ChromeButton>
                  <ChromeButton onClick={() => navigate('/settings/notify/connectors')}>
                    {t('workbench.notify.manage_connectors', { defaultValue: 'Manage connectors' })}
                  </ChromeButton>
                </div>
              </div>
              <FormField
                label={t('workbench.fields.runbook')}
                hint={t('workbench.hints.runbook')}
              >
                <FormInput
                  value={runbook}
                  onChange={(event) => setRunbook(event.target.value)}
                  placeholder="https://runbooks.example.com/high-error-rate"
                  className={cn(!runbookValid && 'border-red/60')}
                />
                {!runbookValid && (
                  <span className="text-xs text-red-soft">
                    {t('workbench.errors.runbook_invalid')}
                  </span>
                )}
              </FormField>
            </WorkbenchSection>
          </main>

          <aside className="min-w-0 space-y-4 xl:sticky xl:top-4 xl:self-start">
            <QueryPreview
              pending={preview.isPending}
              attempted={preview.isSuccess || preview.isError}
              error={preview.error}
              points={points}
              timestamps={chartTimestamps}
              {...(preview.data
                ? { from: preview.data.from, to: preview.data.to }
                : {})}
              threshold={primaryBand}
              {...(currentValue !== undefined ? { currentValue } : {})}
              estimatedEpisodes={estimatedEpisodes}
              {...(preview.data
                ? {
                    scannedRows: preview.data.result.scanned_rows,
                    tookMs: preview.data.result.took_ms,
                  }
                : {})}
              onRun={() => preview.mutate()}
            />
            <ValidationSummary
              queryReady={points.length > 0}
              thresholdsReady={
                thresholds.length > 0 &&
                thresholds.every((band) => Number.isFinite(band.threshold)) &&
                !hasConflict
              }
              runbookReady={runbookValid}
            />
          </aside>
        </div>
        </fieldset>
      </PageBody>
    </>
  );
}

function WorkbenchSteps() {
  const { t } = useTranslation('alerts');
  return (
    <div className="flex min-h-12 items-center gap-2 overflow-x-auto border-b border-bd-0 bg-bg-1 px-4 sm:px-6">
      {[
        t('workbench.steps.identity'),
        t('workbench.steps.condition'),
        t('workbench.steps.delivery'),
      ].map((label, index) => (
        <React.Fragment key={label}>
          {index > 0 && <span className="h-px w-8 shrink-0 bg-bd-1" />}
          <span className="flex shrink-0 items-center gap-2 font-sans text-xs font-strong text-tx-2">
            <span className="grid h-5 w-5 place-items-center rounded-full border border-bd-1 bg-bg-2 font-mono text-type-micro text-tx-1">
              {index + 1}
            </span>
            {label}
          </span>
        </React.Fragment>
      ))}
    </div>
  );
}

function WorkbenchSection({
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
    <section id={id} className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <header className="flex items-start gap-3 border-b border-bd-0 px-4 py-3.5 sm:px-5">
        <span className="mt-0.5 font-mono text-xs font-semibold text-indigo-soft">{number}</span>
        <div>
          <h2 className="font-sans text-sm font-display-strong text-tx-0">{title}</h2>
          <p className="mt-0.5 text-xs leading-relaxed text-tx-3">{description}</p>
        </div>
      </header>
      <div className="space-y-5 p-4 sm:p-5">{children}</div>
    </section>
  );
}

function ThresholdEditor({
  thresholds,
  periodSecs,
  hasConflict,
  onChange,
}: {
  thresholds: SeverityThreshold[];
  periodSecs: number;
  hasConflict: boolean;
  onChange: (thresholds: SeverityThreshold[]) => void;
}) {
  const { t } = useTranslation('alerts');
  const update = (index: number, patch: Partial<SeverityThreshold>) =>
    onChange(
      thresholds.map((threshold, current) =>
        current === index ? { ...threshold, ...patch } : threshold,
      ),
    );
  return (
    <div>
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className={uiLabelClass}>{t('workbench.fields.thresholds')}</div>
          <div className="mt-0.5 text-xs text-tx-3">{t('workbench.hints.thresholds')}</div>
        </div>
        <ChromeButton
          size="sm"
          onClick={() =>
            onChange([
              ...thresholds,
              {
                severity: 'critical',
                operator: thresholds[0]?.operator ?? 'gt',
                threshold: thresholds[0]?.threshold ?? 0,
                for_periods: Math.max(1, Math.ceil(120 / periodSecs)),
              },
            ])
          }
        >
          <Plus className="h-3.5 w-3.5" />
          {t('workbench.actions.add_threshold')}
        </ChromeButton>
      </div>
      <div className="mt-3 space-y-2">
        {thresholds.map((band, index) => {
          const durationMinutes = Math.max(
            1,
            Math.round((band.for_periods * periodSecs) / 60),
          );
          return (
            <div
              key={`${band.severity}-${index}`}
              className="grid grid-cols-1 items-end gap-2 rounded-md border border-bd-0 bg-bg-2 p-3 md:grid-cols-[130px_150px_minmax(100px,1fr)_150px_32px]"
            >
              <FormField label={t('workbench.threshold.severity')}>
                <FormSelect
                  value={band.severity}
                  onChange={(value) => update(index, { severity: value as Severity })}
                  options={[
                    { value: 'info', label: t('severity.info') },
                    { value: 'warning', label: t('severity.warning') },
                    { value: 'error', label: t('severity.error') },
                    { value: 'critical', label: t('severity.critical') },
                  ]}
                  className="bg-bg-1"
                />
              </FormField>
              <FormField label={t('workbench.threshold.operator')}>
                <FormSelect
                  value={band.operator}
                  onChange={(value) => update(index, { operator: value as ComparisonOp })}
                  options={(
                    ['gt', 'gte', 'lt', 'lte', 'eq', 'neq'] as ComparisonOp[]
                  ).map((operator) => ({
                    value: operator,
                    label: `${COMPARISON_LABEL[operator]} ${t(`workbench.operators.${operator}`)}`,
                  }))}
                  className="bg-bg-1"
                />
              </FormField>
              <FormField label={t('workbench.threshold.value')}>
                <FormInput
                  type="number"
                  step="any"
                  value={String(band.threshold)}
                  onChange={(event) =>
                    update(index, { threshold: Number(event.target.value) })
                  }
                  className="bg-bg-1"
                />
              </FormField>
              <FormField label={t('workbench.threshold.duration')}>
                <div className="flex items-center gap-2">
                  <FormInput
                    type="number"
                    min={1}
                    value={String(durationMinutes)}
                    onChange={(event) =>
                      update(index, {
                        for_periods: Math.max(
                          1,
                          Math.ceil((Number(event.target.value) * 60) / periodSecs),
                        ),
                      })
                    }
                    className="bg-bg-1"
                  />
                  <span className="shrink-0 text-xs text-tx-2">
                    {t('workbench.threshold.minutes')}
                  </span>
                </div>
              </FormField>
              <button
                type="button"
                disabled={thresholds.length === 1}
                onClick={() => onChange(thresholds.filter((_, current) => current !== index))}
                className="grid h-9 w-8 place-items-center rounded-md text-tx-3 hover:bg-red-dim hover:text-red-soft disabled:cursor-not-allowed disabled:opacity-30"
                aria-label={t('workbench.actions.remove_threshold')}
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          );
        })}
      </div>
      {hasConflict && (
        <div className="mt-2 flex items-start gap-2 rounded-md border border-red/30 bg-red-dim px-3 py-2 text-xs text-red-soft">
          <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          {t('workbench.errors.threshold_conflict')}
        </div>
      )}
    </div>
  );
}

function QueryPreview({
  pending,
  attempted,
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
  const { t } = useTranslation('alerts');
  const hasData = points.length > 0;
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <header className="flex min-h-12 items-center gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="font-sans text-sm font-display-strong text-tx-0">
            {t('workbench.preview.title')}
          </h2>
          <p className="mt-0.5 text-xs text-tx-3">{t('workbench.preview.window')}</p>
        </div>
        <ChromeButton size="sm" onClick={onRun} disabled={pending}>
          <TestTube2 className="h-3.5 w-3.5" />
          {pending ? t('workbench.actions.testing') : t('workbench.actions.test')}
        </ChromeButton>
      </header>
      <div className="p-4">
        {pending ? (
          <div className="flex h-[220px] items-center justify-center">
            <LoadingState variant="list" rows={3} />
          </div>
        ) : error ? (
          <div className="flex min-h-[220px] flex-col items-center justify-center text-center">
            <CircleAlert className="h-7 w-7 text-red-soft" />
            <div className="mt-3 text-sm font-strong text-tx-0">
              {t('workbench.preview.failed')}
            </div>
            <div className="mt-1 max-w-sm text-xs leading-relaxed text-red-soft">
              {toApiError(error).message}
            </div>
          </div>
        ) : !attempted ? (
          <div className="flex min-h-[220px] flex-col items-center justify-center text-center">
            <Database className="h-7 w-7 text-tx-3" />
            <div className="mt-3 text-sm font-strong text-tx-0">
              {t('workbench.preview.not_run')}
            </div>
            <div className="mt-1 max-w-sm text-xs leading-relaxed text-tx-3">
              {t('workbench.preview.not_run_description')}
            </div>
          </div>
        ) : !hasData ? (
          <div className="flex min-h-[220px] flex-col items-center justify-center text-center">
            <Database className="h-7 w-7 text-tx-3" />
            <div className="mt-3 text-sm font-strong text-tx-0">
              {t('workbench.preview.no_data')}
            </div>
            <div className="mt-1 max-w-sm text-xs leading-relaxed text-tx-3">
              {t('workbench.preview.no_data_description')}
            </div>
          </div>
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
            <dl className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4 xl:grid-cols-2 min-[1560px]:grid-cols-4">
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

function PreviewStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className="mt-1 truncate font-mono text-sm font-semibold text-tx-0">{value}</dd>
    </div>
  );
}

function ValidationSummary({
  queryReady,
  thresholdsReady,
  runbookReady,
}: {
  queryReady: boolean;
  thresholdsReady: boolean;
  runbookReady: boolean;
}) {
  const { t } = useTranslation('alerts');
  const items = [
    { ready: queryReady, label: t('workbench.validation.query') },
    { ready: thresholdsReady, label: t('workbench.validation.thresholds') },
    { ready: runbookReady, label: t('workbench.validation.runbook') },
  ];
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <header className="flex min-h-12 items-center gap-2 border-b border-bd-0 px-4 py-3">
        <Check className="h-4 w-4 text-green-soft" />
        <h2 className="font-sans text-sm font-display-strong text-tx-0">
          {t('workbench.validation.title')}
        </h2>
      </header>
      <ul className="divide-y divide-bd-0">
        {items.map((item) => (
          <li key={item.label} className="flex min-h-10 items-center gap-2 px-4 py-2">
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

function withoutLegacyNotifyAnnotations(
  annotations: Record<string, string>,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(annotations).filter(([key]) => key !== 'channels'),
  );
}

function isValidHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch {
    return false;
  }
}

function formatNumber(value?: number): string {
  if (value === undefined) return '—';
  if (Math.abs(value) >= 1000) return value.toLocaleString(undefined, { maximumFractionDigits: 2 });
  return value.toLocaleString(undefined, { maximumFractionDigits: 4 });
}

function validateBeforeSave({
  name,
  streamName,
  query,
  thresholds,
  hasConflict,
  t,
}: {
  name: string;
  streamName: string;
  query: string;
  thresholds: SeverityThreshold[];
  hasConflict: boolean;
  t: ReturnType<typeof useTranslation<'alerts'>>['t'];
}) {
  if (!name.trim()) throw new Error(t('workbench.errors.name_required'));
  if (!streamName.trim()) throw new Error(t('workbench.errors.stream_required'));
  if (!query.trim()) throw new Error(t('workbench.errors.query_required'));
  if (thresholds.length === 0 || thresholds.some((band) => !Number.isFinite(band.threshold))) {
    throw new Error(t('workbench.errors.threshold_required'));
  }
  if (hasConflict) throw new Error(t('workbench.errors.threshold_conflict'));
}

export const alertRuleWorkbenchTestables = {
  isValidHttpUrl,
  formatNumber,
  ruleSeverity,
  topThreshold,
};
