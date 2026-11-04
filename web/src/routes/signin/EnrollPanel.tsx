import { useQuery } from '@tanstack/react-query';
import { KeyRound, Smartphone } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MfaEnrollRequired, SigninResult } from '@/api/auth';
import * as mfaApi from '@/api/mfa';
import { toApiError } from '@/lib/http';
import { usePreparedCeremony } from '@/lib/usePreparedCeremony';
import { createPasskey, passkeyFailure } from '@/lib/webauthn';
import { useCodeErrorToast } from '@/routes/account/mfa/CodeField';
import { RecoveryCodesDialog } from '@/routes/account/mfa/RecoveryCodesDialog';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton } from '@/shell/chrome';
import { PulseLine } from '@/shell/PulseLine';
import { BackButton, Field, PrimaryButton } from '@/shell/signin/AuthControls';
import { authenticatorDigits } from '@/shell/signin/MfaPanel';

/**
 * Setting up a second factor on the spot, because the organization being signed in
 * to asks for one and the account has none yet. The session comes with the second
 * factor; the recovery codes are shown before the page moves on, this once.
 */
export function EnrollPanel({
  enroll,
  submitRef,
  onFinished,
  onExpired,
  onBack,
}: {
  enroll: MfaEnrollRequired;
  /** The sign-in card is one form: its submit comes here while this panel shows. */
  submitRef: React.MutableRefObject<(() => void) | null>;
  onFinished: (result: SigninResult) => void;
  /** The sign-in ended: too long a wait, or a second factor was added elsewhere. */
  onExpired: () => void;
  onBack: () => void;
}) {
  const { t } = useTranslation('shell');
  const codeError = useCodeErrorToast();
  const [method, setMethod] = React.useState<'totp' | 'passkey' | null>(
    enroll.methods.length === 1 ? (enroll.methods[0] ?? null) : null,
  );
  const [code, setCode] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [passkeyTries, setPasskeyTries] = React.useState(0);
  const [done, setDone] = React.useState<SigninResult | null>(null);

  const fail = (error: unknown) => {
    const apiError = toApiError(error);

    if (
      passkeyFailure(error) === 'other' &&
      apiError.status === 401 &&
      apiError.message.includes('sign in again')
    ) {
      onExpired();
    } else {
      codeError(error);
    }
  };

  const setup = useQuery({
    queryKey: ['signin', 'enroll', 'totp', enroll.challenge],
    queryFn: () => mfaApi.enrollTotp(enroll.challenge),
    enabled: method === 'totp',
    staleTime: Infinity,
    gcTime: 0,
    retry: false,
  });

  const prepared = usePreparedCeremony(
    () => mfaApi.enrollPasskeyOptions(enroll.challenge),
    method === 'passkey',
    passkeyTries,
    fail,
  );

  React.useEffect(() => {
    if (setup.error) fail(setup.error);
    // Said once for each failed request; `fail` only reads the latest callbacks.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setup.error]);

  const finished = (result: SigninResult) => {
    if (result.recoveryCodes) setDone(result);
    else onFinished(result);
  };

  const submit = async () => {
    if (busy) return;

    if (method === 'totp') {
      if (!setup.data || code.length !== 6) return;
      setBusy(true);

      try {
        finished(
          await mfaApi.confirmEnrolledTotp({
            challenge: enroll.challenge,
            setup: setup.data.setup,
            code,
          }),
        );
      } catch (error) {
        setCode('');
        fail(error);
        // Too many wrong codes, or too long a wait: this key is gone, so show a new one.
        if (toApiError(error).message.includes('start')) void setup.refetch();
      } finally {
        setBusy(false);
      }
    }

    if (method === 'passkey') {
      if (!prepared) return;
      setBusy(true);

      try {
        const credential = await createPasskey(prepared.options);

        finished(
          await mfaApi.enrollPasskey({
            challenge: enroll.challenge,
            ceremony: prepared.ceremony,
            credential,
            name: '',
          }),
        );
      } catch (error) {
        // Backing out of the prompt keeps the ceremony; an answer the server saw used it up.
        if (passkeyFailure(error) === 'other') setPasskeyTries((tries) => tries + 1);
        fail(error);
      } finally {
        setBusy(false);
      }
    }
  };

  submitRef.current = () => void submit();

  const otherMethod = enroll.methods.length > 1 && (
    <div className="flex justify-end font-sans text-xs">
      <button
        type="button"
        className="cursor-pointer text-[var(--blue-ink)] enabled:hover:underline disabled:cursor-not-allowed disabled:text-tx-3"
        disabled={busy}
        onClick={() => {
          setCode('');
          setMethod(null);
        }}
      >
        {t('signin.enroll.other_method')}
      </button>
    </div>
  );

  return (
    <div className="mt-6 flex min-w-0 flex-col gap-4" data-testid="mfa-enroll-panel">
      {method === null && (
        <div className="flex flex-col gap-2">
          {enroll.methods.includes('passkey') && (
            <ChromeButton
              type="button"
              className="w-full min-w-0 justify-center"
              onClick={() => setMethod('passkey')}
            >
              <KeyRound size={14} aria-hidden />
              {t('signin.enroll.use_passkey')}
            </ChromeButton>
          )}
          {enroll.methods.includes('totp') && (
            <ChromeButton
              type="button"
              className="w-full min-w-0 justify-center"
              onClick={() => setMethod('totp')}
            >
              <Smartphone size={14} aria-hidden />
              {t('signin.enroll.use_totp')}
            </ChromeButton>
          )}
        </div>
      )}

      {method === 'totp' && !setup.data && (
        <div className="flex h-44 items-center justify-center">
          <PulseLine variant="travel" className="h-10 w-32 text-tx-3" />
        </div>
      )}

      {method === 'totp' && setup.data && (
        <>
          <div className="font-sans text-xs leading-relaxed text-tx-2">
            {t('signin.enroll.totp_hint')}
          </div>
          {/* White under the code in either theme: scanners read dark on light. */}
          <img
            src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(setup.data.qrSvg)}`}
            alt={t('signin.enroll.qr_alt')}
            className="h-44 w-44 self-center rounded-md bg-white"
            data-testid="mfa-qr"
          />
          <CopyableValue
            value={setup.data.secret}
            copyLabel={t('signin.enroll.copy_secret')}
            copiedLabel={t('signin.enroll.secret_copied')}
          />
          <div className="flex flex-col gap-1.5">
            <Field
              label={t('signin.mfa.code')}
              value={code}
              onChange={(value) => setCode(authenticatorDigits(value))}
              placeholder={t('signin.mfa.code_placeholder')}
              autoComplete="one-time-code"
              inputMode="numeric"
              autoFocus
            />
            {otherMethod}
          </div>
          <PrimaryButton disabled={busy || code.length !== 6} busy={busy}>
            {busy ? t('signin.mfa.verifying') : `${t('signin.enroll.finish_totp')} →`}
          </PrimaryButton>
        </>
      )}

      {method === 'passkey' && (
        <>
          <div className="flex flex-col gap-1.5">
            <div className="font-sans text-xs leading-relaxed text-tx-2">
              {t('signin.enroll.passkey_hint')}
            </div>
            {otherMethod}
          </div>
          <PrimaryButton disabled={busy || !prepared} busy={busy || !prepared}>
            {!busy && prepared && <KeyRound size={14} aria-hidden />}
            {busy ? t('signin.mfa.passkey_waiting') : t('signin.enroll.finish_passkey')}
          </PrimaryButton>
        </>
      )}

      <BackButton onClick={onBack}>{t('signin.back_to_signin')}</BackButton>

      <RecoveryCodesDialog
        codes={done?.recoveryCodes ?? null}
        email={done?.login?.email ?? ''}
        onClose={() => done && onFinished(done)}
      />
    </div>
  );
}
