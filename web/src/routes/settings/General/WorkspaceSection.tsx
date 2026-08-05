import { useTranslation } from 'react-i18next';

import type * as meApi from '@/api/me';
import type { ActionAccess } from '@/product/actionAccess';
import {
  ProductState,
  type ProductStateProps,
} from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';

import {
  CopyableValue,
  SettingsRow,
  SettingsSection,
} from '../_atoms';

export function WorkspaceSection({
  profile,
  role,
  state,
  name,
  dirty,
  invalid,
  access,
  pending,
  onNameChange,
  onReset,
  onSave,
}: {
  profile: meApi.MeProfile | undefined;
  role: string;
  state: ProductStateProps | null;
  name: string;
  dirty: boolean;
  invalid: boolean;
  access: ActionAccess;
  pending: boolean;
  onNameChange: (name: string) => void;
  onReset: () => void;
  onSave: () => void;
}) {
  const { t } = useTranslation(['settings-admin', 'common']);
  const pendingReason = pending
    ? t('common:access.operation_pending')
    : undefined;

  return (
    <SettingsSection
      title={
        <span className="inline-flex items-center gap-2">
          {t('general.workspace_title')}
          {profile ? <Pill tone="indigo">{role}</Pill> : null}
        </span>
      }
      description={t('general.workspace_description')}
    >
      {state ? (
        <div className="py-6">
          <ProductState {...state} compact />
        </div>
      ) : (
        <>
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
                disabled={access.disabled || pending}
                disabledReason={access.reason ?? pendingReason}
              />
              <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
                <span
                  aria-live="polite"
                  className="font-sans text-xs text-tx-3"
                >
                  {dirty ? t('preferences.unsaved') : ''}
                </span>
                <div className="flex items-center gap-2">
                  <ChromeButton
                    size="sm"
                    disabled={access.disabled || !dirty || pending}
                    disabledReason={
                      access.reason ??
                      (pending
                        ? pendingReason
                        : !dirty
                          ? t('common:access.no_changes')
                          : undefined)
                    }
                    onClick={onReset}
                  >
                    {t('common:actions.reset')}
                  </ChromeButton>
                  <ChromeButton
                    size="sm"
                    variant="primary"
                    disabled={
                      access.disabled || !dirty || invalid || pending
                    }
                    disabledReason={
                      access.reason ??
                      (pending
                        ? pendingReason
                        : invalid
                          ? t('common:access.form_invalid')
                          : !dirty
                            ? t('common:access.no_changes')
                            : undefined)
                    }
                    onClick={onSave}
                  >
                    {pending
                      ? t('common:status.saving')
                      : t('common:actions.save')}
                  </ChromeButton>
                </div>
              </div>
            </div>
          </SettingsRow>
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
        </>
      )}
    </SettingsSection>
  );
}
