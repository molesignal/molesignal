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
import { AuditImpact, useAuditImpactFacts } from './retention/AuditImpact';
import { auditSaveStep } from './retention/model';
import { useRetentionPreview } from './retention/useRetentionPreview';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

/** Same upper bound as the backend (`MAX_AUDIT_RETENTION_DAYS`). */
export const MAX_AUDIT_RETENTION_DAYS = 3650;

const QUERY_KEY = ['settings', 'audit_retention'] as const;
const DRAFT_KEY = 'audit_retention';

/** The days typed in the field, or `null` when they are not a whole number from 0 to the maximum. */
export function parseRetentionDays(value: string): number | null {
  const text = value.trim();
  if (!/^\d+$/.test(text)) return null;
  const days = Number(text);
  return days <= MAX_AUDIT_RETENTION_DAYS ? days : null;
}

/**
 * How long audit records are kept, for every organization and the platform
 * alike. It is a platform setting, so the page is served in the platform
 * organization only; the compactor deletes older records once a day.
 */
export function AuditRetentionSettings() {
  const { t } = useTranslation('settings-admin');

  return (
    <>
      <PageHeader
        title={t('audit_retention.title')}
        subtitle={t('audit_retention.subtitle') as string}
      />
      <SectionBody opensOnSection className="pb-10">
        <SettingsGroupStack>
          <RetentionSection />
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}

function RetentionSection() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const access = useActionAccess({ permission: 'sys.settings.retention.manage' });
  const queryClient = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => instanceApi.getAuditRetentionSettings(),
  });
  const [draft, setDraft] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (query.data && draft === null) {
      setDraft(String(query.data.retentionDays));
    }
  }, [draft, query.data]);

  const days = draft === null ? null : parseRetentionDays(draft);
  const invalid = draft !== null && days === null;
  const dirty = Boolean(
    query.data && draft !== null && draft.trim() !== String(query.data.retentionDays),
  );
  const stored = query.data?.retentionDays;

  const impact = useRetentionPreview({
    queryKey: [...QUERY_KEY, 'preview'],
    load: instanceApi.previewAuditRetention,
    stored,
    days: dirty ? days : null,
  });

  const impactFacts = useAuditImpactFacts(impact.status === 'ready' ? impact.preview : null);
  const saveStep = auditSaveStep(impact);
  const [confirming, setConfirming] = React.useState(false);

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(
    () => () => saveStatus.setDraftDirty(DRAFT_KEY, false),
    [saveStatus],
  );

  const save = useMutation({
    mutationFn: (retentionDays: number) =>
      instanceApi.updateAuditRetentionSettings({ retentionDays: retentionDays }),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      queryClient.setQueryData(QUERY_KEY, saved);
      setDraft(String(saved.retentionDays));
      setConfirming(false);
      saveStatus.completeSave();
      toast.success(t('audit_retention.saved'));
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
    <SettingsSection title={t('audit_retention.section_title')}>
      <SettingsRow
        label={t('audit_retention.days')}
        description={t('audit_retention.days_hint')}
      >
        <div className="flex items-center gap-2">
          <FormInput
            type="number"
            inputMode="numeric"
            min={0}
            max={MAX_AUDIT_RETENTION_DAYS}
            step={1}
            value={draft ?? ''}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') persist();
            }}
            disabled={controlsDisabled || draft === null}
            disabledReason={controlsDisabledReason}
            aria-label={t('audit_retention.days')}
            aria-invalid={invalid || undefined}
            className="w-28 tabular-nums"
          />
          <span className="font-sans text-sm text-tx-2">
            {t('audit_retention.days_unit')}
          </span>
        </div>
      </SettingsRow>

      <AuditImpact impact={impact} />

      <div className="flex min-h-16 flex-col gap-3 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="font-sans text-xs text-tx-3">
          {invalid
            ? t('audit_retention.invalid')
            : dirty
              ? t('audit_retention.unsaved')
              : t('audit_retention.synced')}
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
                    ? t('audit_retention.preview.counting')
                    : controlsDisabledReason)
            }
          >
            {save.isPending
              ? t('audit_retention.saving')
              : t('audit_retention.save')}
          </ChromeButton>
        </div>
      </div>

      {stored !== undefined && days !== null && (
        <ConfirmDialog
          open={confirming}
          onOpenChange={setConfirming}
          destructive
          title={
            stored === 0
              ? t('audit_retention.confirm.title_from_forever', { count: days })
              : t('audit_retention.confirm.title', { count: days, from: stored })
          }
          description={
            impactFacts
              ? t('audit_retention.confirm.description')
              : t('audit_retention.confirm.description_unknown', { count: days })
          }
          consequences={impactFacts}
          typeToConfirm={saveStep === 'type' ? String(days) : undefined}
          confirmLabel={t('audit_retention.confirm.action')}
          busy={save.isPending}
          busyLabel={t('audit_retention.saving')}
          disabledReason={save.isPending ? undefined : t('audit_retention.confirm.type_first')}
          onConfirm={() => save.mutate(days)}
        />
      )}
    </SettingsSection>
  );
}
