import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as nodeAgentsApi from '@/api/nodeAgents';
import type { NodeTaskQuota } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormInput, FormSection, FormSubmitFooter } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

export const QUOTA_FIELDS = [
  'maxActivePerOrg',
  'maxActivePerNode',
  'maxCreatedPerHourPerOrg',
  'maxCreatedPerHourPerNode',
] as const satisfies readonly (keyof NodeTaskQuota)[];

const CEILING = 100_000;

type Draft = Record<keyof NodeTaskQuota, string>;

const draftOf = (quota: NodeTaskQuota): Draft =>
  Object.fromEntries(QUOTA_FIELDS.map((field) => [field, String(quota[field])])) as Draft;

/** The quota as numbers, or `undefined` while a field is not a whole number in range. */
function quotaOf(draft: Draft): NodeTaskQuota | undefined {
  const quota: NodeTaskQuota = {
    maxActivePerOrg: Number(draft.maxActivePerOrg),
    maxActivePerNode: Number(draft.maxActivePerNode),
    maxCreatedPerHourPerOrg: Number(draft.maxCreatedPerHourPerOrg),
    maxCreatedPerHourPerNode: Number(draft.maxCreatedPerHourPerNode),
  };

  const inRange = QUOTA_FIELDS.every((field) => {
    const value = quota[field];
    return Number.isInteger(value) && value >= 1 && value <= CEILING;
  });

  return inRange && quota.maxActivePerNode <= quota.maxActivePerOrg ? quota : undefined;
}

/** How many node tasks the organization may have at once and create per hour. */
export function QuotaDrawer({
  quota,
  onClose,
  onSaved,
}: {
  quota: NodeTaskQuota | undefined;
  onClose: () => void;
  onSaved: (quota: NodeTaskQuota) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [draft, setDraft] = React.useState<Draft | undefined>(quota && draftOf(quota));
  React.useEffect(() => setDraft(quota && draftOf(quota)), [quota]);
  const next = draft && quotaOf(draft);

  const save = useMutation({
    mutationFn: nodeAgentsApi.setTaskQuota,
    onSuccess: (saved) => {
      toast.success(t('node_agents.quota.saved'));
      onSaved(saved);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={Boolean(quota)}
      onOpenChange={(open) => !open && onClose()}
      title={t('node_agents.quota.title')}
      subtitle={t('node_agents.quota.subtitle')}
      width={520}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          invalid={!next}
          onCancel={onClose}
          formId="node-task-quota"
          submitLabel={t('node_agents.quota.save')}
        />
      }
    >
      <form
        id="node-task-quota"
        onSubmit={(event) => {
          event.preventDefault();
          if (next) save.mutate(next);
        }}
      >
        <FormSection description={t('node_agents.quota.description')}>
          {draft &&
            QUOTA_FIELDS.map((field) => (
              <FormField key={field} label={t(`node_agents.quota.${field}`)}>
                <FormInput
                  type="number"
                  min={1}
                  max={CEILING}
                  value={draft[field]}
                  onChange={(event) => setDraft({ ...draft, [field]: event.target.value })}
                />
              </FormField>
            ))}
        </FormSection>
      </form>
    </FormDrawer>
  );
}
