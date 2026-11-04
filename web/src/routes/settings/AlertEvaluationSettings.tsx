import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { SectionBody, SettingsGroupStack, SettingsRow, SettingsSection } from './_atoms';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

/** Same bound as the backend (`MAX_EVALUATION_DELAY_SECS`). */
export const MAX_EVALUATION_DELAY_SECS = 3600;

const QUERY_KEY = ['settings', 'alert_evaluation'] as const;
const DRAFT_KEY = 'alert_evaluation';

/** The delay typed, or `null` when it is not a whole number of seconds from 0 to the maximum. */
export function parseEvaluationDelay(text: string): number | null {
  const value = text.trim();
  if (!/^\d+$/.test(value)) return null;
  const seconds = Number(value);
  return seconds <= MAX_EVALUATION_DELAY_SECS ? seconds : null;
}

/**
 * How far back alert rules look by default in a cluster split by role, where queries read only
 * what the intake nodes have written out. A platform setting; every alert node reads it each minute.
 */
export function AlertEvaluationSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader title={t('alert_evaluation.title')} subtitle={t('alert_evaluation.subtitle') as string} />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <DelaySection />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function DelaySection() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.alerting.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({ queryKey: QUERY_KEY, queryFn: () => instanceApi.getAlertEvaluationSettings() });
  const [draft, setDraft] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) setDraft(String(query.data.evaluationDelaySecs));
  }, [draft, query.data]);

  const seconds = draft === null ? null : parseEvaluationDelay(draft);
  const invalid = draft !== null && seconds === null;
  const dirty = Boolean(query.data && draft !== null && draft.trim() !== String(query.data.evaluationDelaySecs));

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(() => () => saveStatus.setDraftDirty(DRAFT_KEY, false), [saveStatus]);

  const save = useMutation({
    mutationFn: (evaluationDelaySecs: number) =>
      instanceApi.updateAlertEvaluationSettings({ evaluationDelaySecs: evaluationDelaySecs }),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setDraft(String(saved.evaluationDelaySecs));
      saveStatus.completeSave();
      toast.success(t('alert_evaluation.saved'));
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const controlsDisabled = access.disabled || query.isLoading || query.isError || save.isPending;
  const controlsDisabledReason =
    access.reason ??
    (query.isLoading
      ? tc('access.loading')
      : query.isError
        ? tc('access.page_unavailable')
        : save.isPending
          ? tc('access.operation_pending')
          : undefined);

  const persist = () => {
    if (seconds === null || !dirty || controlsDisabled) return;
    save.mutate(seconds);
  };

  return (
    <SettingsSection title={t('alert_evaluation.section_title')}>
      <SettingsRow label={t('alert_evaluation.delay')} description={t('alert_evaluation.delay_hint')}>
        <div className="flex items-center gap-2">
          <FormInput
            type="number"
            inputMode="numeric"
            min={0}
            max={MAX_EVALUATION_DELAY_SECS}
            step={1}
            value={draft ?? ''}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') persist();
            }}
            disabled={controlsDisabled || draft === null}
            disabledReason={controlsDisabledReason}
            aria-label={t('alert_evaluation.delay')}
            aria-invalid={invalid || undefined}
            className="w-28 tabular-nums"
          />
          <span className="font-sans text-sm text-tx-2">{t('alert_evaluation.unit')}</span>
        </div>
      </SettingsRow>

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {invalid
            ? t('alert_evaluation.invalid', { max: MAX_EVALUATION_DELAY_SECS })
            : dirty
              ? t('alert_evaluation.unsaved')
              : t('alert_evaluation.synced')}
        </div>
        <div className="flex items-center justify-end gap-2">
          <ChromeButton
            onClick={() => query.data && setDraft(String(query.data.evaluationDelaySecs))}
            disabled={access.disabled || !dirty || save.isPending}
            disabledReason={
              access.reason ??
              (!dirty ? tc('access.no_changes') : save.isPending ? tc('access.operation_pending') : undefined)
            }
          >
            {tc('actions.reset')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            onClick={persist}
            disabled={controlsDisabled || !dirty || seconds === null}
            disabledReason={
              access.reason ??
              (invalid ? tc('access.form_invalid') : !dirty ? tc('access.no_changes') : controlsDisabledReason)
            }
          >
            {save.isPending ? t('alert_evaluation.saving') : t('alert_evaluation.save')}
          </ChromeButton>
        </div>
      </div>
    </SettingsSection>
  );
}
