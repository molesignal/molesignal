import { KeyRound } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';

import { BackButton, Field, PrimaryButton } from './AuthControls';

export interface MfaPanelProps {
  /** What this account can use, from the server. */
  methods: MfaMethod[];
  method: MfaMethod;
  onMethodChange: (method: MfaMethod) => void;
  code: string;
  onCodeChange: (value: string) => void;
  /** The browser can be asked for the passkey: the ceremony has arrived. */
  passkeyReady: boolean;
  busy: boolean;
  onBack: () => void;
}

/** The six digits of an authenticator code: a pasted "123 456" keeps them. */
export function authenticatorDigits(value: string): string {
  return value.replace(/\D/g, '').slice(0, 6);
}

/** A recovery code is ten letters and digits, however it was typed. */
export function recoveryCodeComplete(value: string): boolean {
  return value.replace(/[\s-]/g, '').length === 10;
}

/** What the second step offers first: a passkey asks least, and a recovery code is the last resort. */
export function preferredMethod(methods: MfaMethod[]): MfaMethod {
  for (const method of ['passkey', 'totp'] as const) {
    if (methods.includes(method)) return method;
  }
  return 'recovery_code';
}

/** The other ways to verify, each as a link; used wherever a second factor is asked for. */
export function OtherMethods({
  methods,
  current,
  disabled,
  onChange,
}: {
  methods: MfaMethod[];
  current: MfaMethod;
  disabled: boolean;
  onChange: (method: MfaMethod) => void;
}) {
  const { t } = useTranslation('shell');
  const labels: Record<MfaMethod, string> = {
    totp: t('signin.mfa.use_totp'),
    recovery_code: t('signin.mfa.use_recovery'),
    passkey: t('signin.mfa.use_passkey'),
  };

  return (
    <div className="flex flex-wrap justify-end gap-x-4 gap-y-1 font-sans text-xs">
      {methods
        .filter((method) => method !== current)
        .map((method) => (
          <button
            key={method}
            type="button"
            className="cursor-pointer text-[var(--blue-ink)] enabled:hover:underline disabled:cursor-not-allowed disabled:text-tx-3"
            onClick={() => onChange(method)}
            disabled={disabled}
          >
            {labels[method]}
          </button>
        ))}
    </div>
  );
}

/**
 * The second step of signing in to an account with multi-factor authentication:
 * a passkey, a code from the authenticator app, or one of the recovery codes.
 */
export function MfaPanel({
  methods,
  method,
  onMethodChange,
  code,
  onCodeChange,
  passkeyReady,
  busy,
  onBack,
}: MfaPanelProps) {
  const { t } = useTranslation('shell');
  const passkey = method === 'passkey';
  const preparing = passkey && !passkeyReady;
  const complete = passkey
    ? passkeyReady
    : method === 'totp'
      ? code.length === 6
      : recoveryCodeComplete(code);

  const switchMethod = (next: MfaMethod) => {
    onCodeChange('');
    onMethodChange(next);
  };

  return (
    <div className="mt-6 flex flex-col gap-4" data-testid="mfa-panel">
      <div className="flex flex-col gap-1.5">
        {method === 'totp' && (
          <Field
            key="totp"
            label={t('signin.mfa.code')}
            value={code}
            onChange={(value) => onCodeChange(authenticatorDigits(value))}
            placeholder={t('signin.mfa.code_placeholder')}
            autoComplete="one-time-code"
            inputMode="numeric"
            autoFocus
          />
        )}
        {method === 'recovery_code' && (
          <Field
            key="recovery"
            label={t('signin.mfa.recovery_code')}
            value={code}
            onChange={onCodeChange}
            placeholder={t('signin.mfa.recovery_placeholder')}
            autoComplete="off"
            autoFocus
          />
        )}
        <OtherMethods methods={methods} current={method} disabled={busy} onChange={switchMethod} />
      </div>

      <PrimaryButton disabled={busy || !complete} busy={busy || preparing}>
        {passkey && !busy && !preparing && <KeyRound size={14} aria-hidden />}
        {!passkey && (busy ? t('signin.mfa.verifying') : `${t('signin.mfa.verify')} →`)}
        {passkey && (busy ? t('signin.mfa.passkey_waiting') : t('signin.mfa.verify_passkey'))}
      </PrimaryButton>

      <BackButton onClick={onBack}>{t('signin.back_to_signin')}</BackButton>
    </div>
  );
}
