import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellRing, Save } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as queryApi from '@/api/query';
import { toApiError } from '@/lib/http';
import { PROMQL_RANGE_ROWS, promqlStepMicros } from '@/lib/promqlRange';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
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
import { useDraftTouched } from '@/shell/unsaved/useDraftTouched';
import { useLeaveGuard } from '@/shell/unsaved/useLeaveGuard';
import { useAuthStore } from '@/stores/auth';
import type {
  AlertRuleKind,
  ComparisonOp,
  CompositeQuery,
  ForecastParams,
  HeartbeatParams,
  OutlierParams,
  Priority,
  Severity,
  SeverityThreshold,
  Urgency,
} from '@/types/alerting';
import type { QueryLanguage } from '@/types/query';

import {
  alertRuleActivationBlocker,
  alertRulePreviewFingerprint,
  extractQueryPoints,
  ruleSeverity,
  seedThresholds,
  severityRank,
  thresholdConflict,
  topThreshold,
} from './alertRuleModel';
import {
  conditionsOf,
  conditionsPayload,
  minutesFor,
  periodsFor,
  recoveryLineInvalid,
  type RuleConditions,
} from './rule-workbench/conditions';
import { ConditionsEditor } from './rule-workbench/ConditionsEditor';
import { DelayField } from './rule-workbench/DelayField';
import {
  countGroups,
  formatGroupBy,
  MAX_GROUP_BY_COLUMNS,
  missingGroupColumns,
  parseGroupBy,
} from './rule-workbench/groupBy';
import { HomeClusterField } from './rule-workbench/HomeClusterField';
import { ImportanceFields } from './rule-workbench/ImportanceFields';
import {
  CompositeFields,
  ForecastFields,
  HeartbeatAddressDialog,
  HeartbeatFields,
  JudgedSeverity,
  KindPicker,
  OutlierFields,
} from './rule-workbench/KindFields';
import {
  DEFAULT_FORECAST,
  DEFAULT_HEARTBEAT,
  DEFAULT_OUTLIER,
  compositeProblem,
  defaultComposite,
  isWorkbenchKind,
  judgesItself,
  kindParamsOf,
  type WorkbenchKind,
  withoutQuery,
} from './rule-workbench/kinds';
import { type QueryPreset, PRESETS, type RuleSignal } from './rule-workbench/presets';
import { previewSource } from './rule-workbench/previewSource';
import { emptyRumDraft } from './rule-workbench/rumSource';
import {
  formatNumber,
  isValidHttpUrl,
  validateBeforeSave,
  withoutLegacyNotifyAnnotations,
} from './rule-workbench/saveModel';
import { SourceFields } from './rule-workbench/SourceFields';
import { SourceTiles } from './rule-workbench/SourceTiles';
import {
  QueryPreview,
  ValidationSummary,
  WorkbenchSection,
} from './rule-workbench/Surfaces';
import { ThresholdEditor } from './rule-workbench/ThresholdEditor';
import {
  type SourceKind,
  sourceForPeriods,
  sourceFromSearch,
  sourceIsNamed,
  sourceKey,
  sourceLabels,
  sourceOfRule,
  sourceRuleQuery,
  sourceThreshold,
  sourceWindow,
  type TypedSource,
} from './rule-workbench/typedSource';

type SaveMode = 'draft' | 'active';

const HOUR_MICROS = 3_600_000_000;
const PREVIEW_PROMQL_STEP_MICROS = 60_000_000;

