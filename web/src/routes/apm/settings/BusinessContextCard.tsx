import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Briefcase, Plus, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { type BusinessContext, businessContextApi, businessContextQueryKeys } from '@/api/businessContext';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import {
  type BusinessKeyDraft,
  MAX_BUSINESS_KEYS,
  blankBusinessKey,
  businessKeyIssues,
  contextOf,
  draftOf,
} from './businessContext';
import { CardSave, FIELD, Issues, useDraft } from './parts';
import { ApmCard } from '../frame/Page';

const EMPTY: BusinessContext = { keys: [] };

/**
 * The business keys the organization reads its telemetry by, and the field
 * each signal carries them in: what the APM service page splits requests by,
 * and what an incident's business impact is told in. Saved on its own.
 */
export function BusinessContextCard({ access }: { access: ActionAccess }) {
  const { t } = useTranslation('apm');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const client = useQueryClient();
  const query = useQuery({
    queryKey: businessContextQueryKeys.context(orgId),
    queryFn: businessContextApi.get,
    enabled: Boolean(orgId),
  });
  const draft = useDraft<BusinessKeyDraft[]>(draftOf(query.data ?? EMPTY));
  const save = useMutation({
    mutationFn: businessContextApi.save,
    onSuccess: (saved) => {
      client.setQueryData(businessContextQueryKeys.context(orgId), saved);
      draft.reset();
      toast.success(t('org_settings.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const keys = draft.value;
  const issues = businessKeyIssues(keys);
  const disabled = access.disabled || !query.data;
  const change = (index: number, next: BusinessKeyDraft) => draft.set(keys.map((key, at) => (at === index ? next : key)));

  return (
    <ApmCard
      icon={Briefcase}
      title={t('org_settings.business.title')}
      description={t('org_settings.business.hint')}
      testId="settings-business-context"
      actions={
        <CardSave
          name="business-context"
          dirty={draft.dirty}
          invalid={issues.size > 0}
          pending={save.isPending}
          access={access}
          onSave={() => save.mutate(contextOf(keys))}
          onReset={draft.reset}
        />
      }
    >
      <div data-apm-settings-list="business-context" className="space-y-2 p-3">
        {keys.length === 0 && <p className="py-1 font-sans text-xs text-tx-3">{t('org_settings.business.empty')}</p>}
        {keys.map((key, index) => (
          <KeyRow
            key={index}
            index={index}
            value={key}
            invalid={issues.has(index)}
            disabled={disabled}
            onChange={(next) => change(index, next)}
            onRemove={() => draft.set(keys.filter((_, at) => at !== index))}
          />
        ))}
        {keys.length < MAX_BUSINESS_KEYS && (
          <button
            type="button"
            disabled={disabled}
            onClick={() => draft.set([...keys, blankBusinessKey()])}
            className="inline-flex h-8 items-center gap-1.5 rounded-md px-2 font-sans text-xs text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 disabled:opacity-40"
          >
            <Plus aria-hidden className="h-3.5 w-3.5" />
            {t('org_settings.business.add')}
          </button>
        )}
      </div>
      <Issues issues={[...new Set(issues.values())].map((issue) => t(`org_settings.business.issue.${issue}`))} />
    </ApmCard>
  );
}

function KeyRow({
  index,
  value,
  invalid,
  disabled,
  onChange,
  onRemove,
}: {
  index: number;
  value: BusinessKeyDraft;
  invalid: boolean;
  disabled: boolean;
  onChange: (next: BusinessKeyDraft) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation('apm');
  const field = (name: keyof BusinessKeyDraft, placeholder: string, wide = false) => (
    <label className={cn('flex min-w-0 flex-col gap-1 font-sans text-xs text-tx-2', wide && 'sm:col-span-3')}>
      <span>{t(`org_settings.business.${name}`)}</span>
      <input
        aria-invalid={(invalid && (name === 'key' || name === 'span' || name === 'log' || name === 'rum')) || undefined}
        value={value[name]}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => onChange({ ...value, [name]: event.target.value })}
        className={FIELD}
      />
    </label>
  );

  return (
    <div data-apm-settings-row={index} className="flex items-start gap-2 rounded-md border border-bd-0 p-2">
      <div className="grid min-w-0 flex-1 gap-2 sm:grid-cols-3">
        {field('key', 'tenant')}
        {field('label', t('org_settings.business.label_placeholder'))}
        <span aria-hidden className="hidden sm:block" />
        {field('span', 'tenant.id')}
        {field('log', 'tenant_id')}
        {field('rum', 'context.tenant')}
        {field('important', t('org_settings.business.important_placeholder'), true)}
      </div>
      <button
        type="button"
        aria-label={t('org_settings.remove')}
        disabled={disabled}
        onClick={onRemove}
        className="mt-5 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 disabled:opacity-40"
      >
        <Trash2 aria-hidden className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
