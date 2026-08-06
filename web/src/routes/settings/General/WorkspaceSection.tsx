import { useTranslation } from 'react-i18next';

import type * as meApi from '@/api/me';
import type { ActionAccess } from '@/product/actionAccess';
import {
  ProductState,
  type ProductStateProps,
} from '@/product/states';
import { FormInput } from '@/shell/FormDrawer';

import {
  CopyableValue,
  SettingsDraftStatus,
  SettingsRow,
  SettingsSection,
  SettingsSubsection,
} from '../_atoms';

export function WorkspaceSection({
  profile,
  state,
  name,
  dirty,
  invalid,
  access,
  pending,
  error,
  onNameChange,
  onReset,
  onSave,
}: {
  profile: meApi.MeProfile | undefined;
  state: ProductStateProps | null;
  name: string;
  dirty: boolean;
  invalid: boolean;
  access: ActionAccess;
  pending: boolean;
  error: boolean;
  onNameChange: (name: string) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);

  return (
    <SettingsSection
      title={t('general.organization_information.title')}
      description={t('general.organization_information.subtitle')}
      contentClassName="gap-0"
    >
      <SettingsSubsection
        title={t('general.basic_information.title')}
        description={t('general.basic_information.subtitle')}
      >
        {state ? (
          <div className="py-2">
            <ProductState {...state} compact />
          </div>
        ) : (
          <SettingsRow
            label={t('general.organization_name')}
            description={t('general.organization_name_hint')}
            controlClassName="w-full"
          >
            <div className="w-full">
              <FormInput
                value={name}
                onChange={(event) => onNameChange(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    onSave();
                  }
                  if (event.key === 'Escape') onReset();
                }}
                aria-label={t('general.organization_name')}
                aria-invalid={invalid || undefined}
                disabled={access.disabled}
                disabledReason={access.reason}
                className="h-11 text-base lg:h-9 lg:text-sm"
              />
              {invalid ? (
                <p className="mt-2 font-sans text-sm text-red-soft">
                  {t('general.organization_name_required')}
                </p>
              ) : (
                <div className="mt-2">
                  <SettingsDraftStatus
                    dirty={dirty || pending}
                    error={error}
                    modifiedLabel={t('general.auto_save.modified')}
                    undoLabel={t('general.auto_save.undo')}
                    errorLabel={t('general.auto_save.error')}
                    retryLabel={t('general.auto_save.retry')}
                    onUndo={onReset}
                    onRetry={onSave}
                  />
                </div>
              )}
            </div>
          </SettingsRow>
        )}
      </SettingsSubsection>

      <SettingsSubsection
        title={t('general.system_identity.title')}
        description={t('general.system_identity.subtitle')}
      >
        <SettingsRow
          label={t('general.organization_slug')}
          description={t('general.organization_slug_hint')}
          controlClassName="w-full"
        >
          <CopyableValue
            value={profile?.org_slug ?? '—'}
            copyLabel={t('general.copy')}
            copiedLabel={t('general.copied')}
          />
        </SettingsRow>
        <SettingsRow
          label={t('general.organization_id')}
          description={t('general.organization_id_hint')}
          controlClassName="w-full"
        >
          <CopyableValue
            value={profile?.org_id ?? '—'}
            copyLabel={t('general.copy')}
            copiedLabel={t('general.copied')}
          />
        </SettingsRow>
      </SettingsSubsection>
    </SettingsSection>
  );
}
