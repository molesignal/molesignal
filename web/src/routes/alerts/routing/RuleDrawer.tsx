import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as routingApi from '@/api/alertRouting';
import {
  CONDITION_OPERATORS,
  INCIDENT_ORIGINS,
  type ConditionOperator,
  type IncidentOrigin,
  type RoutingRule,
} from '@/api/alertRouting';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import {
  FieldArray,
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { prioritiesOf, useImportance } from '@/shell/incident/Importance';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { TimeConditionField, windowsInvalid } from '@/shell/weekly-ranges/WeeklyWindowsFields';
import type { Priority, Severity, TimeCondition } from '@/types/alerting';

import { ROUTING_KEY, valuesFromText } from './model';

const SEVERITIES: readonly Severity[] = ['info', 'warning', 'error', 'critical'];

interface ConditionRow {
  key: string;
  operator: ConditionOperator;
  values: string;
}

/**
 * One routing rule: the labels an incident must carry, how bad, how heavy, where
 * from and when it must be, and the policy or team it then goes to.
 */
export function RoutingRuleDrawer({
  open,
  editing,
  policies,
  teams,
  onClose,
}: {
  open: boolean;
  editing: RoutingRule | null;
  policies: ReadonlyArray<{ id: string; name: string }>;
  teams: ReadonlyArray<{ id: string; name: string }>;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const offered = prioritiesOf(levels);
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.routing.manage' });
  const [name, setName] = React.useState('');
  const [enabled, setEnabled] = React.useState(true);
  const [conditions, setConditions] = React.useState<ConditionRow[]>([]);
  const [minSeverity, setMinSeverity] = React.useState('');
  const [origins, setOrigins] = React.useState<IncidentOrigin[]>([]);
  const [priorities, setPriorities] = React.useState<Priority[]>([]);
  const [time, setTime] = React.useState<TimeCondition | null>(null);
  const [targetKind, setTargetKind] = React.useState<'policy' | 'team'>('team');
  const [targetId, setTargetId] = React.useState('');

  React.useEffect(() => {
    if (!open) return;

    setName(editing?.name ?? '');
    setEnabled(editing?.enabled ?? true);
    setConditions(
      (editing?.conditions ?? []).map((condition) => ({
        key: condition.key,
        operator: condition.operator,
        values: condition.values.join(', '),
      })),
    );
    setMinSeverity(editing?.minSeverity ?? '');
    setOrigins(editing?.origins ?? []);
    setPriorities(editing?.priorities ?? []);
    setTime(editing?.time ?? null);
    setTargetKind(editing?.target.kind ?? 'team');
    setTargetId(
      editing ? (editing.target.kind === 'policy' ? editing.target.policyId : editing.target.teamId) : '',
    );
  }, [open, editing]);

  const save = useMutation({
    mutationFn: async () => {
      const input: routingApi.RoutingRuleInput = {
        name: name.trim(),
        enabled,
        conditions: conditions
          .filter((row) => row.key.trim())
          .map((row) => ({
            key: row.key.trim(),
            operator: row.operator,
            values: valuesFromText(row.operator, row.values),
          })),
        minSeverity: (minSeverity || null) as Severity | null,
        origins,
        time,
        priorities,
        target: targetKind === 'policy' ? { kind: 'policy', policyId: targetId } : { kind: 'team', teamId: targetId },
      };

      return editing ? routingApi.updateRule(editing.id, input) : routingApi.createRule(input);
    },
    onSuccess: () => {
      toast.success(t(editing ? 'routing.toast_updated' : 'routing.toast_created'));
      void queryClient.invalidateQueries({ queryKey: ROUTING_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const invalid = !name.trim() || !targetId || (time !== null && windowsInvalid(time));

  const submit = (event: React.FormEvent) => {
    event.preventDefault();

    if (!manageAccess.allowed || invalid) return;
    save.mutate();
  };

  const targets = targetKind === 'policy' ? policies : teams;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t(editing ? 'routing.drawer.edit_title' : 'routing.drawer.new_title')}
      subtitle={t('routing.drawer.subtitle')}
      width={760}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t(editing ? 'routing.drawer.save' : 'routing.drawer.create')}
          formId="routing-rule-form"
        />
      }
    >
      <form id="routing-rule-form" onSubmit={submit}>
        <fieldset
          disabled={manageAccess.disabled}
          aria-disabled={manageAccess.disabled || undefined}
          className="contents"
        >
          <FormSection title={t('routing.drawer.identity')}>
            <FormField label={t('routing.drawer.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('routing.drawer.name_placeholder')}
                required
              />
            </FormField>
            <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={enabled} onCheckedChange={(next) => setEnabled(next === true)} />
              <span>{t('routing.drawer.enabled')}</span>
            </label>
          </FormSection>

          <FormSection title={t('routing.drawer.conditions')} description={t('routing.drawer.conditions_hint')}>
            <FieldArray
              items={conditions}
              onChange={setConditions}
              newItem={(): ConditionRow => ({ key: '', operator: 'equals', values: '' })}
              addLabel={t('routing.drawer.add_condition')}
              removeLabel={t('routing.drawer.remove_condition')}
              maxItems={16}
              renderItem={(row, _index, setRow) => (
                <div className="grid grid-cols-[minmax(0,1fr)_132px_minmax(0,1.2fr)] gap-2">
                  <FormInput
                    aria-label={t('routing.drawer.key')}
                    value={row.key}
                    onChange={(event) => setRow({ ...row, key: event.target.value })}
                    placeholder="service.name"
                  />
                  <FormSelect
                    ariaLabel={t('routing.drawer.operator')}
                    value={row.operator}
                    onChange={(value) => setRow({ ...row, operator: value as ConditionOperator })}
                    options={CONDITION_OPERATORS.map((operator) => ({
                      value: operator,
                      label: t(`routing.drawer.operators.${operator}`),
                    }))}
                  />
                  <FormInput
                    aria-label={t('routing.drawer.value')}
                    value={row.values}
                    onChange={(event) => setRow({ ...row, values: event.target.value })}
                    placeholder={row.operator === 'in' ? t('routing.drawer.value_in_placeholder') : 'checkout'}
                  />
                </div>
              )}
            />
          </FormSection>

          <FormSection title={t('routing.drawer.scope')}>
            <FormField label={t('routing.drawer.min_severity')}>
              <FormSelect
                value={minSeverity}
                onChange={setMinSeverity}
                options={[
                  { value: '', label: t('routing.drawer.any_severity') },
                  ...SEVERITIES.map((value) => ({ value, label: t(`severity.${value}`) })),
                ]}
              />
            </FormField>
            <FormField label={t('routing.drawer.origins')} hint={t('routing.drawer.origins_hint')}>
              <div className="flex flex-wrap gap-2">
                {INCIDENT_ORIGINS.map((origin) => (
                  <label
                    key={origin}
                    className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0"
                  >
                    <Checkbox
                      checked={origins.includes(origin)}
                      onCheckedChange={(next) =>
                        setOrigins((current) =>
                          next === true ? [...current, origin] : current.filter((item) => item !== origin),
                        )
                      }
                    />
                    <span>{t(`routing.origins.${origin}`)}</span>
                  </label>
                ))}
              </div>
            </FormField>
            <FormField label={t('routing.drawer.priorities')} hint={t('routing.drawer.priorities_hint')}>
              <div className="flex flex-wrap gap-2" data-routing-priorities>
                {offered.map((priority) => (
                  <label
                    key={priority}
                    className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0"
                  >
                    <Checkbox
                      checked={priorities.includes(priority)}
                      onCheckedChange={(next) =>
                        setPriorities((current) =>
                          next === true
                            ? offered.filter((item) => item === priority || current.includes(item))
                            : current.filter((item) => item !== priority),
                        )
                      }
                    />
                    <span>{priority}</span>
                  </label>
                ))}
              </div>
            </FormField>
            <TimeConditionField value={time} onChange={setTime} hint={t('routing.drawer.time_hint')} />
          </FormSection>

          <FormSection title={t('routing.drawer.target')} className="mb-0">
            <FormRadio
              value={targetKind}
              onChange={(kind) => {
                setTargetKind(kind);
                setTargetId('');
              }}
              className="grid grid-cols-2 gap-1.5"
              options={[
                { value: 'team', label: t('routing.drawer.target_team'), hint: t('routing.drawer.target_team_hint') },
                {
                  value: 'policy',
                  label: t('routing.drawer.target_policy'),
                  hint: t('routing.drawer.target_policy_hint'),
                },
              ]}
            />
            <FormField label={t(targetKind === 'policy' ? 'routing.drawer.target_policy' : 'routing.drawer.target_team')} required>
              <FormSelect
                value={targetId}
                onChange={setTargetId}
                placeholder={t('routing.drawer.target_placeholder')}
                options={targets.map((target) => ({ value: target.id, label: target.name }))}
              />
            </FormField>
          </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}
