import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';
import { toApiError } from '@/lib/http';
import { passkeyFailure } from '@/lib/webauthn';
import { FormField } from '@/shell/FormDrawer';
import {
  authenticatorDigits,
  OtherMethods,
  recoveryCodeComplete,
} from '@/shell/signin/MfaPanel';
import { signinLimitWait } from '@/shell/signin/signinLimit';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';

/** The code is whole: six digits, or the ten characters of a recovery code. */
export function codeComplete(method: MfaMethod, code: string): boolean {
  return method === 'totp' ? code.length === 6 : recoveryCodeComplete(code);
}

/**
 * The second factor that confirms a change: a code from the authenticator app, one
 * of the recovery codes, or a passkey, which only needs the dialog's button. The
 * other `methods` the account has are offered as links.
 */
export function CodeField({
  methods,
  method,
  onMethodChange,
  code,
  onCodeChange,
  disabled = false,
}: {
  methods: MfaMethod[];
  method: MfaMethod;
  onMethodChange: (method: MfaMethod) => void;
  code: string;
  onCodeChange: (code: string) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation('account');
  const totp = method === 'totp';

  return (
    <div className="flex flex-col gap-1.5">
      {method === 'passkey' ? (
        <div className="font-sans text-xs leading-relaxed text-tx-2">
          {t('security.mfa.passkey_hint')}
        </div>
      ) : (
        <FormField label={totp ? t('security.mfa.code') : t('security.mfa.recovery_code')}>
          <Input
            key={method}
            value={code}
            onChange={(event) =>
              onCodeChange(totp ? authenticatorDigits(event.target.value) : event.target.value)
            }
            placeholder={
              totp ? t('security.mfa.code_placeholder') : t('security.mfa.recovery_placeholder')
            }
            autoComplete={totp ? 'one-time-code' : 'off'}
            inputMode={totp ? 'numeric' : 'text'}
            className="tabular-nums"
            disabled={disabled}
            autoFocus
            data-testid="mfa-code-input"
          />
        </FormField>
      )}
      <OtherMethods
        methods={methods}
        current={method}
        disabled={disabled}
        onChange={(next) => {
          onCodeChange('');
          onMethodChange(next);
        }}
      />
    </div>
  );
}

/**
 * Says why a change that asked for a confirmation did not happen: a wrong code,
 * password or passkey, a lock after too many wrong codes, or whatever else the
 * browser or the server answered. Backing out of the browser's prompt says nothing.
 */
export function useCodeErrorToast() {
  const { t } = useTranslation(['account', 'shell']);

  return (error: unknown) => {
    const failure = passkeyFailure(error);

    if (failure === 'cancelled') return;

    if (failure === 'wrong_address') {
      toast.error(t('account:security.passkeys.wrong_address'));
      return;
    }

    if (failure === 'already_added') {
      toast.error(t('account:security.passkeys.already_added'));
      return;
    }

    const apiError = toApiError(error);
    const wait = signinLimitWait(apiError, t);

    if (wait) {
      toast.error(wait);
    } else if (apiError.status === 429) {
      toast.error(t('shell:signin.mfa.locked'));
    } else if (apiError.status === 400 && apiError.message.includes('password is not correct')) {
      toast.error(t('account:security.mfa.wrong_password'));
    } else if (apiError.status === 400 && apiError.message.includes('not correct')) {
      toast.error(t('account:security.mfa.wrong_code'));
    } else if (apiError.status === 400 && apiError.message.includes('passkey could not')) {
      toast.error(t('account:security.mfa.passkey_failed'));
    } else if (apiError.status === 400 && apiError.message.includes('sign in again')) {
      toast.error(t('account:security.mfa.sign_in_again'));
    } else {
      toast.error(t('account:security.mfa.action_failed'), { description: apiError.message });
    }
  };
}
