import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ErrorRule, ErrorRuleWrite, ErrorSummary } from '@/api/apm';
import { apmErrorRulesApi, apmErrorRulesQueryKeys } from '@/api/apmErrorRules';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { Button } from '@/shell/ui/button';
import { Checkbox } from '@/shell/ui/checkbox';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';

import { hasPattern, PATTERN_FIELD, RULE_PATTERNS, ruleMatchesGroup } from './rules';
import { Choice, Field } from '../catalog/DefinitionDrawer';

const SEPARATOR = '␟';

/**
 * A rule as it is set: its name, what it does, the service it is for, the
 * patterns an error must match, and whether it is on. Under the patterns,
 * how many of the groups in view are like the errors it would take.
 */
export function ErrorRuleForm({
  orgId,
  rule,
  initial,
  groups,
  onDone,
}: {
  orgId: string;
  /** The rule changed; none for a new one. */
  rule: ErrorRule | null;
  initial: ErrorRuleWrite;
  groups: readonly ErrorSummary[];
  onDone: () => void;
}) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'apm.error_rules.manage' });
  const [draft, setDraft] = React.useState(initial);
  const set = (patch: Partial<ErrorRuleWrite>) => setDraft((current) => ({ ...current, ...patch }));

  const services = unique([
    ...(draft.serviceName ? [{ value: `${draft.serviceNamespace}${SEPARATOR}${draft.serviceName}`, label: draft.serviceName }] : []),
    ...groups.map((group) => ({ value: `${group.service.namespace}${SEPARATOR}${group.service.name}`, label: group.service.name })),
  ]);
  const matching = groups.filter((group) => ruleMatchesGroup(draft, group)).length;
  const ready = draft.name.trim() !== '' && hasPattern(draft);

  const save = useMutation({
    mutationFn: () => (rule ? apmErrorRulesApi.update(rule.id, draft) : apmErrorRulesApi.create(draft)),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: apmErrorRulesQueryKeys.list(orgId) });
      toast.success(t('errors.rules.saved'));
      onDone();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      data-apm-error-rule-form=""
      onSubmit={(event) => {
        event.preventDefault();
        if (ready) save.mutate();
      }}
    >
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4 font-sans text-xs">
        <Field label={t('errors.rules.fields.name')} hint={t('errors.rules.fields.name_hint')}>
          <Input value={draft.name} onChange={(event) => set({ name: event.target.value })} required maxLength={128} data-apm-error-rule-name="" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('errors.rules.fields.action')}>
            <Choice
              value={draft.action === 'merge' ? '' : draft.action}
              onChange={(value) => set({ action: value === 'ignore' ? 'ignore' : 'merge' })}
              none={t('errors.rules.action_long.merge')}
              options={[{ value: 'ignore', label: t('errors.rules.action_long.ignore') }]}
            />
          </Field>
          <Field label={t('errors.rules.fields.service')}>
            <Choice
              value={draft.serviceName ? `${draft.serviceNamespace}${SEPARATOR}${draft.serviceName}` : ''}
              onChange={(value) => {
                const [namespace = '', name = ''] = value.split(SEPARATOR);
                set({ serviceNamespace: value ? namespace : '', serviceName: value ? name : '' });
              }}
              none={t('errors.rules.every_service')}
              options={services}
            />
          </Field>
        </div>
        <div className="space-y-3">
          {RULE_PATTERNS.map((pattern) => (
            <Field key={pattern} label={t(`errors.rules.fields.${pattern}`)}>
              <Input
                value={draft[PATTERN_FIELD[pattern]]}
                onChange={(event) => set({ [PATTERN_FIELD[pattern]]: event.target.value })}
                maxLength={256}
                placeholder={t(`errors.rules.placeholder.${pattern}`)}
                data-apm-error-rule-pattern={pattern}
              />
            </Field>
          ))}
          <p className="text-tx-3">{t('errors.rules.patterns_hint')}</p>
        </div>
        <label className="flex items-center gap-2 text-tx-1">
          <Checkbox checked={draft.enabled} onCheckedChange={(checked) => set({ enabled: checked === true })} data-apm-error-rule-enabled="" />
          {t('errors.rules.fields.enabled')}
        </label>
        <p data-apm-error-rule-preview={matching} className="text-tx-2">
          {t('errors.rules.preview', { count: matching })}
        </p>
      </div>
      <div className="flex items-center gap-2 border-t border-bd-0 px-4 py-3">
        <div className="flex-1" />
        <Button type="button" variant="outline" size="sm" onClick={onDone}>
          {t('actions.cancel')}
        </Button>
        <Button type="submit" size="sm" disabled={access.disabled || save.isPending || !ready} title={access.reason} data-apm-error-rule-save="">
          {t('actions.save')}
        </Button>
      </div>
    </form>
  );
}

function unique<T extends { value: string }>(options: readonly T[]): T[] {
  const seen = new Set<string>();
  return options.filter((option) => {
    if (seen.has(option.value)) return false;
    seen.add(option.value);
    return true;
  });
}
