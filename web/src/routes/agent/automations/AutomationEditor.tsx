import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as automationsApi from '@/api/agent/automations';
import type { Automation } from '@/api/agent/automations';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { channelConnectors } from '@/routes/logs/inspections/model';
import { RoutesField } from '@/routes/logs/inspections/RoutesField';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import {
  DESCRIPTION_MAX,
  NAME_MAX,
  PROMPT_MAX,
  TIMEOUT_CHOICES,
  draftFromAutomation,
  draftProblems,
  inputFromDraft,
  type AutomationDraft,
} from './model';
import { TriggerFields } from './TriggerFields';
import { useScopes } from '../scope/useScopes';

export const AUTOMATIONS_KEY = ['agent', 'automations'] as const;

const FORM_ID = 'agent-automation-editor';

/** What the editor opens on: an automation to change, or the draft of a new one. */
export type EditorOpening =
  | { automation: Automation; draft?: undefined }
  | { automation?: undefined; draft: AutomationDraft };

/**
 * The form an automation is saved with: what Mole Agent is asked, whose it is,
 * what starts it, and where the result goes. Saving makes the reader the one it
 * runs as, so the footer says so.
 *
 * The form lives for one opening: nothing typed is carried into the next, and
 * nothing is written over it when the list behind reads the automation again.
 */
export function AutomationEditor({
  opening,
  timezone,
  onClose,
}: {
  opening: EditorOpening | null;
  timezone: string;
  onClose: () => void;
}) {
  return opening ? <AutomationForm opening={opening} timezone={timezone} onClose={onClose} /> : null;
}

function AutomationForm({
  opening,
  timezone,
  onClose,
}: {
  opening: EditorOpening;
  timezone: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const mayListConnectors = hasPermission('notify.connectors.read', access);
  const scopes = useScopes();
  const existing = opening.automation ?? null;
  const [draft, setDraft] = React.useState<AutomationDraft>(() =>
    existing ? draftFromAutomation(existing, timezone) : opening.draft!,
  );
  const change = (patch: Partial<AutomationDraft>) => setDraft((current) => ({ ...current, ...patch }));

  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: () => notifyApi.listConnectors(),
    enabled: mayListConnectors,
    retry: false,
  });
  const channels = React.useMemo(
    () => channelConnectors(connectors.data ?? [], draft.channels),
    [connectors.data, draft.channels],
  );

  // A new automation can be anyone's the reader may create for: theirs, their teams', the organization's.
  const scopeOptions = React.useMemo(
    () => [
      ...scopes.options.filter((option) => option.value !== 'org'),
      { value: 'org', label: t('scope.org') },
    ],
    [scopes.options, t],
  );

  const problems = draftProblems(draft);
  const save = useMutation({
    mutationFn: () => {
      const input = inputFromDraft(draft, existing === null);
      return existing
        ? automationsApi.updateAutomation(existing.id, input)
        : automationsApi.createAutomation(input);
    },
    onSuccess: () => {
      toast.success(t(existing ? 'automations.toast.saved' : 'automations.toast.created'));
      void queryClient.invalidateQueries({ queryKey: AUTOMATIONS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open
      onOpenChange={(open) => !open && onClose()}
      title={t(existing ? 'automations.editor.edit_title' : 'automations.editor.new_title')}
      subtitle={t('automations.editor.subtitle')}
      width={680}
      footer={
        <>
          <p data-automation-run-as className="mr-auto min-w-0 max-w-[360px] font-sans text-xs leading-relaxed text-tx-3">
            {t('automations.editor.run_as')}
          </p>
          <FormSubmitFooter
            formId={FORM_ID}
            busy={save.isPending}
            invalid={problems.length > 0}
            onCancel={onClose}
            submitLabel={t(existing ? 'automations.editor.save' : 'automations.editor.create')}
          />
        </>
      }
    >
      <form
        id={FORM_ID}
        data-automation-editor={existing ? 'edit' : 'new'}
        onSubmit={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (problems.length === 0 && !save.isPending) save.mutate();
        }}
      >
        <FormSection title={t('automations.editor.what')}>
          <FormField label={t('automations.editor.name')} required>
            <FormInput
              value={draft.name}
              maxLength={NAME_MAX}
              placeholder={t('automations.editor.name_placeholder')}
              onChange={(event) => change({ name: event.target.value })}
            />
          </FormField>
          <FormField label={t('automations.editor.prompt')} hint={t('automations.editor.prompt_hint')} required>
            <FormTextarea
              value={draft.prompt}
              rows={8}
              maxLength={PROMPT_MAX}
              placeholder={t('automations.editor.prompt_placeholder')}
              onChange={(event) => change({ prompt: event.target.value })}
            />
          </FormField>
          <FormField label={t('automations.editor.description')}>
            <FormInput
              value={draft.description}
              maxLength={DESCRIPTION_MAX}
              placeholder={t('automations.editor.description_placeholder')}
              onChange={(event) => change({ description: event.target.value })}
            />
          </FormField>
          {existing === null && (
            <FormField label={t('automations.editor.scope')} hint={t('automations.editor.scope_hint')}>
              <FormSelect
                ariaLabel={t('automations.editor.scope')}
                value={draft.scope}
                onChange={(scope) => change({ scope })}
                options={scopeOptions}
              />
            </FormField>
          )}
        </FormSection>

        <FormSection title={t('automations.editor.when')}>
          <TriggerFields draft={draft} onChange={change} />
          <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-end gap-3">
            <FormField label={t('automations.editor.timeout')} hint={t('automations.editor.timeout_hint')}>
              <FormSelect
                ariaLabel={t('automations.editor.timeout')}
                value={String(draft.timeoutSecs)}
                onChange={(value) => change({ timeoutSecs: Number(value) })}
                options={TIMEOUT_CHOICES.map((seconds) => ({
                  value: String(seconds),
                  label: t('automations.editor.minutes', { count: seconds / 60 }),
                }))}
              />
            </FormField>
            <label className="flex min-h-9 items-center gap-2 font-sans text-xs text-tx-1">
              <Switch
                checked={draft.enabled}
                onCheckedChange={(enabled) => change({ enabled })}
                aria-label={t('automations.editor.enabled')}
              />
              <span>{t('automations.editor.enabled')}</span>
            </label>
          </div>
        </FormSection>

        <FormSection title={t('automations.editor.outputs')} className="mb-2">
          <div className="flex flex-col gap-1.5">
            <span className="type-label font-sans font-medium text-tx-2">{t('automations.editor.channels')}</span>
            {!mayListConnectors || connectors.isError ? (
              <p data-automation-channels="unavailable" className="m-0 font-sans text-xs leading-relaxed text-tx-3">
                {t('automations.editor.channels_unavailable')}
              </p>
            ) : connectors.isPending ? null : channels.length === 0 ? (
              <p data-automation-channels="none" className="m-0 font-sans text-xs leading-relaxed text-tx-3">
                {t('automations.editor.channels_none')}
              </p>
            ) : (
              <RoutesField
                routes={draft.channels}
                connectors={channels}
                onChange={(routes) => change({ channels: routes.slice(0, 5) })}
              />
            )}
            <span className="font-sans text-xs leading-relaxed text-tx-3">{t('automations.editor.channels_hint')}</span>
          </div>
          {draft.kind === 'incident' && (
            <label className="flex min-h-9 items-center gap-2 font-sans text-xs text-tx-1">
              <Switch
                checked={draft.writeRca}
                onCheckedChange={(writeRca) => change({ writeRca })}
                aria-label={t('automations.editor.write_rca')}
              />
              <span>{t('automations.editor.write_rca')}</span>
            </label>
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}
