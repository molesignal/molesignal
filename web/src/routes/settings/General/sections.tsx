import { useTranslation } from 'react-i18next';

import type * as meApi from '@/api/me';
import type * as resourceSharesApi from '@/api/resourceShares';
import { ProductState } from '@/product/states';
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
        'organizationId' | 'updatedBy' | 'updatedAt'
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
  const dependentDisabledReason = !policy?.allowPublicLinks
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
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allowPublicLinks}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              aria-label={t('general.sharing.allow_public_links')}
              onCheckedChange={(checked) =>
                onChange({ allowPublicLinks: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.allow_public_dashboards')}
            description={t('general.sharing.allow_public_dashboards_hint')}
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allowPublicDashboards}
              disabled={!policy.allowPublicLinks || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.allow_public_dashboards')}
              onCheckedChange={(checked) =>
                onChange({ allowPublicDashboards: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.max_expiry')}
            description={t('general.sharing.max_expiry_hint')}
            controlClassName="w-full min-[1100px]:w-48"
          >
            <FormSelect
              value={String(policy.maxPublicExpirySecs)}
              ariaLabel={t('general.sharing.max_expiry')}
              className="h-11 text-base lg:h-9 lg:text-sm"
              disabled={!policy.allowPublicLinks || !canManage || pending}
              disabledReason={dependentDisabledReason}
              onChange={(value) =>
                onChange({ maxPublicExpirySecs: Number(value) })
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
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.requirePublicReportPassword}
              disabled={!policy.allowPublicLinks || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.report_password')}
              onCheckedChange={(checked) =>
                onChange({ requirePublicReportPassword: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.deny_production')}
            description={t('general.sharing.deny_production_hint')}
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.denyProductionPublicShares}
              disabled={!canManage || pending}
              disabledReason={controlsDisabledReason}
              aria-label={t('general.sharing.deny_production')}
              onCheckedChange={(checked) =>
                onChange({ denyProductionPublicShares: checked })
              }
            />
          </SettingsRow>
          <SettingsRow
            label={t('general.sharing.allow_csv_download')}
            description={t('general.sharing.allow_csv_download_hint')}
          >
            <Switch
              className="relative before:absolute before:-inset-x-2 before:-inset-y-3 before:content-['']"
              checked={policy.allowPublicCsvDownload}
              disabled={!policy.allowPublicLinks || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.allow_csv_download')}
              onCheckedChange={(checked) =>
                onChange({ allowPublicCsvDownload: checked })
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
