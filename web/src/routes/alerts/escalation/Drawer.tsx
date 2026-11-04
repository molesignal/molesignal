import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as escalationsApi from '@/api/escalations';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { windowsInvalid } from '@/shell/weekly-ranges/WeeklyWindowsFields';
import type { EscalationPolicy, EscalationStep, EscalationTarget, Severity, UrgencyRule } from '@/types/alerting';

import {
  ACK_TIMEOUTS,
  emptyStep,
  emptyTarget,
  secsLabel,
  targetRefId,
  type TargetNames,
  withRefId,
} from './steps';
import { UrgencyFields } from './UrgencyFields';

const SEVERITIES: Severity[] = ['info', 'warning', 'error', 'critical'];

interface RefOptions {
  users: Array<{ value: string; label: string }>;
  schedules: Array<{ value: string; label: string }>;
  teams: Array<{ value: string; label: string }>;
  /** What to call a target its list does not hold: the list is still loading, or leaves it out. */
  unlisted: TargetNames;
}

/** The form of one escalation policy: its steps, who each tells, and how long each waits. */
export function EscalationDrawer({
  open,
  editing,
  onClose,
}: {
  open: boolean;
  editing: EscalationPolicy | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.escalations.manage' });
  const [name, setName] = React.useState('');
  const [repeat, setRepeat] = React.useState(false);
  const [maxLoops, setMaxLoops] = React.useState(1);
  const [ackRetrigger, setAckRetrigger] = React.useState('');
  const [steps, setSteps] = React.useState<EscalationStep[]>(() => [emptyStep()]);
  const [urgencyRule, setUrgencyRule] = React.useState<UrgencyRule>({ mode: 'by_severity' });
  const [escalateLow, setEscalateLow] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setRepeat(editing?.repeat ?? false);
    setMaxLoops(editing?.maxLoops ?? 1);
    setAckRetrigger(editing?.ackRetriggerSecs ? String(editing.ackRetriggerSecs) : '');
    setSteps(editing?.steps?.length ? editing.steps.map((s) => ({ ...s })) : [emptyStep()]);
    setUrgencyRule(editing?.urgencyRule ?? { mode: 'by_severity' });
    setEscalateLow(editing?.escalateLowUrgency ?? false);
  }, [open, editing]);

  const users = useUsers();
  const schedulesQuery = useQuery({ queryKey: ['schedules'], queryFn: () => schedulesApi.list(), enabled: open });
  const teamsQuery = useQuery({ queryKey: ['teams'], queryFn: () => teamsApi.list(), enabled: open });

  const refs: RefOptions = React.useMemo(
    () => ({
      users: users.users.map((u) => ({ value: u.id, label: u.name })),
      schedules: (schedulesQuery.data ?? []).map((s) => ({ value: s.id, label: s.name })),
      teams: (teamsQuery.data ?? []).map((tm) => ({ value: tm.id, label: tm.name })),
      unlisted: {
        user: users.nameOf,
        schedule: () => (schedulesQuery.isPending ? '…' : t('escalations.missing_target')),
        team: () => (teamsQuery.isPending ? '…' : t('escalations.missing_target')),
      },
    }),
    [
      users.users,
      users.nameOf,
      schedulesQuery.data,
      schedulesQuery.isPending,
      teamsQuery.data,
      teamsQuery.isPending,
      t,
    ],
  );

  const save = useMutation({
    mutationFn: () => {
      const cleanSteps: EscalationStep[] = steps
        .map((s) => ({
          targets: s.targets.filter((tg) => targetRefId(tg).trim() !== ''),
          ackTimeoutSecs: Math.max(0, Math.floor(s.ackTimeoutSecs) || 0),
          minSeverity: s.minSeverity ?? null,
        }))
        .filter((s) => s.targets.length > 0);
      const payload: escalationsApi.EscalationPolicyInput = {
        name: name.trim(),
        steps: cleanSteps,
        repeat,
        maxLoops: repeat ? Math.max(1, Math.floor(maxLoops) || 1) : 1,
        ackRetriggerSecs: ackRetrigger ? Number(ackRetrigger) : null,
        urgencyRule: urgencyRule,
        escalateLowUrgency: escalateLow,
      };
      return editing ? escalationsApi.update(editing.id, payload) : escalationsApi.create(payload);
    },
    onSuccess: () => {
      toast.success(editing ? t('escalations.toast_updated', { defaultValue: 'Policy updated' }) : t('escalations.toast_created', { defaultValue: 'Policy created' }));
      void qc.invalidateQueries({ queryKey: ['escalation-policies'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!manageAccess.allowed) return;
    if (!name.trim()) {
      toast.error(t('escalations.errors.name_required', { defaultValue: 'Name is required.' }));
      return;
    }
    if (steps.every((s) => s.targets.every((tg) => targetRefId(tg).trim() === ''))) {
      toast.error(t('escalations.errors.target_required', { defaultValue: 'Add at least one target.' }));
      return;
    }
    if (urgencyRule.mode === 'support_hours' && windowsInvalid(urgencyRule.hours)) {
      toast.error(t('escalations.errors.support_hours'));
      return;
    }
    save.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={editing ? t('escalations.edit_title', { defaultValue: 'Edit escalation policy' }) : t('escalations.drawer_title', { defaultValue: 'New escalation policy' })}
      subtitle={t('escalations.drawer_subtitle', { defaultValue: 'Steps fire in order until the incident is acknowledged.' }) as string}
      width={820}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={!name.trim()}
          onCancel={onClose}
          submitLabel={editing ? t('escalations.save', { defaultValue: 'Save policy' }) : t('escalations.create', { defaultValue: 'Create policy' })}
          formId="escalation-form"
        />
      }
    >
      <form id="escalation-form" onSubmit={submit}>
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
        <FormSection title={t('escalations.sections.identity', { defaultValue: 'Identity' })}>
          <FormField label={t('escalations.fields.name', { defaultValue: 'Policy name' })} required>
            <FormInput value={name} onChange={(e) => setName(e.target.value)} placeholder="on_call_default" required />
          </FormField>
          <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
            <Checkbox checked={repeat} onCheckedChange={(next) => setRepeat(next === true)} />
            <span>{t('escalations.fields.repeat', { defaultValue: 'Repeat the whole policy until acknowledged' })}</span>
          </label>
          {repeat && (
            <FormField
              label={t('escalations.fields.max_loops', { defaultValue: 'Max loops' })}
              hint={t('escalations.hints.max_loops', { defaultValue: 'How many times to repeat before giving up.' })}
            >
              <FormInput
                type="number"
                min={1}
                value={String(maxLoops)}
                onChange={(e) => setMaxLoops(Math.max(1, Math.floor(Number(e.target.value)) || 1))}
                className="w-28"
              />
            </FormField>
          )}
          <FormField
            label={t('escalations.fields.ack_retrigger')}
            hint={t('escalations.hints.ack_retrigger')}
          >
            <FormSelect
              value={ackRetrigger}
              onChange={setAckRetrigger}
              ariaLabel={t('escalations.fields.ack_retrigger')}
              options={ackRetriggerOptions(t, ackRetrigger)}
            />
          </FormField>
        </FormSection>

        <FormSection
          title={t('escalations.sections.steps', { defaultValue: 'Escalation steps' })}
          description={t('escalations.sections.steps_description', { defaultValue: 'Each step emits a Notify event for its targets. Notify Policies select connectors, preferences and fallback routes.' }) as string}
        >
          <FieldArray<EscalationStep>
            items={steps}
            onChange={setSteps}
            minItems={1}
            addLabel={t('escalations.add_step', { defaultValue: 'Add step' })}
            removeLabel={t('escalations.remove_step', { defaultValue: 'Remove step' })}
            newItem={emptyStep}
            renderItem={(step, index, setStep) => (
              <StepFields step={step} index={index} setStep={setStep} refs={refs} />
            )}
          />
          {/* True of every target alike, so it is said once and not under each of them. */}
          <p className="mt-3 font-sans text-xs text-tx-2">{t('escalations.fields.notify_routing')}</p>
        </FormSection>

        <UrgencyFields
          rule={urgencyRule}
          onRule={setUrgencyRule}
          escalateLow={escalateLow}
          onEscalateLow={setEscalateLow}
        />
        </fieldset>
      </form>
    </FormDrawer>
  );
}

function StepFields({
  step,
  index,
  setStep,
  refs,
}: {
  step: EscalationStep;
  index: number;
  setStep: (next: EscalationStep) => void;
  refs: RefOptions;
}) {
  const { t } = useTranslation('alerts');
  const severityOptions = React.useMemo(
    () => [
      { value: '', label: t('escalations.any_severity', { defaultValue: 'Any severity' }) },
      ...SEVERITIES.map((s) => ({ value: s, label: t(`severity.${s}`, { defaultValue: s }) })),
    ],
    [t],
  );
  const ackOptions = React.useMemo(
    () => ACK_TIMEOUTS.map((s) => ({ value: String(s), label: secsLabel(s) })),
    [],
  );

  return (
    <div className="flex flex-col gap-3 border-b border-bd-0 pb-4">
      <div className="flex items-center gap-2">
        <span className="grid h-5 w-5 shrink-0 place-items-center rounded-full bg-indigo/15 font-mono text-xs font-strong text-indigo-soft">
          {index + 1}
        </span>
        <span className="font-sans text-xs font-strong text-tx-1">
          {t('escalations.step_n', { defaultValue: 'Step {{n}}', n: index + 1 })}
        </span>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <FormField label={t('escalations.fields.min_severity', { defaultValue: 'Applies from severity' })}>
          <FormSelect
            value={step.minSeverity ?? ''}
            onChange={(v) => setStep({ ...step, minSeverity: v ? (v as Severity) : null })}
            options={severityOptions}
          />
        </FormField>
        <FormField label={t('escalations.fields.ack_timeout', { defaultValue: 'Ack timeout' })}>
          <FormSelect
            value={String(step.ackTimeoutSecs)}
            onChange={(v) => setStep({ ...step, ackTimeoutSecs: Number(v) })}
            options={ackOptions}
          />
        </FormField>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="font-sans text-xs font-strong tracking-normal text-tx-3">
          {t('escalations.fields.targets', { defaultValue: 'Targets' })}
        </span>
        <FieldArray<EscalationTarget>
          items={step.targets}
          onChange={(next) => setStep({ ...step, targets: next })}
          minItems={1}
          addLabel={t('escalations.add_target', { defaultValue: 'Add target' })}
          removeLabel={t('escalations.remove_target', { defaultValue: 'Remove target' })}
          newItem={() => emptyTarget('user')}
          renderItem={(target, _i, setTarget) => (
            <TargetFields target={target} setTarget={setTarget} refs={refs} />
          )}
        />
      </div>
    </div>
  );
}

function TargetFields({
  target,
  setTarget,
  refs,
}: {
  target: EscalationTarget;
  setTarget: (next: EscalationTarget) => void;
  refs: RefOptions;
}) {
  const { t } = useTranslation('alerts');
  const kindOptions = React.useMemo(
    () => [
      { value: 'user', label: t('escalations.target_kinds.user', { defaultValue: 'User' }) },
      { value: 'schedule', label: t('escalations.target_kinds.schedule', { defaultValue: 'Schedule' }) },
      { value: 'team', label: t('escalations.target_kinds.team', { defaultValue: 'Team' }) },
    ],
    [t],
  );
  const refOptions =
    target.kind === 'user'
      ? refs.users
      : target.kind === 'schedule'
        ? refs.schedules
        : refs.teams;
  const refId = targetRefId(target);
  const unlisted = refId !== '' && !refOptions.some((option) => option.value === refId);

  return (
    <div className="flex items-center gap-2">
        <div className="w-28 shrink-0">
          <FormSelect
            value={target.kind}
            onChange={(v) => setTarget(emptyTarget(v as EscalationTarget['kind']))}
            options={kindOptions}
          />
        </div>
        <FormSelect
          value={refId}
          onChange={(v) => setTarget(withRefId(target, v))}
          options={[
            { value: '', label: t('escalations.pick_ref', { defaultValue: '— select —' }) },
            ...refOptions,
            // What the step points at stays a choice, so that saving does not drop it.
            ...(unlisted ? [{ value: refId, label: refs.unlisted[target.kind](refId) }] : []),
          ]}
          className="flex-1"
        />
    </div>
  );
}

/** The choices of how long an acknowledged incident may stay unresolved; a wait set through the
 * API that no preset matches is listed too. */
function ackRetriggerOptions(
  t: TFunction<'alerts'>,
  current: string,
): Array<{ value: string; label: string }> {
  const presets = [1800, 3600, 7200, 14400];
  const secs = Number(current);
  const values = current && !presets.includes(secs) ? [...presets, secs].sort((a, b) => a - b) : presets;

  return [
    { value: '', label: t('escalations.ack_retrigger.off') },
    ...values.map((value) => ({
      value: String(value),
      label:
        value % 3600 === 0
          ? t('escalations.ack_retrigger.hours', { count: value / 3600 })
          : t('escalations.ack_retrigger.minutes', { count: Math.round(value / 60) }),
    })),
  ];
}
