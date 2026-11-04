import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CircleAlert, Plus, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import type { HeartbeatAddress } from '@/api/alerts';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton, IconButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
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
import type {
  AlertRule,
  ComparisonOp,
  CompositeQuery,
  ForecastParams,
  HeartbeatParams,
  OutlierParams,
  Severity,
} from '@/types/alerting';

import {
  FORECAST_LEAD_SECS,
  FORECAST_LOOKBACK_DAYS,
  MAX_COMPOSITE_RULES,
  WORKBENCH_KINDS,
  type WorkbenchKind,
  compositeProblem,
  heartbeatExamples,
  nextLetter,
} from './kinds';

const SEVERITIES: Severity[] = ['critical', 'error', 'warning', 'info'];
const OPERATORS: ComparisonOp[] = ['gt', 'gte', 'lt', 'lte'];

/** How the rule fires: against a threshold, a forecast, the other groups, other rules or check-ins. */
export function KindPicker({ value, onChange }: { value: WorkbenchKind; onChange: (kind: WorkbenchKind) => void }) {
  const { t } = useTranslation('alerts');

  return (
    <div data-rule-kind>
      <div className={uiLabelClass}>{t('workbench.kind.label')}</div>
      <div role="radiogroup" aria-label={t('workbench.kind.label')} className="mt-2 flex flex-wrap gap-1 bg-bg-2 p-0.5">
        {WORKBENCH_KINDS.map((kind) => (
          <button
            key={kind}
            type="button"
            role="radio"
            aria-checked={value === kind}
            onClick={() => onChange(kind)}
            className={cn(
              'rounded-[5px] px-3 py-1.5 font-sans text-xs font-strong transition-colors',
              value === kind ? 'bg-bg-0 text-indigo-soft' : 'text-tx-2 hover:text-tx-0',
            )}
          >
            {t(`workbench.kind.options.${kind}`)}
          </button>
        ))}
      </div>
      <p className="mt-1.5 text-xs text-tx-3">{t(`workbench.kind.hints.${value}`)}</p>
    </div>
  );
}

/** The severity a rule that judges itself fires at, and how long it has to hold first. */
export function JudgedSeverity({
  severity,
  minutes,
  showMinutes,
  onSeverity,
  onMinutes,
}: {
  severity: Severity;
  minutes: number;
  showMinutes: boolean;
  onSeverity: (severity: Severity) => void;
  onMinutes: (minutes: number) => void;
}) {
  const { t } = useTranslation('alerts');

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <FormField label={t('workbench.threshold.severity')}>
        <FormSelect
          value={severity}
          onChange={(value) => onSeverity(value as Severity)}
          options={SEVERITIES.map((item) => ({ value: item, label: t(`history.severity.${item}`) }))}
        />
      </FormField>
      {showMinutes && (
        <FormField label={t('workbench.threshold.duration')}>
          <div className="flex items-center gap-2">
            <FormInput
              type="number"
              min={1}
              value={String(minutes)}
              onChange={(event) => onMinutes(Math.max(1, Math.floor(Number(event.target.value) || 1)))}
            />
            <span className="shrink-0 text-xs text-tx-2">{t('workbench.threshold.minutes')}</span>
          </div>
        </FormField>
      )}
    </div>
  );
}

