import { MonitorSmartphone } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { AccountSection } from '@/routes/account/AccountSection';
import { ChromeButton, Pill } from '@/shell/chrome';
import { useAuthStore } from '@/stores/auth';
import { useOrgStore } from '@/stores/useOrgStore';

function browserSummary(): string {
  const agent = navigator.userAgent;
  const browser = agent.includes('Edg/')
    ? 'Edge'
    : agent.includes('Chrome/')
      ? 'Chrome'
      : agent.includes('Firefox/')
        ? 'Firefox'
        : agent.includes('Safari/')
          ? 'Safari'
          : 'Browser';
  const os = agent.includes('Mac OS')
    ? 'macOS'
    : agent.includes('Windows')
      ? 'Windows'
      : agent.includes('Android')
        ? 'Android'
        : agent.includes('iPhone') || agent.includes('iPad')
          ? 'iOS'
          : agent.includes('Linux')
            ? 'Linux'
            : '';
  return [browser, os].filter(Boolean).join(' · ');
}

export function AccountSessions() {
  const { t } = useTranslation('account');
  const navigate = useNavigate();
  const logout = useAuthStore((state) => state.logout);
  const signOut = () => {
    logout();
    useOrgStore.getState().reset();
    navigate('/signin');
  };

  return (
    <AccountSection
      title={t('sessions.title')}
      subtitle={t('sessions.subtitle')}
    >
      <div className="flex items-center gap-4 pb-2">
        <div className="grid h-10 w-10 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2">
          <MonitorSmartphone className="h-4 w-4 text-tx-2" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="font-sans text-sm font-strong text-tx-0">
            {browserSummary()}
          </div>
          <div className="mt-1 font-sans text-xs text-tx-2">
            {t('sessions.current_device_description')}
          </div>
        </div>
        <Pill tone="green">{t('sessions.current')}</Pill>
      </div>
      <div className="mt-5 flex flex-wrap items-center justify-between gap-4">
        <p className="max-w-xl font-sans text-xs leading-relaxed text-tx-3">
          {t('sessions.current_only')}
        </p>
        <ChromeButton onClick={signOut}>{t('sessions.sign_out')}</ChromeButton>
      </div>
    </AccountSection>
  );
}
