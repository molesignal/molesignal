import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { SettingsDraftStatus, SettingsRow, SettingsSection } from './_atoms';
import { useAutoSave } from './General/useAutoSave';
import { useSettingsSaveStatus } from './SettingsSaveStatus';

const QUERY_KEY = ['settings', 'signup'] as const;
const DRAFT_KEY = 'signin_requirements.signup_policy';

/**
 * Whether anyone can sign up, on the whole instance: new accounts join the first organization, so
 * it is a platform setting and lives with the platform's sign-in requirements.
 */
export function SignupPolicySettings() {
  const qc = useQueryClient();
  const saveStatus = useSettingsSaveStatus();
  const access = useActionAccess({ permission: 'sys.settings.signin.manage' });
  const [draft, setDraft] = React.useState<instanceApi.SignupPolicy | null>(null);

  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => instanceApi.getSignupPolicy(),
    enabled: access.allowed,
  });

  const policy = draft ?? query.data;

  const dirty = Boolean(
    query.data &&
      policy &&
      (policy.signupEnabled !== query.data.signupEnabled ||
        policy.signupRequireApproval !== query.data.signupRequireApproval),
  );

  React.useEffect(() => {
    if (query.data) setDraft(query.data);
  }, [query.data]);

  React.useEffect(() => {
    saveStatus.setDraftDirty(DRAFT_KEY, dirty);
  }, [dirty, saveStatus]);

  React.useEffect(() => () => saveStatus.setDraftDirty(DRAFT_KEY, false), [saveStatus]);

  const update = useMutation({
    mutationFn: (next: instanceApi.SignupPolicy) => instanceApi.updateSignupPolicy(next),
    onMutate: () => saveStatus.beginSave(),
    onSuccess: (saved) => {
      qc.setQueryData(QUERY_KEY, saved);
      setDraft(saved);
      saveStatus.completeSave();
    },
    onError: (error) => {
      saveStatus.failSave();
      toast.error(toApiError(error).message);
    },
  });

  const retry = useAutoSave({
    fingerprint: policy ? `${policy.signupEnabled}:${policy.signupRequireApproval}` : '',
    enabled: access.allowed && Boolean(policy) && dirty,
    busy: update.isPending,
    save: () => (policy ? update.mutateAsync(policy) : Promise.resolve()),
  });

  return (
    <SignupPolicySection
      policy={policy}
      isLoading={query.isLoading}
      isError={query.isError}
      error={query.error}
      pending={update.isPending}
      saveError={update.isError}
      dirty={dirty}
      canManage={access.allowed}
      disabledReason={access.reason}
      onChange={(patch) => {
        if (!access.allowed || !policy || update.isPending) return;
        update.reset();
        setDraft({ ...policy, ...patch });
      }}
      onReset={() => {
        update.reset();
        setDraft(query.data ?? null);
      }}
      onSave={retry}
    />
  );
}

function SignupPolicySection({
  policy,
  isLoading,
  isError,
  error,
  pending,
  saveError,
  dirty,
  canManage,
  disabledReason,
  onChange,
  onReset,
  onSave,
}: {
  canManage: boolean;
  disabledReason?: string | undefined;
  policy?: instanceApi.SignupPolicy | undefined;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  pending: boolean;
  saveError: boolean;
  dirty: boolean;
  onChange: (patch: Partial<instanceApi.SignupPolicy>) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const controlsDisabledReason = !canManage
    ? disabledReason
    : pending
      ? t('common:access.operation_pending')
      : undefined;
  // Without email nobody can prove an address, so signing up always waits for approval.
  const noEmail = policy?.canVerifyEmail === false;

  return (
    <SettingsSection
      title={t('general.signup.title')}
      description={t('general.signup.subtitle')}
      contentClassName="gap-0"
    >
      {isLoading ? (
        <div className="py-6">
          <ProductState variant="loading" compact />
        </div>
      ) : isError || !policy ? (
        <div className="py-6">
          <ProductState variant="error" error={error} compact />
        </div>
      ) : (
        <>
          <SettingsRow
            label={t('general.signup.enabled')}
            description={t('general.signup.enabled_hint')}
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.signupEnabled}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              onCheckedChange={(checked) =>
                onChange({ signupEnabled: checked })
              }
              aria-label={t('general.signup.enabled')}
            />
          </SettingsRow>
          {policy.signupEnabled && (
            <SettingsRow
              label={t('general.signup.require_approval')}
              description={t(
                noEmail
                  ? 'general.signup.require_approval_no_email'
                  : 'general.signup.require_approval_hint',
              )}
            >
              <Switch
                className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
                checked={noEmail || policy.signupRequireApproval}
                disabled={!canManage || pending || noEmail}
                disabledReason={
                  controlsDisabledReason ??
                  (noEmail ? t('general.signup.require_approval_locked') : undefined)
                }
                onCheckedChange={(checked) =>
                  onChange({ signupRequireApproval: checked })
                }
                aria-label={t('general.signup.require_approval')}
              />
            </SettingsRow>
          )}
          {policy.signupEnabled && (
            <SettingsRow
              label={t('general.signup.default_role')}
              description={t('general.signup.default_role_hint')}
            >
              <Pill tone="dim">{t('general.signup.default_role_value')}</Pill>
            </SettingsRow>
          )}
          <SettingsDraftStatus
            dirty={dirty || pending}
            error={saveError}
            modifiedLabel={t('general.auto_save.modified')}
            undoLabel={t('general.auto_save.undo')}
            errorLabel={t('general.auto_save.error')}
            retryLabel={t('general.auto_save.retry')}
            onUndo={onReset}
            onRetry={onSave}
          />
        </>
      )}
    </SettingsSection>
  );
}