/** Other rules named A to Z and how they combine. */
export function CompositeFields({
  value,
  ownId,
  onChange,
}: {
  value: CompositeQuery;
  ownId: string | undefined;
  onChange: (composite: CompositeQuery) => void;
}) {
  const { t } = useTranslation('alerts');
  const rules = useQuery({ queryKey: ['alerts', 'rules'], queryFn: alertsApi.list, staleTime: 30_000 });
  const candidates = (rules.data ?? []).filter((rule) => rule.id !== ownId && rule.kind !== 'composite');
  const problem = compositeProblem(value);
  const letters = Object.keys(value.rules).sort();
  const free = nextLetter(value.rules);

  const setRule = (letter: string, id: string) => onChange({ ...value, rules: { ...value.rules, [letter]: id } });
  const removeRule = (letter: string) => {
    const rest = { ...value.rules };
    delete rest[letter];
    onChange({ ...value, rules: rest });
  };

  return (
    <div data-composite className="flex flex-col gap-4">
      <FormField label={t('workbench.composite.expression')} hint={t('workbench.composite.expression_hint')} required>
        <FormInput
          value={value.expression}
          onChange={(event) => onChange({ ...value, expression: event.target.value })}
          placeholder="A && (B || !C)"
          className="font-mono"
        />
      </FormField>
      <div>
        <div className={uiLabelClass}>{t('workbench.composite.rules')}</div>
        <ul className="mt-2 flex flex-col gap-2">
          {letters.map((letter) => {
            const missing = value.rules[letter] && !candidates.some((rule) => rule.id === value.rules[letter]) && rules.isSuccess;
            return (
              <li key={letter} data-composite-rule={letter} className="flex min-w-0 items-center gap-2">
                <span className="w-6 shrink-0 text-center font-mono text-sm font-strong text-tx-0">{letter}</span>
                <div className="min-w-0 flex-1">
                  <FormSelect
                    value={value.rules[letter] ?? ''}
                    onChange={(id) => setRule(letter, id)}
                    ariaLabel={t('workbench.composite.rule_named', { letter })}
                    options={[
                      { value: '', label: t('workbench.composite.pick_rule') },
                      ...(missing ? [{ value: value.rules[letter]!, label: t('workbench.composite.missing_rule') }] : []),
                      ...candidates.map((rule: AlertRule) => ({ value: rule.id, label: rule.name })),
                    ]}
                  />
                </div>
                <IconButton
                  aria-label={t('workbench.composite.remove_rule', { letter })}
                  onClick={() => removeRule(letter)}
                  className="enabled:hover:text-red-soft"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </IconButton>
              </li>
            );
          })}
        </ul>
        {free && letters.length < MAX_COMPOSITE_RULES && (
          <ChromeButton size="sm" variant="ghost" className="mt-2" onClick={() => setRule(free, '')}>
            <Plus className="h-3.5 w-3.5" />
            {t('workbench.composite.add_rule', { letter: free })}
          </ChromeButton>
        )}
      </div>
      <label className="flex items-start justify-between gap-4 bg-bg-2 px-3 py-2.5">
        <span className="min-w-0">
          <span className="block text-sm text-tx-0">{t('workbench.composite.merge_children')}</span>
          <span className="mt-0.5 block text-xs text-tx-2">{t('workbench.composite.merge_children_hint')}</span>
        </span>
        <Switch
          checked={value.mergeChildren ?? false}
          onCheckedChange={(checked) => onChange({ ...value, mergeChildren: checked })}
          aria-label={t('workbench.composite.merge_children')}
        />
      </label>
      {problem && (
        <p data-composite-problem className="flex items-start gap-2 text-xs text-[var(--orange-ink)]">
          <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          {t(`workbench.composite.problems.${problem.kind}`, {
            letters: 'letters' in problem ? problem.letters.join(', ') : '',
          })}
        </p>
      )}
    </div>
  );
}

