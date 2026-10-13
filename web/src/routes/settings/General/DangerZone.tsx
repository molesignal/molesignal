import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ChromeButton } from '@/shell/chrome';

import { usePlatformAction } from './platformAction';
import { SettingsRow, SettingsSection } from '../_atoms';

/**
 * Deleting an organization is done from the platform's organization
 * management, never from inside the organization itself. Whoever can enter
 * the platform organization (its administrator, or an owner) is taken there;
 * anyone else is told who can do it, not that they are a permission short.
 */
export function DangerZone() {
  const { t } = useTranslation(['settings-admin', 'common']);
  const navigate = useNavigate();
  const management = usePlatformAction('sys.organizations.manage');

  return (
    <SettingsSection
      title={t('general.danger.title')}
      description={t('general.danger.subtitle')}
      tone="danger"
    >
      <SettingsRow
        label={t('general.danger.delete_title')}
        description={t('general.danger.delete_description')}
      >
        {management.reach === 'none' ? (
          <p className="m-0 max-w-[320px] font-sans text-sm leading-relaxed text-tx-2">
            {t('general.danger.platform_only')}
          </p>
        ) : (
          <div className="flex max-w-[360px] flex-col items-start gap-1.5">
            <ChromeButton
              variant="danger"
              disabled={management.reach === 'loading' || management.entering}
              disabledReason={management.reach === 'loading' ? t('common:access.loading') : undefined}
              onClick={() => void management.run(() => navigate('/settings/organization_management'))}
              className="h-11 text-base lg:h-9 lg:text-sm"
            >
              {t('general.danger.manage_delete')}
            </ChromeButton>
            {management.reach === 'platform' && (
              <span className="font-sans text-xs leading-relaxed text-tx-3">
                {t('general.danger.enters_platform')}
              </span>
            )}
          </div>
        )}
      </SettingsRow>
    </SettingsSection>
  );
}
