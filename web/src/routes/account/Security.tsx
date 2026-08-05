import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as authApi from '@/api/auth';
import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { AccountSection } from '@/routes/account/AccountSection';
import { ChromeButton, Pill } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

export function AccountSecurity() {
  const { t, i18n } = useTranslation(['account', 'common']);
  const profileQuery = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
  });
  const resetPassword = useMutation({
    mutationFn: () =>
      authApi.forgotPassword({
        email: profileQuery.data?.email ?? '',
        locale: i18n.resolvedLanguage ?? i18n.language,
      }),
    onSuccess: () => toast.success(t('account:security.reset_sent')),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const passwordDisabled =
    profileQuery.isLoading ||
    profileQuery.isError ||
    !profileQuery.data ||
    resetPassword.isPending;
  const passwordDisabledReason = profileQuery.isLoading
    ? t('common:access.loading')
    : profileQuery.isError
      ? t('common:access.page_unavailable')
      : resetPassword.isPending
        ? t('common:access.operation_pending')
        : undefined;

  return (
    <AccountSection
      title={t('account:security.title')}
      subtitle={t('account:security.subtitle')}
    >
      <div>
        <SecurityRow
          label={t('account:security.password')}
          description={t('account:security.password_description')}
          action={
            <ChromeButton
              disabled={passwordDisabled}
              disabledReason={passwordDisabledReason}
              onClick={() => resetPassword.mutate()}
            >
              {resetPassword.isPending
                ? t('common:status.loading')
                : t('account:security.change_password')}
            </ChromeButton>
          }
        />
        <SecurityRow
          label={t('account:security.mfa')}
          description={t('account:security.mfa_description')}
          action={
            <span aria-readonly="true">
              <Pill tone="dim">{t('account:security.not_configured')}</Pill>
            </span>
          }
        />
        <SecurityRow
          label={t('account:security.passkeys')}
          description={t('account:security.passkeys_description')}
          action={
            <span aria-readonly="true">
              <Pill tone="dim">{t('account:security.not_configured')}</Pill>
            </span>
          }
        />
      </div>
    </AccountSection>
  );
}

function SecurityRow({
  label,
  description,
  action,
}: {
  label: string;
  description: string;
  action: React.ReactNode;
}) {
  return (
    <div className="grid min-h-20 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <div>
        <div className="font-sans text-sm font-strong text-tx-0">{label}</div>
        <div className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-2">
          {description}
        </div>
      </div>
      <div>{action}</div>
    </div>
  );
}
