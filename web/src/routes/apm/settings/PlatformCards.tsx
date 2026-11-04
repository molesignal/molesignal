import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Filter, Layers } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { type ApmLimits, apmSettingsApi, apmSettingsQueryKeys } from '@/api/apmSettings';
import { type SamplingRule, tracePolicyApi, tracePolicyQueryKeys, type TraceRuntimePolicy } from '@/api/tracePolicy';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';

import { limitIssues, parseNumber } from './form';
import { CardSave, FIELD, Issues, ListRows, NumberField, useDraft } from './parts';
import { ApmCard, ApmQueryState } from '../frame/Page';

const LIMIT_KEYS = [
  'servicesPerOrgHour',
  'transactionsPerServiceHour',
  'dependenciesPerServiceHour',
  'operationsPerDependencyHour',
  'errorGroupsPerServiceHour',
  'versionsPerServiceHour',
  'instancesPerServiceHour',
] as const satisfies ReadonlyArray<keyof ApmLimits>;

type TypedLimits = Record<keyof ApmLimits, string>;

/**
 * How many of each kind APM keeps apart an hour, past which the rest are
 * counted together as "other". For the whole platform: set by whoever
 * manages its settings, and in force on every node within half a minute.
 */
export function LimitsCard() {
  const { t } = useTranslation('apm');
  const client = useQueryClient();
  const access = useActionAccess({ permission: 'sys.settings.limits.manage' });
  const query = useQuery({ queryKey: apmSettingsQueryKeys.limits(), queryFn: () => apmSettingsApi.limits() });
  const saved = Object.fromEntries(LIMIT_KEYS.map((key) => [key, String(query.data?.[key] ?? '')])) as TypedLimits;
  const draft = useDraft(saved);
  const limits = Object.fromEntries(LIMIT_KEYS.map((key) => [key, parseNumber(draft.value[key]) ?? Number.NaN])) as unknown as ApmLimits;
  const issues = limitIssues(limits);
  const save = useMutation({
    mutationFn: () => apmSettingsApi.saveLimits(limits),
    onSuccess: (next) => {
      client.setQueryData(apmSettingsQueryKeys.limits(), next);
      draft.reset();
      toast.success(t('org_settings.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <ApmCard
      icon={Layers}
      tone="text-[var(--chart-3)]"
      title={t('org_settings.limits.title')}
      description={t('org_settings.limits.hint')}
      testId="settings-limits"
      actions={<CardSave name="limits" dirty={draft.dirty} invalid={issues.length > 0} pending={save.isPending} access={access} onSave={() => save.mutate()} onReset={draft.reset} />}
    >
      <ApmQueryState query={query} empty={false} compact>
        <div className="grid gap-x-8 gap-y-2 p-3 sm:grid-cols-2 xl:grid-cols-3">
          {LIMIT_KEYS.map((key) => (
            <NumberField
              key={key}
              label={t(`org_settings.limits.${key}`)}
              value={draft.value[key]}
              onChange={(text) => draft.set({ ...draft.value, [key]: text })}
              invalid={issues.includes(key)}
              disabled={access.disabled}
              width="w-24"
              className="flex"
            />
          ))}
        </div>
        <Issues issues={issues.length > 0 ? [t('org_settings.limits.issue')] : []} />
      </ApmQueryState>
    </ApmCard>
  );
}

type RuleKind = 'keep' | 'drop' | 'ratio';

/** A rule as typed: every field text, the action apart from its ratio. */
interface TypedRule {
  name: string;
  service_name: string;
  route: string;
  operation: string;
  minimum_duration_ms: string;
  require_error: boolean;
  kind: RuleKind;
  ratio: string;
}

interface TypedPolicy {
  ratio: string;
  rules: TypedRule[];
}

function typedPolicy(policy: TraceRuntimePolicy | undefined): TypedPolicy {
  return {
    ratio: policy ? String(Math.round(policy.normalSampleRatio * 10_000) / 100) : '',
    rules: (policy?.rules ?? []).map((rule) => ({
      name: rule.name,
      service_name: rule.serviceName ?? '',
      route: rule.route ?? '',
      operation: rule.operation ?? '',
      minimum_duration_ms: rule.minimumDurationMs ? String(rule.minimumDurationMs) : '',
      require_error: rule.requireError,
      kind: rule.action.type,
      ratio: rule.action.type === 'ratio' ? String(Math.round(rule.action.ratio * 10_000) / 100) : '',
    })),
  };
}

/** The policy typed over the one in force; null while a field cannot be read. */
function readPolicy(typed: TypedPolicy, policy: TraceRuntimePolicy): TraceRuntimePolicy | null {
  const ratio = parseNumber(typed.ratio);

  if (ratio === null || ratio < 0 || ratio > 100) return null;

  const rules: SamplingRule[] = [];

  for (const rule of typed.rules) {
    const duration = rule.minimum_duration_ms.trim() ? parseNumber(rule.minimum_duration_ms) : null;
    const share = rule.kind === 'ratio' ? parseNumber(rule.ratio) : 0;

    if (!rule.name.trim() || (rule.minimum_duration_ms.trim() && (duration === null || duration < 1 || !Number.isInteger(duration)))) return null;
    if (share === null || share < 0 || share > 100) return null;

    const text = (value: string) => value.trim() || null;
    rules.push({
      name: rule.name.trim(),
      serviceName: text(rule.service_name),
      route: text(rule.route),
      operation: text(rule.operation),
      minimumDurationMs: duration,
      requireError: rule.require_error,
      action: rule.kind === 'ratio' ? { type: 'ratio', ratio: share / 100 } : { type: rule.kind },
    });
  }

  return { ...policy, normalSampleRatio: ratio / 100, rules };
}

/**
 * Which traces the platform keeps once they end: the share kept of those no
 * rule, error or slowness keeps, and the rules tried before it. Saving
 * publishes a new version of the policy to every node.
 */
export function SamplingCard() {
  const { t } = useTranslation('apm');
  const client = useQueryClient();
  const access = useActionAccess({ permission: 'sys.telemetry.manage' });
  const query = useQuery({ queryKey: tracePolicyQueryKeys.telemetry(), queryFn: () => tracePolicyApi.get() });
  const policy = query.data?.policy;
  const draft = useDraft(typedPolicy(policy));
  const next = policy ? readPolicy(draft.value, policy) : null;
  const save = useMutation({
    mutationFn: (value: TraceRuntimePolicy) => tracePolicyApi.save(value),
    onSuccess: (telemetry) => {
      client.setQueryData(tracePolicyQueryKeys.telemetry(), telemetry);
      draft.reset();
      toast.success(t('org_settings.sampling.published', { version: telemetry.policy.version }));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const kinds = (['keep', 'drop', 'ratio'] as const).map((kind) => ({ value: kind, label: t(`org_settings.sampling.actions.${kind}`) }));
  const disabled = access.disabled;

  return (
    <ApmCard
      icon={Filter}
      tone="text-[var(--chart-4)]"
      title={t('org_settings.sampling.title')}
      description={policy && t('org_settings.sampling.version', { version: policy.version })}
      testId="settings-sampling"
      actions={<CardSave name="sampling" dirty={draft.dirty} invalid={!next} pending={save.isPending} access={access} onSave={() => next && save.mutate(next)} onReset={draft.reset} />}
    >
      <ApmQueryState query={query} empty={false} compact>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-bd-0 p-3 font-sans text-xs">
          <NumberField label={t('org_settings.sampling.ratio')} unit="%" value={draft.value.ratio} onChange={(ratio) => draft.set({ ...draft.value, ratio })} disabled={disabled} />
          <span className="text-tx-3">{t('org_settings.sampling.ratio_hint')}</span>
          {query.data && !query.data.effectiveEnabled && <span className="font-strong text-[var(--orange-ink)]">{t('org_settings.sampling.off')}</span>}
        </div>
        <div className="overflow-x-auto">
          <div className="min-w-[860px]">
            <ListRows
              name="sampling"
              rows={draft.value.rules}
              onChange={(rules) => draft.set({ ...draft.value, rules })}
              blank={(): TypedRule => ({ name: '', service_name: '', route: '', operation: '', minimum_duration_ms: '', require_error: false, kind: 'keep', ratio: '' })}
              addLabel={t('org_settings.sampling.add')}
              empty={t('org_settings.sampling.empty')}
              disabled={disabled}
              head={
                <>
                  <span className="min-w-0 flex-[2] truncate">{t('org_settings.sampling.name')}</span>
                  <span className="min-w-0 flex-[2] truncate">{t('org_settings.sampling.service')}</span>
                  <span className="min-w-0 flex-[2] truncate">{t('org_settings.sampling.route')}</span>
                  <span className="min-w-0 flex-[2] truncate">{t('org_settings.sampling.operation')}</span>
                  <span className="w-28 shrink-0 truncate text-right">{t('org_settings.sampling.minimum')}</span>
                  <span className="w-[96px] shrink-0" />
                  <span className="w-[176px] shrink-0 truncate">{t('org_settings.sampling.action')}</span>
                </>
              }
              render={(rule, change) => (
                <>
                  <input aria-label={t('org_settings.sampling.name')} placeholder={t('org_settings.sampling.name')} value={rule.name} disabled={disabled} onChange={(event) => change({ ...rule, name: event.target.value })} className={cn(FIELD, 'flex-[2]')} />
                  <input aria-label={t('org_settings.sampling.service')} placeholder={t('org_settings.sampling.service')} value={rule.service_name} disabled={disabled} onChange={(event) => change({ ...rule, service_name: event.target.value })} className={cn(FIELD, 'flex-[2]')} />
                  <input aria-label={t('org_settings.sampling.route')} placeholder={t('org_settings.sampling.route')} value={rule.route} disabled={disabled} onChange={(event) => change({ ...rule, route: event.target.value })} className={cn(FIELD, 'flex-[2]')} />
                  <input aria-label={t('org_settings.sampling.operation')} placeholder={t('org_settings.sampling.operation')} value={rule.operation} disabled={disabled} onChange={(event) => change({ ...rule, operation: event.target.value })} className={cn(FIELD, 'flex-[2]')} />
                  <input
                    aria-label={t('org_settings.sampling.minimum')}
                    placeholder={t('org_settings.sampling.minimum')}
                    inputMode="numeric"
                    value={rule.minimum_duration_ms}
                    disabled={disabled}
                    onChange={(event) => change({ ...rule, minimum_duration_ms: event.target.value })}
                    className={cn(FIELD, 'w-28 shrink-0 text-right tabular-nums')}
                  />
                  <label className="inline-flex w-[96px] shrink-0 items-center gap-1.5 whitespace-nowrap font-sans text-xs text-tx-2">
                    <Checkbox checked={rule.require_error} disabled={disabled} onCheckedChange={(checked) => change({ ...rule, require_error: checked === true })} />
                    {t('org_settings.sampling.errors_only')}
                  </label>
                  <span className="flex w-[176px] shrink-0 items-center gap-2">
                    <FittedSelect
                      aria-label={t('org_settings.sampling.action')}
                      value={rule.kind}
                      onValueChange={(kind) => change({ ...rule, kind: kind as RuleKind })}
                      options={kinds}
                      maxWidth={120}
                      disabled={disabled}
                      className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
                    />
                    {rule.kind === 'ratio' && (
                      <input
                        aria-label={t('org_settings.sampling.rule_ratio')}
                        inputMode="decimal"
                        value={rule.ratio}
                        disabled={disabled}
                        onChange={(event) => change({ ...rule, ratio: event.target.value })}
                        className={cn(FIELD, 'w-14 text-right tabular-nums')}
                      />
                    )}
                  </span>
                </>
              )}
            />
          </div>
        </div>
        <Issues issues={policy && !next ? [t('org_settings.sampling.issue')] : []} />
      </ApmQueryState>
    </ApmCard>
  );
}
