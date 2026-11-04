import { MailCheck } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { BackButton, Field, PrimaryButton } from './AuthControls';
import { AuthStage } from './AuthStage';
import { BrandHeader } from './BrandHeader';
import { MfaPanel, type MfaPanelProps } from './MfaPanel';
import { SigninSteps, type SigninStepsProps, useStepHeading } from './SigninSteps';
import { SsoLinkPanel, type SsoLinkPanelProps } from './SsoLinkPanel';

export type SigninView =
  | 'signin'
  | 'forgot-password'
  | 'forgot-password-sent'
  | 'reset-password'
  | 'sso-link'
  | 'mfa'
  | 'mfa-enroll';

/**
 * SigninShell is the public-facing authentication surface: the brand and one
 * sentence above a single form card, the product version underneath, and
 * nothing else. It is purely presentational; the `/signin` route owns auth
 * state and side effects.
 */
export interface SigninShellProps {
  view: SigninView;
  email: string;
  onEmailChange: (value: string) => void;
  password: string;
  onPasswordChange: (value: string) => void;
  confirmPassword: string;
  onConfirmPasswordChange: (value: string) => void;
  remember: boolean;
  onRememberChange: (value: boolean) => void;
  busy: boolean;
  onSubmit: (event: React.FormEvent) => void;
  onOpenForgotPassword: () => void;
  onBackToSignin: () => void;
  /** The sign-in view's step and the ways in it lists. */
  signin: Pick<
    SigninStepsProps,
    'step' | 'rows' | 'canGoBack' | 'onBack' | 'onChoose' | 'passkeyBusy' | 'passkeyAutofill'
  >;
  /** Why the last redirect sign-in (OIDC, OAuth 2.0, SAML) failed. */
  ssoError?: string | null;
  /** The confirmation a first single sign-on into an existing account waits for. */
  ssoLink?: Omit<SsoLinkPanelProps, 'busy' | 'onBack'> | null;
  /** The second step of an account with multi-factor authentication. */
  mfa?: Omit<MfaPanelProps, 'busy' | 'onBack'> | null;
  /** Setting up a second factor on the spot, for the organization that asks for one. */
  enroll?: { organizationName: string; panel: React.ReactNode } | null;
  /** Product version only; the deployment's release channel is not shown here. */
  buildVersion?: string | undefined;
  signupEnabled?: boolean;
  onSignup?: () => void;
}

