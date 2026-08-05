import { useTranslation } from 'react-i18next';

import { UserNotifyPanel } from '@/routes/notify/UserNotifyPanel';
import { useAuthStore } from '@/stores/auth';

import { AccountSection } from './AccountSection';

export function AccountNotify() {
  const { t } = useTranslation('notify');
  const userId = useAuthStore((state) => state.ctx?.user_id);
  return (
    <AccountSection
      title={t('account.title')}
      subtitle={t('account.subtitle')}
      width="page"
    >
      {userId ? (
        <UserNotifyPanel userId={userId} />
      ) : (
        <p className="text-sm text-tx-3">{t('account.unavailable')}</p>
      )}
    </AccountSection>
  );
}
