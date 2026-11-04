import { useMutation } from '@tanstack/react-query';
import { MonitorSmartphone } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as mfaApi from '@/api/mfa';
import { toApiError } from '@/lib/http';
import { AccountSection } from '@/routes/account/AccountSection';
import { ChromeButton, Pill } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { signOut } from '@/stores/auth/signOut';

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
  const signOutHere = () => {
    void signOut().then(() => navigate('/signin'));
  };
  const signOutOthers = useMutation({
    mutationFn: () => mfaApi.revokeOtherSessions(),
    onSuccess: () => toast.success(t('sessions.signed_out_others')),
    onError: (error) =>
      toast.error(t('sessions.sign_out_others_failed'), {
        description: toApiError(error).message,
      }),
  });

  return (
    <AccountSection
      title={t('sessions.title')}
      subtitle={t('sessions.subtitle')}
    >
      <div className="flex items-center gap-4 pb-2">
        <div className="grid h-10 w-10 shrink-0 place-items-center rounded-md bg-bg-2">
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
          {t('sessions.others_description')}
        </p>
        <div className="flex flex-wrap gap-2">
          <ChromeButton
            disabled={signOutOthers.isPending}
            onClick={() => signOutOthers.mutate()}
            data-testid="sign-out-others"
          >
            {signOutOthers.isPending
              ? t('sessions.signing_out_others')
              : t('sessions.sign_out_others')}
          </ChromeButton>
          <ChromeButton onClick={signOutHere}>{t('sessions.sign_out')}</ChromeButton>
        </div>
      </div>
    </AccountSection>
  );
}
