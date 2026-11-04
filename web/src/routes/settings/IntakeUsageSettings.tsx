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

import {
  SectionBody,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

/** Same bounds as the backend: a retention must read back a whole month of a budget. */
export const MIN_RETENTION_DAYS = 35;
export const MAX_RETENTION_DAYS = 3650;
export const MAX_SERVICES_PER_ORG = 100_000;

const QUERY_KEY = ['settings', 'intake_usage'] as const;
const DRAFT_KEY = 'intake_usage';

interface Draft {
  retentionDays: string;
  maxServices: string;
}

function toDraft(settings: instanceApi.IntakeUsageSettings): Draft {
  return {
    retentionDays: String(settings.retentionDays),
    maxServices: String(settings.maxServicesPerOrg),
  };
}

function wholeNumber(value: string, min: number, max: number): number | null {
  const text = value.trim();
  if (!/^\d+$/.test(text)) return null;
  const number = Number(text);
  return number >= min && number <= max ? number : null;
}

/** The settings typed in the fields, or `null` when they are out of bounds. */
export function parseIntakeUsageSettings(draft: Draft): instanceApi.IntakeUsageSettings | null {
  const retentionDays = wholeNumber(draft.retentionDays, MIN_RETENTION_DAYS, MAX_RETENTION_DAYS);
  const maxServices = wholeNumber(draft.maxServices, 1, MAX_SERVICES_PER_ORG);
  if (retentionDays === null || maxServices === null) return null;
  return { retentionDays: retentionDays, maxServicesPerOrg: maxServices };
}

/**
 * How long the hourly usage of intake is kept, and how many services an
 * organization has counted apart in an hour. A platform setting, served in the
 * platform organization only.
 */
export function IntakeUsageSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader title={t('intake_usage.title')} subtitle={t('intake_usage.subtitle') as string} />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <UsageSection />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function UsageSection() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.limits.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => instanceApi.getIntakeUsageSettings(),
  });
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) setDraft(toDraft(query.data));
  }, [draft, query.data]);

  const parsed = draft === null ? null : parseIntakeUsageSettings(draft);
  const invalid = draft !== null && parsed === null;
  const dirty = Boolean(
    query.data &&
      draft !== null &&
      (draft.retentionDays.trim() !== String(query.data.retentionDays) ||
        draft.maxServices.trim() !== String(query.data.maxServicesPerOrg)),
  );

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty(DRAFT_KEY, false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (settings: instanceApi.IntakeUsageSettings) =>
      instanceApi.updateIntakeUsageSettings(settings),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setDraft(toDraft(saved));
      saveStatus.completeSave();
      toast.success(t('intake_usage.saved'));
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const controlsDisabled =
    access.disabled || query.isLoading || query.isError || save.isPending;
  const controlsDisabledReason =
    access.reason ??
    (query.isLoading
      ? tc('access.loading')
      : query.isError
        ? tc('access.page_unavailable')
        : save.isPending
          ? tc('access.operation_pending')
          : undefined);

  const reset = () => {
    if (query.data) setDraft(toDraft(query.data));
  };

  const persist = () => {
    if (parsed === null || !dirty || controlsDisabled) return;
    save.mutate(parsed);
  };

  const numberField = (key: keyof Draft, label: string, unit: string, min: number, max: number) => (
    <div className="flex items-center gap-2">
      <FormInput
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={1}
        value={draft?.[key] ?? ''}
        onChange={(event) =>
          setDraft((current) => (current ? { ...current, [key]: event.target.value } : current))
        }
        onKeyDown={(event) => {
          if (event.key === 'Enter') persist();
        }}
        disabled={controlsDisabled || draft === null}
        disabledReason={controlsDisabledReason}
        aria-label={label}
        aria-invalid={invalid || undefined}
        className="w-36 tabular-nums"
      />
      <span className="font-sans text-sm text-tx-2">{unit}</span>
    </div>
  );

  return (
    <SettingsSection title={t('intake_usage.section_title')}>
      <SettingsRow
        label={t('intake_usage.retention_days')}
        description={t('intake_usage.retention_days_hint')}
      >
        {numberField(
          'retentionDays',
          t('intake_usage.retention_days'),
          t('intake_usage.days'),
          MIN_RETENTION_DAYS,
          MAX_RETENTION_DAYS,
        )}
      </SettingsRow>
      <SettingsRow
        label={t('intake_usage.max_services')}
        description={t('intake_usage.max_services_hint')}
      >
        {numberField(
          'maxServices',
          t('intake_usage.max_services'),
          t('intake_usage.services'),
          1,
          MAX_SERVICES_PER_ORG,
        )}
      </SettingsRow>

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {invalid
            ? t('intake_usage.invalid', {
                minDays: MIN_RETENTION_DAYS,
                maxDays: MAX_RETENTION_DAYS,
                maxServices: MAX_SERVICES_PER_ORG,
              })
            : dirty
              ? t('intake_usage.unsaved')
              : t('intake_usage.synced')}
        </div>
        <div className="flex items-center justify-end gap-2">
          <ChromeButton
            onClick={reset}
            disabled={access.disabled || !dirty || save.isPending}
            disabledReason={
              access.reason ??
              (!dirty
                ? tc('access.no_changes')
                : save.isPending
                  ? tc('access.operation_pending')
                  : undefined)
            }
          >
            {tc('actions.reset')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            onClick={persist}
            disabled={controlsDisabled || !dirty || parsed === null}
            disabledReason={
              access.reason ??
              (invalid
                ? tc('access.form_invalid')
                : !dirty
                  ? tc('access.no_changes')
                  : controlsDisabledReason)
            }
          >
            {save.isPending ? t('intake_usage.saving') : t('intake_usage.save')}
          </ChromeButton>
        </div>
      </div>
    </SettingsSection>
  );
}
