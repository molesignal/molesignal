import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as escalationsApi from '@/api/escalations';
import * as schedulesApi from '@/api/schedules';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { queryStateFor } from '@/shell/query/State';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { EscalationPolicy, EscalationStep, EscalationTarget, Severity } from '@/types/alerting';

import { AlertsSubNav } from './Layout';

const SEVERITIES: Severity[] = ['info', 'warning', 'error', 'critical'];
const ACK_TIMEOUTS = [60, 300, 600, 900, 1800, 3600];

function secsLabel(secs: number): string {
  if (secs > 0 && secs % 3600 === 0) return `${secs / 3600}h`;
  if (secs > 0 && secs % 60 === 0) return `${secs / 60}m`;
  return `${secs}s`;
}

/** Read the referenced entity id regardless of target kind. */
function targetRefId(t: EscalationTarget): string {
  switch (t.kind) {
    case 'user':
      return t.user_id;
    case 'schedule':
      return t.schedule_id;
    case 'team':
      return t.team_id;
  }
}

/** Immutably set the referenced entity id, preserving kind. */
function withRefId(t: EscalationTarget, id: string): EscalationTarget {
  switch (t.kind) {
    case 'user':
      return { ...t, user_id: id };
    case 'schedule':
      return { ...t, schedule_id: id };
    case 'team':
      return { ...t, team_id: id };
  }
}

function emptyTarget(kind: EscalationTarget['kind']): EscalationTarget {
  switch (kind) {
    case 'user':
      return { kind: 'user', user_id: '' };
    case 'schedule':
      return { kind: 'schedule', schedule_id: '' };
    case 'team':
      return { kind: 'team', team_id: '' };
  }
}

function emptyStep(): EscalationStep {
  return { targets: [emptyTarget('user')], ack_timeout_secs: 300, min_severity: null };
}

