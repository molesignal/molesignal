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

/** Same bound as the backend (`MAX_ACTIVE_SERIES_LIMIT`). */
export const MAX_ACTIVE_SERIES_LIMIT = 100_000_000;

const QUERY_KEY = ['settings', 'series_limits'] as const;
const DRAFT_KEY = 'series_limits';

interface Draft {
  perOrg: string;
  perMetric: string;
}

function toDraft(settings: instanceApi.SeriesLimitSettings): Draft {
  return {
    perOrg: String(settings.maxActiveSeriesPerOrg),
    perMetric: String(settings.maxActiveSeriesPerMetric),
  };
}

function wholeCount(value: string): number | null {
  const text = value.trim();
  if (!/^\d+$/.test(text)) return null;
  const count = Number(text);
  return count >= 1 && count <= MAX_ACTIVE_SERIES_LIMIT ? count : null;
}

/**
 * The limits typed in the fields, or `null` when they are not whole numbers
 * from 1 to the maximum, or a metric would be allowed more than its organization.
 */
export function parseSeriesLimits(draft: Draft): instanceApi.SeriesLimitSettings | null {
  const perOrg = wholeCount(draft.perOrg);
  const perMetric = wholeCount(draft.perMetric);
  if (perOrg === null || perMetric === null || perMetric > perOrg) return null;
  return { maxActiveSeriesPerOrg: perOrg, maxActiveSeriesPerMetric: perMetric };
}

/**
 * How many Prometheus series an organization, and a metric in it, may keep
 * active at once. A platform setting, served in the platform organization only;
 * every intake node reads it again every half minute.
 */
export function SeriesLimitSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader
        title={t('series_limits.title')}
        subtitle={t('series_limits.subtitle') as string}
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <LimitsSection />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function LimitsSection() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.limits.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => instanceApi.getSeriesLimitSettings(),
  });
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) setDraft(toDraft(query.data));
  }, [draft, query.data]);

  const limits = draft === null ? null : parseSeriesLimits(draft);
  const invalid = draft !== null && limits === null;
  const dirty = Boolean(
    query.data &&
      draft !== null &&
      (draft.perOrg.trim() !== String(query.data.maxActiveSeriesPerOrg) ||
        draft.perMetric.trim() !== String(query.data.maxActiveSeriesPerMetric)),
  );

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty(DRAFT_KEY, false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (settings: instanceApi.SeriesLimitSettings) =>
      instanceApi.updateSeriesLimitSettings(settings),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setDraft(toDraft(saved));
      saveStatus.completeSave();
      toast.success(t('series_limits.saved'));
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
    if (limits === null || !dirty || controlsDisabled) return;
    save.mutate(limits);
  };

  const countField = (key: keyof Draft, label: string) => (
    <div className="flex items-center gap-2">
      <FormInput
        type="number"
        inputMode="numeric"
        min={1}
        max={MAX_ACTIVE_SERIES_LIMIT}
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
      <span className="font-sans text-sm text-tx-2">{t('series_limits.unit')}</span>
    </div>
  );

  return (
    <SettingsSection title={t('series_limits.section_title')}>
      <SettingsRow
        label={t('series_limits.per_org')}
        description={t('series_limits.per_org_hint')}
      >
        {countField('perOrg', t('series_limits.per_org'))}
      </SettingsRow>
      <SettingsRow
        label={t('series_limits.per_metric')}
        description={t('series_limits.per_metric_hint')}
      >
        {countField('perMetric', t('series_limits.per_metric'))}
      </SettingsRow>

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {invalid
            ? t('series_limits.invalid', { max: MAX_ACTIVE_SERIES_LIMIT })
            : dirty
              ? t('series_limits.unsaved')
              : t('series_limits.synced')}
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
            disabled={controlsDisabled || !dirty || limits === null}
            disabledReason={
              access.reason ??
              (invalid
                ? tc('access.form_invalid')
                : !dirty
                  ? tc('access.no_changes')
                  : controlsDisabledReason)
            }
          >
            {save.isPending ? t('series_limits.saving') : t('series_limits.save')}
          </ChromeButton>
        </div>
      </div>
    </SettingsSection>
  );
}
