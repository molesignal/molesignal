import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as authApi from '@/api/auth';
import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { AccountSection } from '@/routes/account/AccountSection';
import { LinkedSignIns } from '@/routes/account/LinkedSignIns';
import { MfaSection } from '@/routes/account/mfa';
import { SecurityRow } from '@/routes/account/SecurityRow';
import { ChromeButton } from '@/shell/chrome';
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
        <LinkedSignIns />
        <MfaSection email={profileQuery.data?.email ?? ''} />
      </div>
    </AccountSection>
  );
}
