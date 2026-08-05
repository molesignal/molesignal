import { useTranslation } from 'react-i18next';

import type * as instanceApi from '@/api/instance';
import type * as meApi from '@/api/me';
import type * as resourceSharesApi from '@/api/resourceShares';
import { ProductState } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { PreferencesFields } from '@/shell/PreferencesFields';
import { Switch } from '@/shell/ui/switch';
import type { Dashboard } from '@/types/dashboard';

import { SettingsRow, SettingsSection } from '../_atoms';

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
    <SettingsSection
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
          >
            <Switch
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
            >
              <Switch
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
          <SettingsRow
            label={t('general.signup.default_role')}
            description={t('general.signup.default_role_hint')}
          >
            <Pill tone="dim">{t('general.signup.default_role_value')}</Pill>
          </SettingsRow>
          <SettingsActions
            dirty={dirty}
            pending={pending}
            canManage={canManage}
            disabledReason={disabledReason}
            onReset={onReset}
            onSave={onSave}
          />
        </>
      )}
    </SettingsSection>
  );
}

export function SharingPolicySection({
  policy,
  isLoading,
  isError,
  error,
  pending,
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
    <SettingsSection
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
          >
            <Switch
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
            controlClassName="w-full md:w-44"
          >
            <FormSelect
              value={String(policy.max_public_expiry_secs)}
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
          >
            <Switch
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
          >
            <Switch
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
          >
            <Switch
              checked={policy.allow_public_csv_download}
              disabled={!policy.allow_public_links || !canManage || pending}
              disabledReason={dependentDisabledReason}
              aria-label={t('general.sharing.allow_csv_download')}
              onCheckedChange={(checked) =>
                onChange({ allow_public_csv_download: checked })
              }
            />
          </SettingsRow>
          <SettingsActions
            dirty={dirty}
            pending={pending}
            canManage={canManage}
            disabledReason={disabledReason}
            onReset={onReset}
            onSave={onSave}
          />
        </>
      )}
    </SettingsSection>
  );
}

function SettingsActions({
  dirty,
  pending,
  canManage,
  disabledReason,
  onReset,
  onSave,
}: AccessProps & {
  dirty: boolean;
  pending: boolean;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const reason = !canManage
    ? disabledReason
    : pending
        ? t('common:access.operation_pending')
        : !dirty
          ? t('common:access.no_changes')
          : undefined;

  return (
    <div className="flex min-h-16 flex-wrap items-center justify-between gap-3 py-3">
      <span aria-live="polite" className="font-sans text-xs text-tx-3">
        {dirty ? t('preferences.unsaved') : ''}
      </span>
      <div className="flex items-center gap-2">
        <ChromeButton
          type="button"
          disabled={!canManage || !dirty || pending}
          disabledReason={reason}
          onClick={onReset}
        >
          {t('common:actions.reset')}
        </ChromeButton>
        <ChromeButton
          type="button"
          variant="primary"
          disabled={!canManage || !dirty || pending}
          disabledReason={reason}
          onClick={onSave}
        >
          {pending ? t('common:status.saving') : t('common:actions.save')}
        </ChromeButton>
      </div>
    </div>
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
  onChange: (patch: Partial<meApi.UserPreferences>) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const noChangesReason = t('common:access.no_changes');

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
        <form
          className="max-w-[920px] py-5"
          onSubmit={(event) => {
            event.preventDefault();
            if (canManage && dirty && !pending) onSave();
          }}
        >
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
          <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
            <span
              aria-live="polite"
              className="font-sans text-xs text-tx-3"
            >
              {dirty ? t('preferences.unsaved') : ''}
            </span>
            <div className="flex items-center gap-2">
              <ChromeButton
                type="button"
                disabled={!canManage || !dirty || pending}
                disabledReason={
                  !canManage
                    ? disabledReason
                    : !dirty
                      ? noChangesReason
                      : pending
                        ? t('common:access.operation_pending')
                        : undefined
                }
                onClick={onReset}
              >
                {t('common:actions.reset')}
              </ChromeButton>
              <ChromeButton
                type="submit"
                variant="primary"
                disabled={!canManage || !dirty || pending}
                disabledReason={
                  !canManage
                    ? disabledReason
                    : !dirty
                      ? noChangesReason
                      : pending
                        ? t('common:access.operation_pending')
                        : undefined
                }
              >
                {pending
                  ? t('preferences.actions.saving')
                  : t('preferences.actions.save')}
              </ChromeButton>
            </div>
          </div>
        </form>
      )}
    </SettingsSection>
  );
}