/** The line a forecast must not cross, how far back it looks and how far ahead. */
export function ForecastFields({
  value,
  operator,
  threshold,
  onChange,
  onLine,
}: {
  value: ForecastParams;
  operator: ComparisonOp;
  threshold: number;
  onChange: (forecast: ForecastParams) => void;
  onLine: (operator: ComparisonOp, threshold: number) => void;
}) {
  const { t } = useTranslation('alerts');
  const lead = (secs: number) =>
    secs < 86_400
      ? t('workbench.forecast.hours', { count: secs / 3_600 })
      : t('workbench.forecast.days', { count: secs / 86_400 });

  return (
    <div data-forecast className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-[160px_minmax(0,1fr)]">
        <FormField label={t('workbench.forecast.line')}>
          <FormSelect
            value={operator}
            onChange={(next) => onLine(next as ComparisonOp, threshold)}
            options={OPERATORS.map((item) => ({ value: item, label: t(`workbench.operators.${item}`) }))}
          />
        </FormField>
        <FormField label={t('workbench.threshold.value')} required>
          <FormInput
            type="number"
            step="any"
            value={Number.isFinite(threshold) ? String(threshold) : ''}
            onChange={(event) => onLine(operator, event.target.value === '' ? Number.NaN : Number(event.target.value))}
          />
        </FormField>
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <FormField label={t('workbench.forecast.lead')}>
          <FormSelect
            value={String(value.leadSecs)}
            onChange={(next) => onChange({ ...value, leadSecs: Number(next) })}
            options={FORECAST_LEAD_SECS.map((secs) => ({ value: String(secs), label: lead(secs) }))}
          />
        </FormField>
        <FormField label={t('workbench.forecast.lookback')}>
          <FormSelect
            value={String(value.lookbackDays)}
            onChange={(next) => {
              const days = Number(next);
              onChange({ ...value, lookbackDays: days, model: value.model === 'weekly' && days < 7 ? 'daily' : value.model });
            }}
            options={FORECAST_LOOKBACK_DAYS.map((days) => ({ value: String(days), label: t('workbench.forecast.days', { count: days }) }))}
          />
        </FormField>
        <FormField label={t('workbench.forecast.model')}>
          <FormSelect
            value={value.model}
            onChange={(next) => onChange({ ...value, model: next as ForecastParams['model'] })}
            options={(['linear', 'daily', 'weekly'] as const)
              .filter((model) => model !== 'weekly' || value.lookbackDays >= 7)
              .map((model) => ({ value: model, label: t(`workbench.forecast.models.${model}`) }))}
          />
        </FormField>
      </div>
      <p className="text-xs text-tx-3">{t('workbench.forecast.hint')}</p>
    </div>
  );
}

/** How far apart a group must stand from the others, and on which side. */
export function OutlierFields({ value, onChange }: { value: OutlierParams; onChange: (outlier: OutlierParams) => void }) {
  const { t } = useTranslation('alerts');

  return (
    <div data-outlier className="flex flex-col gap-2">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <FormField label={t('workbench.outlier.tolerance')} hint={t('workbench.outlier.tolerance_hint')}>
          <FormInput
            type="number"
            min={0.5}
            max={10}
            step={0.5}
            value={String(value.tolerance)}
            onChange={(event) => onChange({ ...value, tolerance: Number(event.target.value) })}
          />
        </FormField>
        <FormField label={t('workbench.outlier.direction')}>
          <FormSelect
            value={value.direction}
            onChange={(next) => onChange({ ...value, direction: next as OutlierParams['direction'] })}
            options={(['both', 'above', 'below'] as const).map((direction) => ({
              value: direction,
              label: t(`workbench.outlier.directions.${direction}`),
            }))}
          />
        </FormField>
      </div>
      <p className="text-xs text-tx-3">{t('workbench.outlier.needs_split')}</p>
    </div>
  );
}

/** How often a check-in is expected and how late it may be; a saved rule shows its last check-in. */
export function HeartbeatFields({
  value,
  ruleId,
  onChange,
  onAddress,
}: {
  value: HeartbeatParams;
  ruleId: string | undefined;
  onChange: (heartbeat: HeartbeatParams) => void;
  onAddress: (address: HeartbeatAddress) => void;
}) {
  const { t } = useTranslation('alerts');
  const minutes = (secs: number) => String(Math.round(secs / 60));

  return (
    <div data-heartbeat className="flex flex-col gap-4">
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <FormField label={t('workbench.heartbeat.period')} required>
          <div className="flex items-center gap-2">
            <FormInput
              type="number"
              min={1}
              value={minutes(value.periodSecs)}
              onChange={(event) => onChange({ ...value, periodSecs: Math.max(1, Number(event.target.value) || 1) * 60 })}
            />
            <span className="shrink-0 text-xs text-tx-2">{t('workbench.threshold.minutes')}</span>
          </div>
        </FormField>
        <FormField label={t('workbench.heartbeat.grace')}>
          <div className="flex items-center gap-2">
            <FormInput
              type="number"
              min={0}
              value={minutes(value.graceSecs)}
              onChange={(event) => onChange({ ...value, graceSecs: Math.max(0, Number(event.target.value) || 0) * 60 })}
            />
            <span className="shrink-0 text-xs text-tx-2">{t('workbench.threshold.minutes')}</span>
          </div>
        </FormField>
      </div>
      {ruleId ? (
        <HeartbeatStatus ruleId={ruleId} onAddress={onAddress} />
      ) : (
        <p className="text-xs text-tx-3">{t('workbench.heartbeat.address_after_save')}</p>
      )}
    </div>
  );
}

