import { useTranslation } from 'react-i18next';

import * as auth from '@/api/auth';
import { toApiError } from '@/lib/http';
import { toast } from '@/shell/ui/sonner';

/** Asking for a reset link, and setting a new password with the link's token. */
export function usePasswordRecovery({
  email,
  token,
  password,
  confirmPassword,
  setBusy,
  onSent,
  onReset,
}: {
  email: string;
  token: string;
  password: string;
  confirmPassword: string;
  setBusy: (busy: boolean) => void;
  onSent: () => void;
  onReset: () => void;
}) {
  const { t, i18n } = useTranslation('shell');

  const askForLink = async () => {
    setBusy(true);
    try {
      await auth.forgotPassword({
        email,
        locale: i18n.resolvedLanguage ?? i18n.language,
      });
      onSent();
    } catch (err) {
      toast.error(t('signin.forgot_failed'), { description: toApiError(err).message });
    } finally {
      setBusy(false);
    }
  };

  const setNewPassword = async () => {
    if (password !== confirmPassword) {
      toast.error(t('signin.password_mismatch'));
      return;
    }
    setBusy(true);
    try {
      await auth.resetPassword({ token, password });
      toast.success(t('signin.reset_success'));
      onReset();
    } catch (err) {
      const apiError = toApiError(err);
      toast.error(t('signin.reset_failed'), {
        description: apiError.status === 400 ? t('signin.reset_invalid') : apiError.message,
      });
    } finally {
      setBusy(false);
    }
  };

  return { askForLink, setNewPassword };
}