export function SigninShell({
  view,
  email,
  onEmailChange,
  password,
  onPasswordChange,
  confirmPassword,
  onConfirmPasswordChange,
  remember,
  onRememberChange,
  busy,
  onSubmit,
  onOpenForgotPassword,
  onBackToSignin,
  signin,
  ssoError,
  ssoLink,
  mfa,
  enroll,
  buildVersion,
  signupEnabled,
  onSignup,
}: SigninShellProps) {
  const { t } = useTranslation(['shell', 'common', 'errors']);
  const passwordMismatch = Boolean(confirmPassword) && password !== confirmPassword;
  const shortPassword = Boolean(password) && [...password].length < 8;
  const version = buildVersion?.trim();
  const stepHeading = useStepHeading(signin.step);
  const submitDisabled =
    busy ||
    (view === 'forgot-password' && !email) ||
    (view === 'reset-password' &&
      (!password || !confirmPassword || passwordMismatch || shortPassword));

  const heading =
    view === 'signin'
      ? stepHeading.title
      : view === 'forgot-password'
        ? t('shell:signin.forgot_title')
        : view === 'forgot-password-sent'
          ? t('shell:signin.forgot_sent_title')
          : view === 'sso-link'
            ? t('shell:signin.link.title')
            : view === 'mfa'
              ? t('shell:signin.mfa.title')
              : view === 'mfa-enroll'
                ? t('shell:signin.enroll.title')
                : t('shell:signin.reset_title');
  const subtitle =
    view === 'signin'
      ? stepHeading.subtitle
      : view === 'forgot-password'
        ? t('shell:signin.forgot_subtitle')
        : view === 'forgot-password-sent'
          ? t('shell:signin.forgot_sent_subtitle', { email })
          : view === 'sso-link'
            ? t('shell:signin.link.subtitle', {
                email: ssoLink?.link.emailHint ?? '',
                provider: ssoLink?.link.providerName ?? '',
                organization: ssoLink?.link.organizationName ?? '',
              })
            : view === 'mfa'
              ? mfa?.method === 'recovery_code'
                ? t('shell:signin.mfa.subtitle_recovery')
                : mfa?.method === 'passkey'
                  ? t('shell:signin.mfa.subtitle_passkey')
                  : t('shell:signin.mfa.subtitle_totp')
              : view === 'mfa-enroll'
                ? t('shell:signin.enroll.subtitle', { organization: enroll?.organizationName ?? '' })
                : t('shell:signin.reset_subtitle');

  return (
    <AuthStage>
      <BrandHeader />

      <form
        onSubmit={onSubmit}
        className="ms-auth-card mt-9 flex w-full min-w-0 flex-col rounded-2xl border border-bd-0 bg-bg-1 p-6 shadow-login sm:p-8"
        data-testid="signin-card"
      >
        <h1 className="m-0 font-sans text-xl font-display tracking-tight text-tx-0">{heading}</h1>
        <div className="mt-1.5 font-sans text-xs leading-6 text-tx-2">{subtitle}</div>

        {view === 'signin' && ssoError && (
          <div
            role="alert"
            className="mt-5 rounded-md bg-red-dim px-3 py-2.5 font-sans text-xs leading-relaxed text-red-soft"
            data-testid="signin-sso-error"
          >
            <div className="font-strong">{t('errors:sign_in_failed')}</div>
            <div className="mt-0.5 break-words">{ssoError}</div>
          </div>
        )}

        {view === 'signin' && (
          <SigninSteps
            {...signin}
            email={email}
            onEmailChange={onEmailChange}
            password={password}
            onPasswordChange={onPasswordChange}
            remember={remember}
            onRememberChange={onRememberChange}
            busy={busy}
            onOpenForgotPassword={onOpenForgotPassword}
          />
        )}

        {view === 'forgot-password' && (
          <div className="mt-6 flex flex-col gap-4">
            <Field
              label={t('common:labels.email')}
              value={email}
              onChange={onEmailChange}
              placeholder={t('shell:signin.email_placeholder')}
              type="email"
              autoComplete="email"
              autoFocus
            />
            <div className="font-sans text-xs leading-relaxed text-tx-3">
              {t('shell:signin.forgot_privacy_hint')}
            </div>
            <PrimaryButton disabled={submitDisabled} busy={busy}>
              {busy ? t('shell:signin.sending_reset') : `${t('shell:signin.send_reset')} →`}
            </PrimaryButton>
            <BackButton onClick={onBackToSignin}>{t('shell:signin.back_to_signin')}</BackButton>
          </div>
        )}

        {view === 'forgot-password-sent' && (
          <div className="mt-6 flex flex-col">
            <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-indigo-dim text-indigo-soft">
              <MailCheck size={20} strokeWidth={1.8} aria-hidden />
            </div>
            <div className="mt-4 font-sans text-xs leading-relaxed text-tx-1">
              {t('shell:signin.forgot_sent_detail')}
            </div>
            <BackButton onClick={onBackToSignin} className="mt-6">
              {t('shell:signin.back_to_signin')}
            </BackButton>
          </div>
        )}

        {view === 'reset-password' && (
          <div className="mt-6 flex flex-col gap-4">
            <Field
              label={t('shell:signin.new_password')}
              value={password}
              onChange={onPasswordChange}
              placeholder={t('shell:signin.password_placeholder')}
              type="password"
              autoComplete="new-password"
              minLength={8}
              maxLength={256}
              autoFocus
            />
            <Field
              label={t('shell:signin.confirm_password')}
              value={confirmPassword}
              onChange={onConfirmPasswordChange}
              placeholder={t('shell:signin.password_placeholder')}
              type="password"
              autoComplete="new-password"
              minLength={8}
              maxLength={256}
            />
            <div
              className={`font-sans text-xs leading-relaxed ${
                passwordMismatch || shortPassword ? 'text-red-soft' : 'text-tx-3'
              }`}
              role={passwordMismatch || shortPassword ? 'alert' : undefined}
            >
              {passwordMismatch
                ? t('shell:signin.password_mismatch')
                : t('shell:signin.password_hint')}
            </div>
            <PrimaryButton disabled={submitDisabled} busy={busy}>
              {busy
                ? t('shell:signin.resetting_password')
                : `${t('shell:signin.reset_password')} →`}
            </PrimaryButton>
            <BackButton onClick={onBackToSignin}>{t('shell:signin.back_to_signin')}</BackButton>
          </div>
        )}

        {view === 'sso-link' && ssoLink && (
          <SsoLinkPanel {...ssoLink} busy={busy} onBack={onBackToSignin} />
        )}

        {view === 'mfa' && mfa && <MfaPanel {...mfa} busy={busy} onBack={onBackToSignin} />}

        {view === 'mfa-enroll' && enroll?.panel}
      </form>

      {view === 'signin' && signupEnabled && (
        <p
          className="ms-auth-rise mt-6 font-sans text-xs text-tx-2"
          style={{ '--i': 5 } as React.CSSProperties}
        >
          {t('shell:signin.no_account')}{' '}
          <button
            type="button"
            className="cursor-pointer text-[var(--blue-ink)] hover:underline"
            onClick={onSignup}
          >
            {t('shell:signin.signup_link')}
          </button>
        </p>
      )}

      {version ? (
        <p
          className="ms-auth-fade mt-8 font-sans text-xs tabular-nums text-tx-3"
          data-testid="signin-build-info"
        >
          {t('shell:signin.version', { version })}
        </p>
      ) : null}
    </AuthStage>
  );
}
