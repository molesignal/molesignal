import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as auth from '@/api/auth';
import { toApiError } from '@/lib/http';
import { usePreparedCeremony } from '@/lib/usePreparedCeremony';
import { getPasskey, passkeyFailure } from '@/lib/webauthn';
import { toast } from '@/shell/ui/sonner';

import { type MfaPanelProps, preferredMethod } from './MfaPanel';

/** A sign-in past its first step, waiting for the account's second factor. */
interface PendingMfa<Options> {
  challenge: string;
  methods: auth.MfaMethod[];
  options: Options;
}

/**
 * The second step of a sign-in: which factor is being used, the code typed so far, the passkey
 * ceremony prepared ahead of the button, and what the answer means. `active` while it's on screen.
 */
export function useSecondStep<Options>({
  active,
  busy,
  setBusy,
  onSignedIn,
  onExpired,
}: {
  active: boolean;
  busy: boolean;
  setBusy: (busy: boolean) => void;
  onSignedIn: (login: auth.SigninResponse, options: Options) => Promise<void>;
  /** Expired, or too many wrong codes: the password is asked for again. */
  onExpired: () => void;
}) {
  const { t } = useTranslation(['errors', 'shell']);
  const [mfa, setMfa] = React.useState<PendingMfa<Options> | null>(null);
  const [mfaMethod, setMfaMethod] = React.useState<auth.MfaMethod>('totp');
  const [mfaCode, setMfaCode] = React.useState('');
  const [mfaPasskeyTries, setMfaPasskeyTries] = React.useState(0);

  const mfaPasskey = usePreparedCeremony(
    () => auth.secondStepPasskey(mfa?.challenge ?? ''),
    active && mfaMethod === 'passkey' && mfa !== null,
    mfaPasskeyTries,
    (error) =>
      toast.error(t('errors:sign_in_failed'), { description: toApiError(error).message }),
  );

  const finishMfa = async (proof: auth.FactorProof) => {
    if (!mfa) return;
    try {
      const res = await auth.verifySignin({ challenge: mfa.challenge, ...proof });
      const left = res.recoveryCodesLeft;
      if (typeof left === 'number') {
        toast.warning(
          left > 0
            ? t('shell:signin.mfa.recovery_left', { count: left })
            : t('shell:signin.mfa.recovery_none_left'),
        );
      }
      if (res.login) await onSignedIn(res.login, mfa.options);
    } catch (err) {
      const apiError = toApiError(err);
      setMfaCode('');
      // The ceremony was used up, whatever the answer was.
      setMfaPasskeyTries((count) => count + 1);
      if (apiError.status === 429) {
        toast.error(t('errors:sign_in_failed'), { description: t('shell:signin.mfa.locked') });
      } else if (apiError.status === 401 && apiError.message.includes('sign in again')) {
        toast.error(t('errors:sign_in_failed'), { description: t('shell:signin.mfa.expired') });
        setMfa(null);
        onExpired();
      } else if (apiError.status === 401) {
        toast.error(
          proof.method === 'passkey'
            ? t('shell:signin.mfa.passkey_failed')
            : t('shell:signin.mfa.wrong_code'),
        );
      } else {
        toast.error(t('errors:sign_in_failed'), { description: apiError.message });
      }
    }
  };

  const submitMfa = async (code: string = mfaCode) => {
    if (mfaMethod === 'passkey') return;
    setBusy(true);
    await finishMfa({ method: mfaMethod, code });
    setBusy(false);
  };

  const submitMfaPasskey = async () => {
    if (!mfaPasskey) return;
    setBusy(true);
    try {
      const credential = await getPasskey(mfaPasskey.options);
      await finishMfa({ method: 'passkey', ceremony: mfaPasskey.ceremony, credential });
    } catch (err) {
      // Backing out of the browser's prompt leaves the choice open, nothing to say.
      const failure = passkeyFailure(err);
      if (failure !== 'cancelled') {
        toast.error(t('errors:sign_in_failed'), {
          description:
            failure === 'wrong_address'
              ? t('shell:signin.passkey_wrong_address')
              : toApiError(err).message,
        });
      }
    } finally {
      setBusy(false);
    }
  };

  const changeMfaCode = (value: string) => {
    setMfaCode(value);
    // A whole authenticator code is sent straight away.
    if (mfaMethod === 'totp' && value.length === 6 && value !== mfaCode && !busy) {
      void submitMfa(value);
    }
  };

  const panel: Omit<MfaPanelProps, 'busy' | 'onBack'> | null = mfa && {
    methods: mfa.methods,
    method: mfaMethod,
    onMethodChange: setMfaMethod,
    code: mfaCode,
    onCodeChange: changeMfaCode,
    passkeyReady: mfaPasskey !== null,
  };

  return {
    panel,
    open: (required: auth.MfaRequired, options: Options) => {
      setMfaCode('');
      setMfaMethod(preferredMethod(required.methods));
      setMfa({ challenge: required.challenge, methods: required.methods, options });
    },
    reset: () => {
      setMfa(null);
      setMfaCode('');
    },
    submit: () => void (mfaMethod === 'passkey' ? submitMfaPasskey() : submitMfa()),
  };
}
