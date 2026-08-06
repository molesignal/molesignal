import { useTranslation } from 'react-i18next';

import type * as instanceApi from '@/api/instance';
import type * as meApi from '@/api/me';
import type * as resourceSharesApi from '@/api/resourceShares';
import { ProductState } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { PreferencesFields } from '@/shell/PreferencesFields';
import { Switch } from '@/shell/ui/switch';
import type { Dashboard } from '@/types/dashboard';

import {
  SettingsDraftStatus,
  SettingsRow,
  SettingsSection,
  SettingsSubsection,
} from '../_atoms';

interface AccessProps {
  canManage: boolean;
  disabledReason?: string | undefined;
}

export function SignupPolicySection({
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
}: AccessProps & {
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

  return (
    <SettingsSubsection
      title={t('general.signup.title')}
      description={t('general.signup.subtitle')}
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
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.signup_enabled}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              onCheckedChange={(checked) =>
                onChange({ signup_enabled: checked })
              }
              aria-label={t('general.signup.enabled')}
            />
          </SettingsRow>
          {policy.signup_enabled && (
            <SettingsRow
              label={t('general.signup.require_approval')}
              description={t('general.signup.require_approval_hint')}
              controlClassName="justify-start min-[1100px]:justify-end"
            >
              <Switch
                className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
                checked={policy.signup_require_approval}
                disabled={!canManage || pending}
                disabledReason={controlsDisabledReason}
                onCheckedChange={(checked) =>
                  onChange({ signup_require_approval: checked })
                }
                aria-label={t('general.signup.require_approval')}
              />
            </SettingsRow>
          )}
          {policy.signup_enabled && (
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
    </SettingsSubsection>
  );
}

export function SharingPolicySection({
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
}: AccessProps & {
  policy?: resourceSharesApi.ResourceSharePolicy | undefined;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  pending: boolean;
  saveError: boolean;
  dirty: boolean;
  onChange: (
    patch: Partial<
      Omit<
        resourceSharesApi.ResourceSharePolicy,
        'organization_id' | 'updated_by' | 'updated_at'
      >
    >,
  ) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const dependencyReason = t('general.sharing.public_links_required');
  const controlsDisabledReason = !canManage
    ? disabledReason
    : pending
      ? t('common:access.operation_pending')
      : undefined;
  const dependentDisabledReason = !policy?.allow_public_links
    ? dependencyReason
    : controlsDisabledReason;

  return (
    <SettingsSubsection
      title={t('general.sharing.title')}
      description={t('general.sharing.subtitle')}
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
            label={t('general.sharing.allow_public_links')}
            description={t('general.sharing.allow_public_links_hint')}
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allow_public_links}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              aria-label={t('general.sharing.allow_public_links')}
              onCheckedChange={(checked) =>
                onChange({ allow_public_links: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.allow_public_dashboards')}
            description={t('general.sharing.allow_public_dashboards_hint')}
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allow_public_dashboards}
              disabled={!policy.allow_public_links || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.allow_public_dashboards')}
              onCheckedChange={(checked) =>
                onChange({ allow_public_dashboards: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.max_expiry')}
            description={t('general.sharing.max_expiry_hint')}
            controlClassName="w-full min-[1100px]:w-48"
          >
            <FormSelect
              value={String(policy.max_public_expiry_secs)}
              ariaLabel={t('general.sharing.max_expiry')}
              className="h-11 text-base lg:h-9 lg:text-sm"
              disabled={!policy.allow_public_links || !canManage || pending}
              disabledReason={dependentDisabledReason}
              onChange={(value) =>
                onChange({ max_public_expiry_secs: Number(value) })
              }
              options={[
                {
                  value: String(24 * 60 * 60),
                  label: t('general.sharing.one_day'),
                },
                {
                  value: String(7 * 24 * 60 * 60),
                  label: t('general.sharing.seven_days'),
                },
                {
                  value: String(30 * 24 * 60 * 60),
                  label: t('general.sharing.thirty_days'),
                },
              ]}
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.report_password')}
            description={t('general.sharing.report_password_hint')}
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.require_public_report_password}
              disabled={!policy.allow_public_links || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.report_password')}
              onCheckedChange={(checked) =>
                onChange({ require_public_report_password: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.deny_production')}
            description={t('general.sharing.deny_production_hint')}
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.deny_production_public_shares}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              aria-label={t('general.sharing.deny_production')}
              onCheckedChange={(checked) =>
                onChange({ deny_production_public_shares: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.allow_csv_download')}
            description={t('general.sharing.allow_csv_download_hint')}
            controlClassName="justify-start min-[1100px]:justify-end"
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allow_public_csv_download}
              disabled={!policy.allow_public_links || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.allow_csv_download')}
              onCheckedChange={(checked) =>
                onChange({ allow_public_csv_download: checked })
              }
            />
          </SettingsRow>
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
    </SettingsSubsection>
  );
}

export function PreferenceDefaultsSection({
  value,
  dashboards,
  dirty,
  isLoading,
  isError,
  error,
  pending,
  saveError,
  canManage,
  disabledReason,
  onChange,
  onReset,
  onSave,
}: AccessProps & {
  value: meApi.UserPreferences;
  dashboards: Dashboard[];
  dirty: boolean;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  pending: boolean;
  saveError: boolean;
  onChange: (patch: Partial<meApi.UserPreferences>) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);

  return (
    <SettingsSection
      title={t('general.preference_defaults.title')}
      description={t('general.preference_defaults.subtitle')}
    >
      {isLoading ? (
        <div className="py-8">
          <ProductState variant="loading" compact />
        </div>
      ) : isError ? (
        <div className="py-8">
          <ProductState variant="error" error={error} compact />
        </div>
      ) : (
        <div className="w-full">
          <PreferencesFields
            value={value}
            dashboards={dashboards}
            onChange={onChange}
            onThemePreview={() => undefined}
            surface="page"
            readOnly={!canManage}
            disabled={pending}
            disabledReason={
              !canManage
                ? disabledReason
                : pending
                  ? t('common:access.operation_pending')
                  : undefined
            }
          />
          <div className="mt-5">
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
          </div>
        </div>
      )}
    </SettingsSection>
  );
}