function HeartbeatStatus({ ruleId, onAddress }: { ruleId: string; onAddress: (address: HeartbeatAddress) => void }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const state = useQuery({
    queryKey: ['alerts', 'heartbeat', ruleId],
    queryFn: () => alertsApi.heartbeatState(ruleId),
    retry: false,
  });
  const reset = useMutation({
    mutationFn: () => alertsApi.resetHeartbeatAddress(ruleId),
    onSuccess: (address) => {
      onAddress(address);
      void queryClient.invalidateQueries({ queryKey: ['alerts', 'heartbeat', ruleId] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const beat = state.data;

  return (
    <div data-heartbeat-status className="flex flex-col gap-2 bg-bg-2 px-3 py-2.5 text-xs text-tx-2 sm:flex-row sm:items-center">
      <div className="min-w-0 flex-1">
        {beat ? (
          <>
            <span className="block text-sm text-tx-0">
              {beat.lastCheckinAt
                ? t('workbench.heartbeat.last_checkin', { value: formatMicrosActive(beat.lastCheckinAt, false) })
                : t('workbench.heartbeat.never')}
              {beat.lastCheckinAt && (
                <span className={cn('ml-2 text-xs font-strong', beat.lastStatus === 'fail' ? 'text-red' : 'text-green')}>
                  {t(`workbench.heartbeat.status.${beat.lastStatus}`)}
                </span>
              )}
            </span>
            <span className={cn('mt-0.5 block', beat.alarming && 'text-red')}>
              {t('workbench.heartbeat.due', { value: formatMicrosActive(beat.dueAt, false) })}
            </span>
          </>
        ) : (
          <span>{t('workbench.heartbeat.address_after_save')}</span>
        )}
      </div>
      <ChromeButton size="sm" disabled={reset.isPending} onClick={() => reset.mutate()}>
        {t('workbench.heartbeat.reset')}
      </ChromeButton>
    </div>
  );
}

/** A heartbeat rule's check-in address, the one time it is seen. */
export function HeartbeatAddressDialog({ address, onClose }: { address: HeartbeatAddress | null; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');

  if (!address) return null;

  const url = address.url ?? `${window.location.origin}${address.path}`;
  const examples = heartbeatExamples(url);

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="bg-[var(--functional-surface)]">
        <DialogHeader>
          <DialogTitle>{t('workbench.heartbeat.dialog_title')}</DialogTitle>
          <DialogDescription>{t('workbench.heartbeat.dialog_description')}</DialogDescription>
        </DialogHeader>
        <div data-heartbeat-address className="flex min-w-0 flex-col gap-3 text-xs text-tx-2">
          <CopyableValue value={url} copyLabel={t('workbench.heartbeat.copy')} copiedLabel={tc('actions.copied')} />
          <p>{t('workbench.heartbeat.shown_once')}</p>
          {!address.url && <p>{t('workbench.heartbeat.no_external_url')}</p>}
          <div className="flex min-w-0 flex-col gap-1">
            <span className="font-strong text-tx-1">{t('workbench.heartbeat.example')}</span>
            <code className="block whitespace-pre-wrap break-all bg-bg-2 px-3 py-2 font-mono text-xs text-tx-1">
              {`${examples.ok}\n${examples.fail}`}
            </code>
          </div>
        </div>
        <DialogFooter>
          <ChromeButton variant="primary" onClick={onClose}>
            {tc('actions.close')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
