import { useMutation, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { runOverRows } from '@/admin';
import * as usageApi from '@/api/intakeUsage';
import type { BudgetPeriod, SourceUsage, UsageDimension } from '@/api/intakeUsage';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRadio,
  FormRow,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import {
  LIMIT_UNITS,
  type LimitUnit,
  limitParts,
  parseLimit,
  parseWarnPercent,
  sourceName,
} from './model';

const FORM_ID = 'intake-budget-form';
const DEFAULT_WARN_PERCENT = 80;

interface Draft {
  period: BudgetPeriod;
  limit: string;
  unit: LimitUnit;
  warn: string;
  notify: ReadonlySet<string>;
}

/** Sets the budget of one source, or the same one on several. What one source has is filled in. */
export function BudgetDrawer({
  dimension,
  sources,
  onClose,
  onSaved,
}: {
  dimension: UsageDimension;
  /** Empty while the drawer is closed. */
  sources: readonly SourceUsage[];
  onClose: () => void;
  /** The list has changed; `allSaved` says that no source is left to try again. */
  onSaved: (allSaved: boolean) => void | Promise<unknown>;
}) {
  const { t } = useTranslation('streams');
  const userId = useAuthStore((state) => state.ctx?.userId) ?? '';
  const open = sources.length > 0;
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (!open) {
      setDraft(null);
      return;
    }

    const budget = sources.length === 1 ? sources[0]?.budget : null;
    const limit = budget ? limitParts(budget.limitSize) : { text: '', unit: 'GiB' as const };

    setDraft({
      period: budget?.period ?? 'day',
      limit: limit.text,
      unit: limit.unit,
      warn: String(budget?.warnPercent ?? DEFAULT_WARN_PERCENT),
      notify: new Set(budget?.notifyUserIds ?? (userId ? [userId] : [])),
    });
    // Only what was opened fills the form: a list that reads itself again must not wipe a draft.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const members = useQuery({
    queryKey: ['iam', 'users'],
    queryFn: () => usersApi.list(),
    enabled: open,
  });

  const activeMembers = (members.data ?? []).filter((member) => !member.disabled && member.status !== 'pending');
  const limitSize = draft ? parseLimit(draft.limit, draft.unit) : null;
  const warnPercent = draft ? parseWarnPercent(draft.warn) : null;

  // Without the list of members the form cannot offer anybody: the person setting it is told.
  // Somebody who left since the budget was set is not offered, and the backend would refuse them.
  const recipients = members.isError
    ? null
    : (draft ? [...draft.notify] : []).filter(
        (id) => !members.data || activeMembers.some((member) => member.id === id),
      );
  const noRecipient = recipients !== null && recipients.length === 0 && !members.isLoading;
  const invalid = limitSize === null || warnPercent === null || noRecipient;

  const save = useMutation({
    mutationFn: () => {
      if (!draft || limitSize === null || warnPercent === null) return Promise.reject(new Error('invalid'));

      return runOverRows(sources, (source) =>
        usageApi.setBudget({
          dimension,
          sourceKey: source.sourceKey,
          period: draft.period,
          limitSize: limitSize,
          warnPercent: warnPercent,
          ...(recipients ? { notifyUserIds: recipients } : {}),
        }),
      );
    },
    onSuccess: async (outcome) => {
      if (outcome.failed === 0) {
        toast.success(t('usage.budget.saved', { count: outcome.done }));
        onClose();
      } else {
        toast.error(
          t('usage.budget.save_failed', {
            count: outcome.failed,
            total: outcome.done + outcome.failed,
            reason: toApiError(outcome.firstError).message,
          }),
        );
      }

      await onSaved(outcome.failed === 0);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const change = (next: Partial<Draft>) => setDraft((current) => (current ? { ...current, ...next } : current));

  const toggleMember = (id: string, checked: boolean) => {
    if (!draft) return;

    const notify = new Set(draft.notify);
    if (checked) notify.add(id);
    else notify.delete(id);
    change({ notify });
  };

  const single = sources.length === 1 ? sources[0] : undefined;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={single?.budget ? t('usage.budget.change_title') : t('usage.budget.set_title')}
      subtitle={
        single
          ? sourceName(t, single.sourceKey, single.label)
          : t('usage.budget.many_subtitle', { count: sources.length })
      }
      width={560}
      footer={
        <FormSubmitFooter
          formId={FORM_ID}
          busy={save.isPending}
          invalid={invalid}
          submitLabel={t('usage.budget.save')}
          onCancel={onClose}
        />
      }
    >
      {draft && (
        <form
          id={FORM_ID}
          className="flex flex-col gap-5"
          onSubmit={(event) => {
            event.preventDefault();
            if (!invalid) save.mutate();
          }}
        >
          <FormGroup label={t('usage.budget.period')}>
            <FormRadio<BudgetPeriod>
              value={draft.period}
              onChange={(period) => change({ period })}
              options={[
                { value: 'day', label: t('usage.budget.periods.day'), hint: t('usage.budget.periods.day_hint') },
                {
                  value: 'month',
                  label: t('usage.budget.periods.month'),
                  hint: t('usage.budget.periods.month_hint'),
                },
              ]}
            />
          </FormGroup>
          <FormRow>
            <FormField label={t('usage.budget.limit')} hint={t('usage.budget.limit_hint')} required>
              <FormInput
                inputMode="decimal"
                value={draft.limit}
                onChange={(event) => change({ limit: event.target.value })}
                aria-invalid={(draft.limit.trim() !== '' && limitSize === null) || undefined}
                data-testid="budget-limit"
              />
            </FormField>
            <FormField label={t('usage.budget.unit')}>
              <FormSelect
                value={draft.unit}
                onChange={(unit) => change({ unit: unit as LimitUnit })}
                options={[...LIMIT_UNITS]}
                ariaLabel={t('usage.budget.unit')}
              />
            </FormField>
          </FormRow>
          <FormField label={t('usage.budget.warn_percent')} hint={t('usage.budget.warn_percent_hint')}>
            <FormInput
              inputMode="numeric"
              value={draft.warn}
              onChange={(event) => change({ warn: event.target.value })}
              aria-invalid={warnPercent === null || undefined}
              data-testid="budget-warn"
            />
          </FormField>
          <FormGroup
            label={t('usage.budget.notify')}
            hint={
              members.isError
                ? t('usage.budget.notify_self')
                : noRecipient
                  ? t('usage.budget.notify_required')
                  : t('usage.budget.notify_hint')
            }
          >
            {!members.isError && (
              <div className="flex max-h-56 flex-col gap-1 overflow-y-auto">
                {activeMembers.map((member) => (
                  <label
                    key={member.id}
                    className="flex min-h-8 cursor-pointer items-center gap-2 rounded-md px-2 hover:bg-bg-2"
                  >
                    <Checkbox
                      checked={draft.notify.has(member.id)}
                      onCheckedChange={(checked) => toggleMember(member.id, checked === true)}
                    />
                    <span className="truncate font-sans text-sm text-tx-1">
                      {member.displayName || member.email}
                    </span>
                    {member.displayName && (
                      <span className="truncate font-sans text-xs text-tx-3">{member.email}</span>
                    )}
                  </label>
                ))}
              </div>
            )}
          </FormGroup>
        </form>
      )}
    </FormDrawer>
  );
}
