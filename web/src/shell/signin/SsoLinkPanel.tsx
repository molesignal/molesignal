import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { SsoLinkRequired } from '@/api/sso';
import { SegmentedControl } from '@/shell/PreferencesFields/controls';

import { BackButton, Field, PrimaryButton } from './AuthControls';

export type SsoLinkMethod = 'password' | 'email';

/** The server sends one code a minute per account; the resend button waits as long. */
const RESEND_SECONDS = 60;

export interface SsoLinkPanelProps {
  link: SsoLinkRequired;
  method: SsoLinkMethod;
  onMethodChange: (method: SsoLinkMethod) => void;
  emailCode: string;
  onEmailCodeChange: (value: string) => void;
  password: string;
  onPasswordChange: (value: string) => void;
  /** When the last code was sent (epoch ms); null until the first one is. */
  codeSentAt: number | null;
  /** A code is being sent. The first one is sent by the form's submit button. */
  sending: boolean;
  onResendCode: () => void;
  busy: boolean;
  onBack: () => void;
}

/** How the account can be confirmed, the quicker way first. */
export function linkMethods(link: SsoLinkRequired): SsoLinkMethod[] {
  return [
    ...(link.passwordAllowed ? (['password'] as const) : []),
    ...(link.emailAvailable ? (['email'] as const) : []),
  ];
}

/**
 * The owner of an existing account confirms a first single sign-on into it, with the
 * account password or a code emailed to the account. The code only works on this page,
 * so a link someone else started cannot be confirmed from the email.
 */
export function SsoLinkPanel({
  link,
  method,
  onMethodChange,
  emailCode,
  onEmailCodeChange,
  password,
  onPasswordChange,
  codeSentAt,
  sending,
  onResendCode,
  busy,
  onBack,
}: SsoLinkPanelProps) {
  const { t } = useTranslation(['shell', 'common']);
  const methods = linkMethods(link);
  const resendIn = useSecondsLeft(codeSentAt, RESEND_SECONDS);

  if (methods.length === 0) {
    return (
      <div className="mt-6 flex flex-col gap-4" data-testid="sso-link-panel">
        <div role="alert" className="font-sans text-xs leading-relaxed text-tx-1">
          {t('shell:signin.link.unavailable')}
        </div>
        <BackButton onClick={onBack}>{t('shell:signin.back_to_signin')}</BackButton>
      </div>
    );
  }

  // With a choice of methods, the choice names the field under it.
  const named = methods.length > 1;
  const codeSent = codeSentAt !== null;
  const sendStep = method === 'email' && !codeSent;
  const disabled =
    busy ||
    sending ||
    (method === 'password' ? !password : codeSent && emailCode.length !== 6);
  const action = sendStep
    ? sending
      ? t('shell:signin.link.sending')
      : `${t('shell:signin.link.send_code')} →`
    : busy
      ? t('shell:signin.signing_in')
      : `${t('shell:signin.link.confirm')} →`;

  return (
    <div className="mt-6 flex flex-col gap-4" data-testid="sso-link-panel">
      {methods.length > 1 && (
        <SegmentedControl
          value={method}
          onChange={onMethodChange}
          ariaLabel={t('shell:signin.link.method')}
          options={methods.map((value) => ({
            value,
            label: t(`shell:signin.link.methods.${value}`),
          }))}
        />
      )}

      {method === 'password' && (
        <Field
          label={t('common:labels.password')}
          value={password}
          onChange={onPasswordChange}
          placeholder={t('shell:signin.password_placeholder')}
          type="password"
          autoComplete="current-password"
          hideLabel={named}
          autoFocus
        />
      )}

      {sendStep && (
        <div className="font-sans text-xs leading-relaxed text-tx-2">
          {t('shell:signin.link.code_target')}
        </div>
      )}

      {method === 'email' && codeSent && (
        <div className="flex flex-col gap-1.5">
          <Field
            label={t('shell:signin.link.code')}
            value={emailCode}
            // A pasted "123 456" keeps its six digits: no length cap before the clean-up.
            onChange={(value) => onEmailCodeChange(value.replace(/\D/g, '').slice(0, 6))}
            placeholder={t('shell:signin.link.code_placeholder')}
            autoComplete="one-time-code"
            inputMode="numeric"
            hideLabel={named}
            autoFocus
          />
          <div className="flex items-center justify-between gap-3 font-sans text-xs leading-relaxed text-tx-3">
            <span className="min-w-0">{t('shell:signin.link.code_sent')}</span>
            <button
              type="button"
              className="shrink-0 cursor-pointer text-[var(--blue-ink)] tabular-nums enabled:hover:underline disabled:cursor-not-allowed disabled:text-tx-3"
              onClick={onResendCode}
              disabled={busy || sending || resendIn > 0}
            >
              {resendIn > 0
                ? t('shell:signin.link.resend_in', { seconds: resendIn })
                : t('shell:signin.link.resend')}
            </button>
          </div>
        </div>
      )}

      <PrimaryButton disabled={disabled} busy={busy || (sendStep && sending)}>
        {action}
      </PrimaryButton>

      <BackButton onClick={onBack}>{t('shell:signin.back_to_signin')}</BackButton>
    </div>
  );
}

/** Whole seconds until `seconds` have passed since `since`; 0 when there is nothing to wait for. */
function useSecondsLeft(since: number | null, seconds: number): number {
  const [now, setNow] = React.useState(() => Date.now());

  React.useEffect(() => {
    if (since === null) return;
    const end = since + seconds * 1000;
    const tick = () => {
      const current = Date.now();
      setNow(current);
      if (current >= end) window.clearInterval(timer);
    };
    const timer = window.setInterval(tick, 1000);
    tick();
    return () => window.clearInterval(timer);
  }, [since, seconds]);

  if (since === null) return 0;
  // Until the first tick `now` can be older than `since`; the wait is never longer than `seconds`.
  return Math.min(seconds, Math.max(0, Math.ceil((since + seconds * 1000 - now) / 1000)));
}
