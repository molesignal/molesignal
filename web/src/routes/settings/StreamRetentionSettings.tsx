import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, PageHeader } from '@/admin';
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
import { streamSaveStep } from './retention/model';
import { StreamImpact, useStreamImpactFacts } from './retention/StreamImpact';
import { useRetentionPreview } from './retention/useRetentionPreview';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

/** Same bounds as the backend (`MAX_STREAM_RETENTION_DAYS`, `MAX_SYNTHETIC_RESULT_RETENTION_DAYS`). */
export const MAX_STREAM_RETENTION_DAYS = 3650;

/** The days typed in the field, or `null` when they are not a whole number from `min` to `max`. */
export function parseRetentionDays(value: string, min: number, max: number): number | null {
  const text = value.trim();
  if (!/^\d+$/.test(text)) return null;
  const days = Number(text);
  return days >= min && days <= max ? days : null;
}

/** The days a stream keeps by default: a whole number from 1 to the maximum. */
export function parseStreamRetentionDays(value: string): number | null {
  return parseRetentionDays(value, 1, MAX_STREAM_RETENTION_DAYS);
}

/** One retention the page sets: where it is read and saved, its bounds, and its words. */
interface Retention {
  queryKey: readonly string[];
  draftKey: string;
  load: () => Promise<{ retentionDays: number }>;
  store: (days: number) => Promise<{ retentionDays: number }>;
  /** What a shorter retention would delete, where that can be told before saving. */
  preview?: (days: number, signal?: AbortSignal) => Promise<instanceApi.StreamRetentionPreview>;
  /** 0 keeps for good where it is allowed. */
  min: number;
  texts: 'stream_retention' | 'synthetic_retention';
}

const STREAMS: Retention = {
  queryKey: ['settings', 'stream_retention'],
  draftKey: 'stream_retention',
  load: () => instanceApi.getStreamRetentionSettings(),
  store: (days) => instanceApi.updateStreamRetentionSettings({ retentionDays: days }),
  preview: (days, signal) => instanceApi.previewStreamRetention(days, signal),
  min: 1,
  texts: 'stream_retention',
};

const SYNTHETIC_RESULTS: Retention = {
  queryKey: ['settings', 'synthetic_retention'],
  draftKey: 'synthetic_retention',
  load: () => instanceApi.getSyntheticRetentionSettings(),
  store: (days) => instanceApi.updateSyntheticRetentionSettings({ retentionDays: days }),
  min: 0,
  texts: 'synthetic_retention',
};

/**
 * How long data is kept, in every organization: a stream that has no retention
 * of its own, and the runs of synthetic checks. These are platform settings, so
 * the page is served in the platform organization only; the compactor reads
 * them on every round.
 */
export function StreamRetentionSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader
        title={t('stream_retention.title')}
        subtitle={t('stream_retention.subtitle') as string}
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <RetentionSection retention={STREAMS} />
          <RetentionSection retention={SYNTHETIC_RESULTS} />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function RetentionSection({ retention }: { retention: Retention }) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.retention.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const { queryKey, draftKey, texts, min } = retention;
  const query = useQuery({
    queryKey,
    queryFn: retention.load,
  });
  const [draft, setDraft] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) {
      setDraft(String(query.data.retentionDays));
    }
  }, [draft, query.data]);

  const days = draft === null ? null : parseRetentionDays(draft, min, MAX_STREAM_RETENTION_DAYS);
  const invalid = draft !== null && days === null;
  const dirty = Boolean(
    query.data && draft !== null && draft.trim() !== String(query.data.retentionDays),
  );
  const stored = query.data?.retentionDays;

  const impact = useRetentionPreview({
    queryKey: [...queryKey, 'preview'],
    load: retention.preview,
    stored,
    days: dirty ? days : null,
  });

  const impactFacts = useStreamImpactFacts(impact.status === 'ready' ? impact.preview : null);
  const saveStep = streamSaveStep(impact);
  const [confirming, setConfirming] = React.useState(false);

  React.useEffect(() => {
    saveStatus.setDraftDirty(draftKey, dirty);
  }, [dirty, draftKey, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty(draftKey, false),
    [draftKey, saveStatus],
  );

  const save = useMutation({
    mutationFn: retention.store,
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(queryKey, saved);
      setDraft(String(saved.retentionDays));
      setConfirming(false);
      saveStatus.completeSave();
      toast.success(t(`${texts}.saved`));
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
    if (query.data) setDraft(String(query.data.retentionDays));
  };

  const persist = () => {
    if (days === null || !dirty || controlsDisabled || impact.status === 'pending') return;

    if (saveStep === 'save') save.mutate(days);
    else setConfirming(true);
  };

  return (
    <SettingsSection title={t(`${texts}.section_title`)}>
      <SettingsRow label={t(`${texts}.days`)} description={t(`${texts}.days_hint`)}>
        <div className="flex items-center gap-2">
          <FormInput
            type="number"
            inputMode="numeric"
            min={min}
            max={MAX_STREAM_RETENTION_DAYS}
            step={1}
            value={draft ?? ''}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') persist();
            }}
            disabled={controlsDisabled || draft === null}
            disabledReason={controlsDisabledReason}
            aria-label={t(`${texts}.days`)}
            aria-invalid={invalid || undefined}
            className="w-28 tabular-nums"
          />
          <span className="font-sans text-sm text-tx-2">
            {t('stream_retention.days_unit')}
          </span>
        </div>
      </SettingsRow>

      {retention.preview && <StreamImpact impact={impact} />}

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {invalid
            ? t(`${texts}.invalid`, { max: MAX_STREAM_RETENTION_DAYS })
            : dirty
              ? t('stream_retention.unsaved')
              : t('stream_retention.synced')}
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
            disabled={controlsDisabled || !dirty || days === null || impact.status === 'pending'}
            disabledReason={
              access.reason ??
              (invalid
                ? tc('access.form_invalid')
                : !dirty
                  ? tc('access.no_changes')
                  : impact.status === 'pending'
                    ? t('stream_retention.preview.estimating')
                    : controlsDisabledReason)
            }
          >
            {save.isPending ? t('stream_retention.saving') : t('stream_retention.save')}
          </ChromeButton>
        </div>
      </div>

      {retention.preview && stored !== undefined && days !== null && (
        <ConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          destructive
          title={t('stream_retention.confirm.title', { count: days, from: stored })}
          description={
            impactFacts
              ? t('stream_retention.confirm.description')
              : t('stream_retention.confirm.description_unknown', { count: days })
          }
          consequences={impactFacts}
          typeToConfirm={saveStep === 'type' ? String(days) : undefined}
          confirmLabel={t('stream_retention.confirm.action')}
          busy={save.isPending}
          busyLabel={t('stream_retention.saving')}
          disabledReason={save.isPending ? undefined : t('stream_retention.confirm.type_first')}
          onConfirm={() => save.mutate(days)}
        />
      )}
    </SettingsSection>
  );
}
