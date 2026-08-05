import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import {
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { QueryState, queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import type { AlertRule, Severity, StreamType } from '@/types/alerting';

const SEVERITY_OPTIONS: Severity[] = ['critical', 'error', 'warning', 'info'];

const STREAM_TYPES: StreamType[] = ['logs', 'metrics', 'traces'];
const WINDOW_OPTIONS = [
  { value: '60', label: '1m' },
  { value: '300', label: '5m' },
  { value: '900', label: '15m' },
  { value: '3600', label: '1h' },
];
/** `mad` and `ewma` are implemented backend-side; the others 400 on create. */
const ALGORITHMS = ['mad', 'ewma'];

interface AnomalyForm {
  name: string;
  severity: Severity;
  service: string;
  streamName: string;
  streamType: StreamType;
  language: 'sql' | 'promql';
  periodSecs: string;
  statement: string;
  algorithm: string;
  lookbackDays: string;
  k: string;
  alpha: string;
  weeklySeasonality: boolean;
  forPeriods: string;
}

const EMPTY_FORM: AnomalyForm = {
  name: '',
  severity: 'warning',
  service: '',
  streamName: '',
  streamType: 'metrics',
  language: 'sql',
  periodSecs: '300',
  statement: 'SELECT avg(response_ms) FROM app_metrics',
  algorithm: 'mad',
  lookbackDays: '7',
  k: '3',
  alpha: '0.3',
  weeklySeasonality: false,
  forPeriods: '1',
};

function formFromRule(rule: AlertRule): AnomalyForm {
  return {
    name: rule.name,
    severity: (rule.labels?.severity as Severity) ?? 'warning',
    service: rule.labels?.service ?? '',
    streamName: rule.query?.stream?.name ?? '',
    streamType: rule.query?.stream?.stream_type ?? 'metrics',
    language: rule.query?.language ?? 'sql',
    periodSecs: String(rule.query?.period_secs ?? 300),
    statement: rule.query?.statement ?? '',
    algorithm: rule.anomaly_params?.algorithm ?? 'mad',
    lookbackDays: String(rule.anomaly_params?.lookback_days ?? 7),
    k: String(rule.anomaly_params?.k ?? 3),
    alpha: String(rule.anomaly_params?.alpha ?? 0.3),
    weeklySeasonality: rule.anomaly_params?.weekly_seasonality ?? false,
    forPeriods: String(rule.trigger?.for_periods ?? 1),
  };
}

export function AlertsAnomaly() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const { id } = useParams<{ id: string }>();
  const isEdit = Boolean(id);
  const nav = useNavigate();
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });

  const ruleQuery = useQuery({
    queryKey: ['alerts', 'rules', id],
    queryFn: () => alertsApi.get(id!),
    enabled: isEdit,
  });

  const [form, setForm] = React.useState<AnomalyForm>(EMPTY_FORM);
  // Hydrate the form once the edited rule arrives. The ref guards against
  // clobbering user edits if the query refetches in the background.
  const hydratedFor = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (isEdit && ruleQuery.data && hydratedFor.current !== id) {
      setForm(formFromRule(ruleQuery.data));
      hydratedFor.current = id ?? null;
    }
  }, [isEdit, ruleQuery.data, id]);

  const set = <K extends keyof AnomalyForm>(key: K, value: AnomalyForm[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const save = useMutation({
    mutationFn: () => {
      const periodSecs = clampInt(form.periodSecs, 300, 1, 86_400);
      // Weekly seasonality samples one point per week, so it needs at least a
      // 7-day window (the backend rejects < 7).
      const lookbackDays = clampInt(form.lookbackDays, 7, form.weeklySeasonality ? 7 : 1, 30);
      const k = clampFloat(form.k, 3, 0.1, 100);
      // α only applies to EWMA; backend ignores it for other algorithms.
      const alpha = clampFloat(form.alpha, 0.3, 0.01, 1);
      const forPeriods = clampInt(form.forPeriods, 1, 1, 100);
      const labels: Record<string, string> = {
        ...(ruleQuery.data?.labels ?? {}),
        severity: form.severity,
      };
      if (form.service.trim()) labels.service = form.service.trim();
      else delete labels.service;

      const payload: alertsApi.AlertRuleInput = {
        name: form.name.trim(),
        description: ruleQuery.data?.description ?? '',
        enabled: ruleQuery.data?.enabled ?? true,
        kind: 'anomaly',
        query: {
          language: form.language,
          statement: form.statement,
          period_secs: periodSecs,
          stream: { name: form.streamName.trim(), stream_type: form.streamType },
        },
        trigger: {
          operator: 'gt',
          threshold: 0,
          for_periods: forPeriods,
          silence_secs: ruleQuery.data?.trigger?.silence_secs ?? 300,
        },
        anomaly_params: {
          algorithm: form.algorithm,
          lookback_days: lookbackDays,
          k,
          weekly_seasonality: form.weeklySeasonality,
          ...(form.algorithm === 'ewma' ? { alpha } : {}),
        },
        labels,
        annotations: ruleQuery.data?.annotations ?? {},
      };
      if (ruleQuery.data?.escalation_policy_id) {
        payload.escalation_policy_id = ruleQuery.data.escalation_policy_id;
      }
      return isEdit ? alertsApi.update(id!, payload) : alertsApi.create(payload);
    },
    onSuccess: () => {
      toast.success(t('anomaly.toast_saved', { name: form.name }));
      void qc.invalidateQueries({ queryKey: ['alerts', 'rules'] });
      nav('/alerts/rules');
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!manageAccess.allowed) return;
    if (!form.name.trim()) {
      toast.error(t('anomaly.errors.name_required'));
      return;
    }
    if (!form.streamName.trim()) {
      toast.error(t('anomaly.errors.stream_required'));
      return;
    }
    save.mutate();
  };

  // When editing, surface load failures before rendering a blank form.
  const loadState = isEdit
    ? queryStateFor({
        isLoading: ruleQuery.isLoading,
        isError: ruleQuery.isError,
        data: ruleQuery.data ? [ruleQuery.data] : [],
      })
    : null;

  return (
    <>
      <PageHeader
        title={isEdit ? t('anomaly.edit_title') : t('anomaly.new_title')}
        subtitle={t('anomaly.subtitle')}
      />
      <PageBody>
        {loadState === 'loading' || loadState === 'error' ? (
          <QueryState
            state={loadState}
            error={ruleQuery.error}
            loadingLabel={tc('loading', { defaultValue: 'Loading…' })}
          />
        ) : (
          <form onSubmit={submit} className="max-w-3xl">
            <fieldset
              disabled={manageAccess.disabled}
              aria-disabled={manageAccess.disabled || undefined}
              className="contents"
            >
            <FormSection title={t('anomaly.sections.identity')}>
              <FormField label={t('anomaly.fields.name')} required hint={t('anomaly.hints.name')}>
                <FormInput
                  value={form.name}
                  onChange={(e) => set('name', e.target.value)}
                  placeholder={t('anomaly.placeholders.name')}
                  required
                />
              </FormField>
              <FormRow>
                <FormField label={t('anomaly.fields.severity')} required>
                  <FormSelect
                    value={form.severity}
                    onChange={(v) => set('severity', v as Severity)}
                    options={SEVERITY_OPTIONS}
                  />
                </FormField>
                <FormField label={t('anomaly.fields.service')} hint={t('anomaly.hints.service')}>
                  <FormInput
                    value={form.service}
                    onChange={(e) => set('service', e.target.value)}
                    placeholder={t('anomaly.placeholders.service')}
                  />
                </FormField>
              </FormRow>
            </FormSection>

            <FormSection
              title={t('anomaly.sections.detection')}
              description={t('anomaly.detection_description')}
            >
              <FormRow>
                <FormField label={t('anomaly.fields.stream_name')} required hint={t('anomaly.hints.stream_name')}>
                  <FormInput
                    value={form.streamName}
                    onChange={(e) => set('streamName', e.target.value)}
                    placeholder={t('anomaly.placeholders.stream_name')}
                    required
                  />
                </FormField>
                <FormField label={t('anomaly.fields.stream_type')} required>
                  <FormSelect
                    value={form.streamType}
                    onChange={(v) => set('streamType', v as StreamType)}
                    options={STREAM_TYPES}
                  />
                </FormField>
              </FormRow>
              <FormRow>
                <FormField label={t('anomaly.fields.language')} required>
                  <FormSelect
                    value={form.language}
                    onChange={(v) => set('language', v as 'sql' | 'promql')}
                    options={[
                      { value: 'sql', label: 'SQL' },
                      { value: 'promql', label: 'PromQL' },
                    ]}
                  />
                </FormField>
                <FormField label={t('anomaly.fields.window')} required hint={t('anomaly.hints.window')}>
                  <FormSelect
                    value={form.periodSecs}
                    onChange={(v) => set('periodSecs', v)}
                    options={WINDOW_OPTIONS}
                  />
                </FormField>
              </FormRow>
              <FormField label={t('anomaly.fields.query')} required hint={t('anomaly.hints.query')}>
                <CodeEditor
                  value={form.statement}
                  onChange={(v) => set('statement', v)}
                  language={form.language === 'promql' ? 'promql' : 'sql'}
                  label={form.language === 'promql' ? 'PromQL' : 'SQL'}
                  ariaLabel={t('anomaly.fields.query')}
                  minHeight={160}
                  maxHeight={320}
                  fontSize={12}
                  lineHeight={18}
                  compact
                  resizable
                  showHeader={false}
                  readOnly={manageAccess.disabled}
                />
              </FormField>
            </FormSection>

            <FormSection
              title={t('anomaly.sections.detector')}
              description={t('anomaly.detector_description')}
            >
              <FormRow>
                <FormField label={t('anomaly.fields.algorithm')} required hint={t('anomaly.hints.algorithm')}>
                  <FormSelect
                    value={form.algorithm}
                    onChange={(v) => set('algorithm', v)}
                    options={ALGORITHMS.map((a) => ({ value: a, label: a.toUpperCase() }))}
                  />
                </FormField>
                <FormField label={t('anomaly.fields.lookback_days')} required hint={t('anomaly.hints.lookback_days')}>
                  <FormInput
                    type="number"
                    min={form.weeklySeasonality ? 7 : 1}
                    max={30}
                    value={form.lookbackDays}
                    onChange={(e) => set('lookbackDays', e.target.value)}
                  />
                </FormField>
              </FormRow>
              <FormRow>
                <FormField
                  label={t('anomaly.fields.weekly_seasonality')}
                  hint={t('anomaly.hints.weekly_seasonality')}
                >
                  <FormSelect
                    value={form.weeklySeasonality ? 'on' : 'off'}
                    onChange={(v) => set('weeklySeasonality', v === 'on')}
                    options={[
                      { value: 'off', label: t('anomaly.weekly.off') },
                      { value: 'on', label: t('anomaly.weekly.on') },
                    ]}
                  />
                </FormField>
              </FormRow>
              <FormRow>
                <FormField label={t('anomaly.fields.k')} required hint={t('anomaly.hints.k')}>
                  <FormInput
                    type="number"
                    min={0.1}
                    step={0.1}
                    value={form.k}
                    onChange={(e) => set('k', e.target.value)}
                  />
                </FormField>
                <FormField label={t('anomaly.fields.for_periods')} required hint={t('anomaly.hints.for_periods')}>
                  <FormInput
                    type="number"
                    min={1}
                    value={form.forPeriods}
                    onChange={(e) => set('forPeriods', e.target.value)}
                  />
                </FormField>
              </FormRow>
              {form.algorithm === 'ewma' && (
                <FormRow>
                  <FormField label={t('anomaly.fields.alpha')} required hint={t('anomaly.hints.alpha')}>
                    <FormInput
                      type="number"
                      min={0.01}
                      max={1}
                      step={0.05}
                      value={form.alpha}
                      onChange={(e) => set('alpha', e.target.value)}
                    />
                  </FormField>
                </FormRow>
              )}
            </FormSection>
            </fieldset>

            <div className="mt-4 flex items-center gap-2">
              <ChromeButton
                type="submit"
                variant="primary"
                disabled={save.isPending || manageAccess.disabled}
                disabledReason={!save.isPending ? manageAccess.reason : undefined}
              >
                {save.isPending
                  ? t('anomaly.actions.saving')
                  : isEdit
                    ? t('anomaly.actions.save')
                    : t('anomaly.actions.create')}
              </ChromeButton>
              <ChromeButton type="button" onClick={() => nav('/alerts/rules')}>
                {tc('actions.cancel')}
              </ChromeButton>
            </div>
          </form>
        )}
      </PageBody>
    </>
  );
}

function clampInt(raw: string, fallback: number, min: number, max: number): number {
  if (raw.trim() === '') return fallback;
  const n = Math.round(Number(raw));
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}

function clampFloat(raw: string, fallback: number, min: number, max: number): number {
  if (raw.trim() === '') return fallback;
  const n = Number(raw);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}
