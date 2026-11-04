import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { UserNotifyPanel } from '@/routes/notify/UserNotifyPanel';
import { useReachability } from '@/shell/useReachability';
import { useAuthStore } from '@/stores/auth';

import { AccountSection } from './AccountSection';

export function AccountNotify() {
  const { t } = useTranslation('notify');
  const userId = useAuthStore((state) => state.ctx?.userId);
  const reach = useReachability(userId ? [userId] : []);
  const mine = userId ? reach[userId] : undefined;
  return (
    <AccountSection
      title={t('account.title')}
      subtitle={t('account.subtitle')}
      width="page"
    >
      {mine && mine !== 'personal' && (
        <div
          role="status"
          data-account-unreachable
          className="mb-4 flex items-start gap-2 rounded-md bg-orange-dim px-3 py-2.5 font-sans text-sm text-tx-0"
        >
          <TriangleAlert aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--orange-ink)]" />
          <span>{t(`account.reachability.${mine}`)}</span>
        </div>
      )}
      {userId ? (
        <UserNotifyPanel userId={userId} />
      ) : (
        <p className="text-sm text-tx-3">{t('account.unavailable')}</p>
      )}
    </AccountSection>
  );
}