export function AlertsEscalations() {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<EscalationPolicy | null>(null);
  const [removing, setRemoving] = React.useState<EscalationPolicy | null>(null);

  const q = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list() });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('escalations.empty_title', { defaultValue: 'No escalation policies' }),
    emptyDescription: t('escalations.empty_description', {
      defaultValue: 'Route unacknowledged incidents to on-call users, schedules and teams.',
    }),
  });

  const remove = useMutation({
    mutationFn: (id: string) => escalationsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['escalation-policies'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('escalations.title', { defaultValue: 'Escalation policies' })}
        subtitle={t('escalations.subtitle', {
          defaultValue: 'Multi-step routing for unacknowledged incidents',
        })}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setCreating(true)}
          >
            {t('escalations.new_policy', { defaultValue: 'New policy' })}
          </ChromeButton>
        }
      />
      <AlertsSubNav />
      <PageBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            onRowClick={(r) => setEditing(r)}
            isRowClickDisabled={() => manageAccess.disabled}
            rowClickDisabledReason={() => manageAccess.reason}
            columns={[
              { key: 'name', header: t('escalations.columns.name', { defaultValue: 'Name' }), cell: (r) => r.name },
              {
                key: 'steps',
                header: t('escalations.columns.steps', { defaultValue: 'Steps' }),
                cell: (r) => r.steps.length,
                width: 90,
              },
              {
                key: 'repeat',
                header: t('escalations.columns.repeat', { defaultValue: 'Repeat' }),
                cell: (r) =>
                  r.repeat ? (
                    <Pill tone="blue">{t('escalations.repeat_n', { defaultValue: '×{{n}}', n: r.max_loops })}</Pill>
                  ) : (
                    <Pill tone="dim">{tc('status.off')}</Pill>
                  ),
                width: 110,
              },
              {
                key: 'actions',
                header: t('escalations.columns.actions', { defaultValue: 'Actions' }),
                width: 176,
                className: 'text-right',
                headerClassName: 'text-right',
                cell: (r) => (
                  <div className="flex items-center justify-end gap-1">
                    <ChromeButton
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={(event) => {
                        event.stopPropagation();
                        setEditing(r);
                      }}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                      {tc('actions.edit')}
                    </ChromeButton>
                    <ChromeButton
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      className="text-tx-2 hover:border-red/40 hover:text-red-soft"
                      onClick={(event) => {
                        event.stopPropagation();
                        setRemoving(r);
                      }}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                      {tc('actions.delete')}
                    </ChromeButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </PageBody>
      <EscalationDrawer
        open={creating || editing !== null}
        editing={editing}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('escalations.delete_title', { defaultValue: 'Delete escalation policy' })}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}

interface RefOptions {
  users: Array<{ value: string; label: string }>;
  schedules: Array<{ value: string; label: string }>;
  teams: Array<{ value: string; label: string }>;
}

function EscalationDrawer({
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
  const manageAccess = useActionAccess({ permission: 'alerts.manage' });
  const [name, setName] = React.useState('');
  const [repeat, setRepeat] = React.useState(false);
  const [maxLoops, setMaxLoops] = React.useState(1);
  const [steps, setSteps] = React.useState<EscalationStep[]>(() => [emptyStep()]);

  React.useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? '');
    setRepeat(editing?.repeat ?? false);
    setMaxLoops(editing?.max_loops ?? 1);
    setSteps(editing?.steps?.length ? editing.steps.map((s) => ({ ...s })) : [emptyStep()]);
  }, [open, editing]);

  const users = useUsers();
  const schedulesQuery = useQuery({ queryKey: ['schedules'], queryFn: () => schedulesApi.list(), enabled: open });
  const teamsQuery = useQuery({ queryKey: ['teams'], queryFn: () => teamsApi.list(), enabled: open });

  const refs: RefOptions = React.useMemo(
    () => ({
      users: users.users.map((u) => ({ value: u.id, label: u.name })),
      schedules: (schedulesQuery.data ?? []).map((s) => ({ value: s.id, label: s.name })),
      teams: (teamsQuery.data ?? []).map((tm) => ({ value: tm.id, label: tm.name })),
    }),
    [users.users, schedulesQuery.data, teamsQuery.data],
  );

  const save = useMutation({
    mutationFn: () => {
      const cleanSteps: EscalationStep[] = steps
        .map((s) => ({
          targets: s.targets.filter((tg) => targetRefId(tg).trim() !== ''),
          ack_timeout_secs: Math.max(0, Math.floor(s.ack_timeout_secs) || 0),
          min_severity: s.min_severity ?? null,
        }))
        .filter((s) => s.targets.length > 0);
      const payload: escalationsApi.EscalationPolicyInput = {
        name: name.trim(),
        steps: cleanSteps,
        repeat,
        max_loops: repeat ? Math.max(1, Math.floor(maxLoops) || 1) : 1,
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
          <label className="flex min-h-9 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-2.5 py-2 font-sans text-xs text-tx-0">
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
        </FormSection>

        <FormSection
          title={t('escalations.sections.steps', { defaultValue: 'Escalation steps' })}
          description={t('escalations.sections.steps_description', { defaultValue: 'Each step emits a Notify event for its targets. Notify Policies select connectors, preferences and fallback routes.' }) as string}
          className="mb-0"
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
        </FormSection>
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
    <div className="flex flex-col gap-3 rounded-md border border-bd-1 bg-bg-1 p-3">
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
            value={step.min_severity ?? ''}
            onChange={(v) => setStep({ ...step, min_severity: v ? (v as Severity) : null })}
            options={severityOptions}
          />
        </FormField>
        <FormField label={t('escalations.fields.ack_timeout', { defaultValue: 'Ack timeout' })}>
          <FormSelect
            value={String(step.ack_timeout_secs)}
            onChange={(v) => setStep({ ...step, ack_timeout_secs: Number(v) })}
            options={ackOptions}
          />
        </FormField>
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="font-sans text-xs font-strong uppercase tracking-wide text-tx-3">
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

  return (
    <div className="flex flex-col gap-2 rounded-md border border-bd-0 bg-bg-2 p-2">
      <div className="flex items-center gap-2">
        <div className="w-28 shrink-0">
          <FormSelect
            value={target.kind}
            onChange={(v) => setTarget(emptyTarget(v as EscalationTarget['kind']))}
            options={kindOptions}
          />
        </div>
        <FormSelect
          value={targetRefId(target)}
          onChange={(v) => setTarget(withRefId(target, v))}
          options={[{ value: '', label: t('escalations.pick_ref', { defaultValue: '— select —' }) }, ...refOptions]}
          className="flex-1"
        />
      </div>
      <p className="font-sans text-xs text-tx-3">
        {t('escalations.fields.notify_routing', {
          defaultValue: 'Connector selection and fallback routing are configured in Settings → Notify → Policies.',
        })}
      </p>
    </div>
  );
}
