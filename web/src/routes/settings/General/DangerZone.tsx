import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';

import { SettingsRow, SettingsSection } from '../_atoms';

export function DangerZone({ access }: { access: ActionAccess }) {
  const { t } = useTranslation('settings-admin');
  const navigate = useNavigate();

  return (
    <SettingsSection
      title={t('general.danger.title')}
      description={t('general.danger.subtitle')}
      tone="danger"
    >
      <SettingsRow
        label={t('general.danger.delete_title')}
        description={t('general.danger.delete_description')}
        controlClassName="justify-start min-[1100px]:justify-end"
      >
        <ChromeButton
          disabled={access.disabled}
          disabledReason={access.reason}
          onClick={() => navigate('/settings/organization_management')}
          className="h-11 border-red/30 bg-red-dim text-base text-red-soft enabled:hover:border-red/50 enabled:hover:bg-red-dim enabled:hover:text-tx-0 lg:h-9 lg:text-sm"
        >
          {t('general.danger.manage_delete')}
        </ChromeButton>
      </SettingsRow>
    </SettingsSection>
  );
}