export function AlertRuleWorkbench() {
  const { t } = useTranslation('alerts');
  const navigate = useNavigate();
  const location = useLocation();
  const params = useParams();
  const ruleId = params.id;
  const isEdit = Boolean(ruleId);
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.rules.manage' });

  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [service, setService] = React.useState('');
  const [signal, setSignal] = React.useState<RuleSignal>('metrics');
  const [queryLanguage, setQueryLanguage] = React.useState<QueryLanguage>('promql');
  const [streamName, setStreamName] = React.useState('http_requests_total');
  const [query, setQuery] = React.useState(PRESETS.metrics[0]!.statement);
  // The columns of the result the rule is split by, as typed: each group opens its own incident.
  const [groupBy, setGroupBy] = React.useState('');
  // A figure of APM or RUM watched in place of a query; a page of the product opens the editor with one.
  const [source, setSource] = React.useState<TypedSource | null>(() => sourceFromSearch(location.search));
  const [periodSecs, setPeriodSecs] = React.useState(() => (source ? sourceWindow(source) : 60));
  // How far back each round's window is moved; null leaves it to the deployment.
  const [delaySecs, setDelaySecs] = React.useState<number | null>(null);
  // The cluster of a federation that decides a real-time rule; null lets every cluster decide by itself.
  const [homeCluster, setHomeCluster] = React.useState<string | null>(null);
  // What the rule's incidents open with; unset leaves them to pipelines and the escalation policy.
  const [priority, setPriority] = React.useState<Priority | null>(null);
  const [urgency, setUrgency] = React.useState<Urgency | null>(null);
  const [thresholds, setThresholds] = React.useState<SeverityThreshold[]>(() => [
    {
      severity: 'warning',
      ...(source ? sourceThreshold(source) : { operator: 'gt' as const, threshold: 0.05 }),
      forPeriods: source ? sourceForPeriods(source) : 5,
    },
  ]);
  // When an incident recovers, and what a round without a value counts as.
  const [conditions, setConditions] = React.useState<RuleConditions>(() => conditionsOf(undefined, periodSecs));
  const [runbook, setRunbook] = React.useState('');
  // How the rule fires, and what the kinds that judge themselves need.
  const [ruleKind, setRuleKind] = React.useState<WorkbenchKind>('scheduled');
  const [composite, setComposite] = React.useState<CompositeQuery>(defaultComposite);
  const [forecast, setForecast] = React.useState<ForecastParams>(DEFAULT_FORECAST);
  const [outlier, setOutlier] = React.useState<OutlierParams>(DEFAULT_OUTLIER);
  const [heartbeat, setHeartbeat] = React.useState<HeartbeatParams>(DEFAULT_HEARTBEAT);
  const [forecastLine, setForecastLine] = React.useState<{ operator: ComparisonOp; threshold: number }>({
    operator: 'gte',
    threshold: 90,
  });
  const [judgedSeverity, setJudgedSeverity] = React.useState<Severity>('warning');
  const [judgedMinutes, setJudgedMinutes] = React.useState(1);
  // A heartbeat rule's check-in address, shown once after it is made or reset.
  const [heartbeatAddress, setHeartbeatAddress] = React.useState<alertsApi.HeartbeatAddress | null>(null);
  const [leaveAfterAddress, setLeaveAfterAddress] = React.useState(false);
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
    setSignal(rule.query.stream?.streamType ?? 'metrics');
    setQueryLanguage(rule.query.language);
    setStreamName(rule.query.stream?.name ?? '');
    setQuery(rule.query.statement);
    setGroupBy(formatGroupBy(rule.query.groupBy));
    setConditions(conditionsOf(rule, rule.query.periodSecs || 60));
    setSource(sourceOfRule(rule));
    setPeriodSecs(rule.query.periodSecs || 60);
    setDelaySecs(rule.query.delaySecs ?? null);
    setHomeCluster(rule.query.homeCluster ?? null);
    setPriority(rule.trigger.priority ?? null);
    setUrgency(rule.trigger.urgency ?? null);
    setThresholds(seedThresholds(rule));
    setRunbook(rule.annotations.runbook_url ?? '');
    if (isWorkbenchKind(rule.kind)) setRuleKind(rule.kind ?? 'scheduled');
    const params = kindParamsOf(rule);
    setComposite(params.composite);
    setForecast(params.forecast);
    setOutlier(params.outlier);
    setHeartbeat(params.heartbeat);
    setForecastLine({ operator: rule.trigger.operator, threshold: rule.trigger.threshold });
    setJudgedSeverity(ruleSeverity(rule));
    setJudgedMinutes(minutesFor(rule.trigger.forPeriods, rule.query.periodSecs || 60));
  }, [ruleQuery.data]);

  // The kinds made here are picked; an anomaly or real-time rule keeps its own. A figure of a product
  // watches one scope: it is compared against thresholds or forecast, never split.
  const loadedKind: AlertRuleKind = ruleQuery.data?.kind ?? 'scheduled';
  const kind: AlertRuleKind = isWorkbenchKind(loadedKind)
    ? source && ruleKind !== 'forecast'
      ? 'scheduled'
      : ruleKind
    : loadedKind;
  const noQuery = withoutQuery(kind);
  const judged = judgesItself(kind) && kind !== 'anomaly';
  const canSplit = !source && (kind === 'scheduled' || kind === 'outlier');
  const groupColumns = canSplit ? parseGroupBy(groupBy) : [];
  const previewInput = {
    signal,
    queryLanguage,
    streamName: source ? source.kind : streamName,
    statement: source ? `${sourceKey(source)}@${periodSecs}` : query,
    groupBy: groupColumns,
  };
  const currentPreviewFingerprint = alertRulePreviewFingerprint(previewInput);

  const preview = useMutation({
    mutationFn: async () => {
      if (!orgId) throw new Error(t('workbench.errors.org_required'));
      if (source) {
        if (!sourceIsNamed(source)) {
          throw new Error(t(source.kind === 'apm' ? 'workbench.errors.apm_service_required' : 'workbench.errors.rum_application_required'));
        }
        const shown = await previewSource(source, periodSecs);
        if (shown.unavailable) throw new Error(t(`workbench.rum.unavailable.${shown.unavailable}`));
        return { result: shown.result, from: shown.from, to: shown.to, fingerprint: alertRulePreviewFingerprint(previewInput) };
      }
      if (!streamName.trim()) throw new Error(t('workbench.errors.stream_required'));
      if (streamName.trim() === '_audit') throw new Error(t('workbench.errors.stream_audit'));
      if (!query.trim()) throw new Error(t('workbench.errors.query_required'));
      const to = Date.now() * 1000;
      const from = to - 6 * 60 * 60 * 1_000_000;
      const fingerprint = alertRulePreviewFingerprint({
        signal,
        queryLanguage,
        streamName,
        statement: query,
        groupBy: groupColumns,
      });
      const result = await queryApi.runQuery({
        orgId: orgId,
        language: queryLanguage,
        statement: query.trim(),
        timeRange: { start: from, end: to },
        stream: { name: streamName.trim(), streamType: signal },
        // PromQL is previewed as a series a minute apart over the six hours.
        ...(queryLanguage === 'promql'
          ? { step: promqlStepMicros(from, to, PREVIEW_PROMQL_STEP_MICROS), limit: PROMQL_RANGE_ROWS }
          : { limit: 1000 }),
      });
      const missing = missingGroupColumns(result, groupColumns);
      if (missing.length > 0) {
        throw new Error(t('workbench.errors.group_by_missing', { columns: missing.join(', ') }));
      }
      return { result, from, to, fingerprint };
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  // A RUM figure is read over as many windows as the server chose; a query over six hours.
  const previewHours =
    source?.kind === 'rum'
      ? preview.data
        ? Math.max(1, Math.round((preview.data.to - preview.data.from) / HOUR_MICROS))
        : null
      : undefined;

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
  const hasConflict = !judged && thresholdConflict(thresholds);
  const recoveryInvalid = !judgesItself(kind) && recoveryLineInvalid(thresholds, conditions.recoveryThreshold);
  const runbookValid = !runbook.trim() || isValidHttpUrl(runbook);
  const identityReady = Boolean(name.trim());
  const thresholdsReady = judged
    ? kind !== 'forecast' || Number.isFinite(forecastLine.threshold)
    : thresholds.length > 0 &&
      thresholds.every((band) => Number.isFinite(band.threshold)) &&
      !hasConflict &&
      !recoveryInvalid;
  const previewMatchesCurrentInputs =
    preview.data?.fingerprint === currentPreviewFingerprint;
  const queryReady =
    kind === 'composite'
      ? compositeProblem(composite) === null
      : kind === 'heartbeat'
        ? heartbeat.periodSecs >= 60
        : preview.isSuccess &&
          previewMatchesCurrentInputs &&
          points.length > 0 &&
          (kind !== 'outlier' || groupColumns.length > 0);
  const activationBlocker = alertRuleActivationBlocker({
    identityReady,
    previewRunning: preview.isPending,
    queryReady,
    thresholdsReady,
    runbookReady: runbookValid,
  });
  const activationDisabledReason = activationBlocker
    ? activationBlocker === 'preview_required' && noQuery
      ? t(`workbench.validation.blockers.${kind}`)
      : activationBlocker === 'preview_required' && kind === 'outlier' && groupColumns.length === 0
        ? t('workbench.outlier.needs_split')
        : t(`workbench.validation.blockers.${activationBlocker}`)
    : undefined;
  const draft = useDraftTouched({ buttons: true });
  const leave = useLeaveGuard(draft.touched);

  const save = useMutation({
    mutationFn: (mode: SaveMode) => {
      if (source && !sourceIsNamed(source)) {
        throw new Error(t(source.kind === 'apm' ? 'workbench.errors.apm_service_required' : 'workbench.errors.rum_application_required'));
      }
      if (noQuery) {
        if (!name.trim()) throw new Error(t('workbench.errors.name_required'));
        const problem = kind === 'composite' ? compositeProblem(composite) : null;
        if (problem) {
          throw new Error(
            t(`workbench.composite.problems.${problem.kind}`, {
              letters: 'letters' in problem ? problem.letters.join(', ') : '',
            }),
          );
        }
      } else {
        validateBeforeSave({
          name,
          streamName: source ? source.kind : streamName,
          query: source ? sourceKey(source) : query,
          thresholds: judged
            ? [{ severity: judgedSeverity, ...forecastLine, threshold: kind === 'forecast' ? forecastLine.threshold : 0, forPeriods: 1 }]
            : thresholds,
          hasConflict,
          t,
        });
      }
      if (kind === 'outlier' && groupColumns.length === 0) throw new Error(t('workbench.outlier.needs_split'));
      if (groupColumns.length > MAX_GROUP_BY_COLUMNS) {
        throw new Error(t('workbench.errors.group_by_too_many', { count: MAX_GROUP_BY_COLUMNS }));
      }
      if (recoveryInvalid) throw new Error(t('workbench.errors.recovery_side'));
      const bands = thresholds.map((band) => ({
        ...band,
        threshold: Number(band.threshold),
        forPeriods: Math.max(1, Math.floor(band.forPeriods)),
      }));
      const lowest =
        bands
          .slice()
          .sort(
            (left, right) => severityRank(left.severity) - severityRank(right.severity),
          )[0] ?? bands[0]!;
      const existing = ruleQuery.data;
      const queried = source
        ? sourceRuleQuery(source, periodSecs)
        : {
            language: queryLanguage,
            statement: query.trim(),
            periodSecs,
            stream: { name: streamName.trim(), streamType: signal },
            groupBy: groupColumns,
          };
      const delayed = delaySecs === null ? {} : { delaySecs };
      const homed = kind === 'real_time' && homeCluster ? { homeCluster } : {};
      const ruleQueryPayload: alertsApi.AlertRuleInput['query'] =
        kind === 'composite'
          ? { language: 'sql', statement: composite.expression.trim(), periodSecs: 60, stream: null, composite }
          : kind === 'heartbeat'
            ? { language: 'sql', statement: 'heartbeat', periodSecs: 60, stream: null, heartbeat }
            : kind === 'forecast'
              ? { ...queried, ...delayed, forecast }
              : kind === 'outlier'
                ? { ...queried, ...delayed, outlier }
                : { ...queried, ...delayed, ...homed };
      const rulePeriod = noQuery ? 60 : periodSecs;
      const importance = { ...(priority ? { priority } : {}), ...(urgency ? { urgency } : {}) };
      const payload: alertsApi.AlertRuleInput = {
        name: name.trim(),
        description: description.trim(),
        enabled: mode === 'active',
        kind,
        query: ruleQueryPayload,
        trigger: judged
          ? {
              operator: kind === 'forecast' ? forecastLine.operator : 'gt',
              threshold: kind === 'forecast' ? forecastLine.threshold : 0,
              forPeriods: kind === 'heartbeat' ? 1 : periodsFor(judgedMinutes, rulePeriod),
              silenceSecs: existing?.trigger.silenceSecs ?? 300,
              ...conditionsPayload(conditions, kind),
              ...importance,
            }
          : {
              operator: lowest.operator,
              threshold: lowest.threshold,
              forPeriods: lowest.forPeriods,
              silenceSecs: existing?.trigger.silenceSecs ?? 300,
              ...conditionsPayload(conditions, kind),
              ...importance,
            },
        thresholds: judged ? [] : bands,
        severity: judged ? judgedSeverity : existing?.severity ?? null,
        labels: {
          ...(existing?.labels ?? {}),
          ...(source ? sourceLabels(source) : service.trim() ? { service: service.trim() } : {}),
        },
        annotations: {
          ...withoutLegacyNotifyAnnotations(existing?.annotations ?? {}),
          ...(runbook.trim() ? { runbook_url: runbook.trim() } : {}),
        },
        ...(existing?.escalationPolicyId
          ? { escalationPolicyId: existing.escalationPolicyId }
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
      leave.allowLeave();
      // A heartbeat rule's first address is shown before leaving: it is not shown again.
      if (saved.heartbeatAddress) {
        setLeaveAfterAddress(true);
        setHeartbeatAddress(saved.heartbeatAddress);
        return;
      }
      navigate('/alerts/rules');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const chooseSource = (kind: SourceKind) => {
    const next: TypedSource =
      source?.kind === kind
        ? source
        : kind === 'apm'
          ? { kind: 'apm', query: { service: '', metric: 'error_rate' } }
          : { kind: 'rum', query: emptyRumDraft() };
    // The windows of one product are not the windows of another.
    if (source?.kind !== kind) setPeriodSecs(sourceWindow(next));
    setSource(next);
    setThresholds([{ severity: 'warning', ...sourceThreshold(next), forPeriods: sourceForPeriods(next) }]);
    preview.reset();
  };

  // A figure of a product watched with another metric is another unit: its thresholds and window start again.
  const changeSource = (next: TypedSource) => {
    if (next.kind === 'rum' && source?.kind === 'rum' && next.query.metric !== source.query.metric) {
      setPeriodSecs(sourceWindow(next));
      setThresholds([{ severity: 'warning', ...sourceThreshold(next), forPeriods: sourceForPeriods(next) }]);
    }
    setSource(next);
    preview.reset();
  };

  const chooseSignal = (next: RuleSignal) => {
    if (source) setPeriodSecs(60);
    setSource(null);
    setSignal(next);
    setQueryLanguage(next === 'metrics' ? 'promql' : 'sql');
    const first = PRESETS[next][0]!;
    setStreamName(first.stream);
    setQuery(first.statement);
    setGroupBy('');
    setThresholds([
      {
        severity: first.severity,
        operator: first.operator,
        threshold: first.threshold,
        forPeriods: 5,
      },
    ]);
    preview.reset();
  };

  const chooseKind = (next: WorkbenchKind) => {
    // An outlier rule compares the groups of a query; a figure of a product has none.
    if (next === 'outlier' && source) {
      setSource(null);
      setPeriodSecs(60);
    }
    setRuleKind(next);
    preview.reset();
  };

  const applyPreset = (preset: QueryPreset) => {
    setQueryLanguage(signal === 'metrics' ? 'promql' : 'sql');
    setStreamName(preset.stream);
    setQuery(preset.statement);
    setGroupBy('');
    setThresholds([
      {
        severity: preset.severity,
        operator: preset.operator,
        threshold: preset.threshold,
        forPeriods: 5,
      },
    ]);
    preview.reset();
  };

  if (isEdit && ruleQuery.isLoading) {
    return (
      <>
        <PageHeader
          title={t('workbench.edit_title')}
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
      {leave.prompt}
      <PageHeader
        title={isEdit ? t('workbench.edit_named', { name }) : t('workbench.new_title')}
        subtitle={t('workbench.subtitle')}
        toolbar={
          <>
            <ChromeButton
              onClick={() => {
                leave.allowLeave();
                navigate('/alerts/rules');
              }}
            >
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
              variant="primary"
              onClick={() => save.mutate('active')}
              disabled={
                save.isPending ||
                manageAccess.disabled ||
                activationBlocker !== null
              }
              disabledReason={
                !save.isPending
                  ? manageAccess.disabled
                    ? manageAccess.reason
                    : activationDisabledReason
                  : undefined
              }
            >
              <BellRing className="h-4 w-4" />
              {isEdit
                ? t('workbench.actions.save')
                : t('workbench.actions.create')}
            </ChromeButton>
          </>
        }
      />
      <PageBody className="p-4 sm:p-5 xl:p-6">
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
          {...draft.events}
        >
        <div className="mx-auto grid max-w-[1680px] grid-cols-1 gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(360px,2fr)]">
          <main className="min-w-0 space-y-6">
            <WorkbenchSection
              id="identity"
              number="01"
              title={t('workbench.sections.identity')}
              description={t('workbench.sections.identity_description')}
            >
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                <FormField label={t('workbench.fields.name')} required {...(source ? { className: 'md:col-span-2' } : {})}>
                  <FormInput
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder={t('workbench.placeholders.name')}
                  />
                </FormField>
                {/* An APM rule's service is the one its source watches. */}
                {!source && (
                  <FormField label={t('workbench.fields.service')}>
                    <FormInput
                      value={service}
                      onChange={(event) => setService(event.target.value)}
                      placeholder={t('workbench.placeholders.service')}
                    />
                  </FormField>
                )}
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
              {isWorkbenchKind(loadedKind) && <KindPicker value={ruleKind} onChange={chooseKind} />}

              {kind === 'composite' && <CompositeFields value={composite} ownId={ruleId} onChange={setComposite} />}
              {kind === 'heartbeat' && (
                <HeartbeatFields value={heartbeat} ruleId={ruleId} onChange={setHeartbeat} onAddress={setHeartbeatAddress} />
              )}

              {!noQuery && (
              <SourceTiles
                signal={signal}
                source={source}
                onSignal={chooseSignal}
                onSource={chooseSource}
                typed={kind !== 'outlier'}
              />
              )}

              {noQuery ? null : source ? (
                <>
                  <SourceFields
                    source={source}
                    onChange={changeSource}
                    periodSecs={periodSecs}
                    onPeriodChange={(secs) => {
                      setPeriodSecs(secs);
                      preview.reset();
                    }}
                  />
                  {source.kind === 'apm' && (
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
                  )}
                </>
              ) : (
                <>

                  <div>
                    <div className={uiLabelClass}>{t('workbench.quick_start')}</div>
                    <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-3">
                      {PRESETS[signal].map((preset) => (
                        <button
                          key={preset.id}
                          type="button"
                          onClick={() => applyPreset(preset)}
                          className="bg-bg-2 px-3 py-3 text-left hover:bg-bg-3 focus-visible:bg-indigo-dim focus-visible:text-indigo"
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
                              'rounded-[5px] px-2.5 py-1 font-mono text-xs font-strong transition-colors',
                              queryLanguage === language
                                ? 'bg-bg-0 text-indigo-soft'
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

                  {canSplit && (
                    <FormField label={t('workbench.fields.group_by')} hint={t('workbench.hints.group_by')}>
                      <FormInput
                        value={groupBy}
                        onChange={(event) => setGroupBy(event.target.value)}
                        placeholder={t('workbench.placeholders.group_by')}
                        className="font-mono"
                      />
                    </FormField>
                  )}
                </>
              )}

              {!noQuery && kind !== 'real_time' && (
                <DelayField value={delaySecs} onChange={setDelaySecs} />
              )}
              {kind === 'real_time' && <HomeClusterField value={homeCluster} onChange={setHomeCluster} />}

              {kind === 'forecast' && (
                <ForecastFields
                  value={forecast}
                  operator={forecastLine.operator}
                  threshold={forecastLine.threshold}
                  onChange={setForecast}
                  onLine={(operator, threshold) => setForecastLine({ operator, threshold })}
                />
              )}
              {kind === 'outlier' && <OutlierFields value={outlier} onChange={setOutlier} />}

              {judged ? (
                <JudgedSeverity
                  severity={judgedSeverity}
                  minutes={judgedMinutes}
                  showMinutes={kind !== 'heartbeat'}
                  onSeverity={setJudgedSeverity}
                  onMinutes={setJudgedMinutes}
                />
              ) : (
                <ThresholdEditor
                  thresholds={thresholds}
                  periodSecs={periodSecs}
                  hasConflict={hasConflict}
                  onChange={setThresholds}
                />
              )}

              <ConditionsEditor
                value={conditions}
                kind={kind}
                periodSecs={noQuery ? 60 : periodSecs}
                recoveryInvalid={recoveryInvalid}
                onChange={setConditions}
              />
            </WorkbenchSection>

            <WorkbenchSection
              id="delivery"
              number="03"
              title={t('workbench.sections.delivery')}
              description={t('workbench.sections.delivery_description')}
            >
              <ImportanceFields
                priority={priority}
                urgency={urgency}
                onPriority={setPriority}
                onUrgency={setUrgency}
              />
              <div className="flex flex-col gap-3 bg-bg-2 px-4 py-3 sm:flex-row sm:items-center">
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

          <aside className="min-w-0 space-y-6 xl:sticky xl:top-4 xl:self-start">
            {!noQuery && (
            <QueryPreview
              pending={preview.isPending}
              attempted={preview.isSuccess || preview.isError}
              stale={preview.isSuccess && !previewMatchesCurrentInputs}
              error={preview.error}
              points={points}
              timestamps={chartTimestamps}
              {...(preview.data
                ? { from: preview.data.from, to: preview.data.to }
                : {})}
              threshold={primaryBand}
              {...(currentValue !== undefined ? { currentValue } : {})}
              {...(preview.data
                ? {
                    scannedRows: preview.data.result.scannedRows,
                    tookMs: preview.data.result.tookMs,
                  }
                : {})}
              {...(previewHours !== undefined ? { hours: previewHours } : {})}
              {...(preview.data && groupColumns.length > 0
                ? { groups: { columns: groupColumns, count: countGroups(preview.data.result, groupColumns) } }
                : {})}
              onRun={() => preview.mutate()}
            />
            )}
            <ValidationSummary
              identityReady={identityReady}
              queryReady={queryReady}
              thresholdsReady={thresholdsReady}
              runbookReady={runbookValid}
              {...(noQuery ? { queryLabel: t(`workbench.validation.${kind}`) } : {})}
            />
          </aside>
        </div>
        </fieldset>
      </PageBody>
      <HeartbeatAddressDialog
        address={heartbeatAddress}
        onClose={() => {
          setHeartbeatAddress(null);
          if (leaveAfterAddress) navigate('/alerts/rules');
        }}
      />
    </>
  );
}

export const alertRuleWorkbenchTestables = {
  isValidHttpUrl,
  formatNumber,
  ruleSeverity,
  topThreshold,
};
