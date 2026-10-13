import { MailCheck } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PublicSsoProvider } from '@/api/sso';
import { ChromeButton } from '@/shell/chrome';
import { Checkbox } from '@/shell/ui/checkbox';

import { BackButton, Field, PrimaryButton } from './AuthControls';
import { AuthStage } from './AuthStage';
import { BrandHeader } from './BrandHeader';

export type SigninView = 'signin' | 'forgot-password' | 'forgot-password-sent' | 'reset-password';

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
  ssoProviders: PublicSsoProvider[];
  onBeginSso: (provider: PublicSsoProvider) => void;
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
  ssoProviders,
  onBeginSso,
  buildVersion,
  signupEnabled,
  onSignup,
}: SigninShellProps) {
  const { t } = useTranslation(['shell', 'common']);
  const passwordMismatch = Boolean(confirmPassword) && password !== confirmPassword;
  const shortPassword = Boolean(password) && [...password].length < 8;
  const hasLdapProvider = ssoProviders.some((provider) => provider.kind === 'ldap');
  const version = buildVersion?.trim();
  const submitDisabled =
    busy ||
    (view === 'signin' && (!email || !password)) ||
    (view === 'forgot-password' && !email) ||
    (view === 'reset-password' &&
      (!password || !confirmPassword || passwordMismatch || shortPassword));

  const heading =
    view === 'signin'
      ? t('common:actions.sign_in')
      : view === 'forgot-password'
        ? t('shell:signin.forgot_title')
        : view === 'forgot-password-sent'
          ? t('shell:signin.forgot_sent_title')
          : t('shell:signin.reset_title');
  const subtitle =
    view === 'signin'
      ? t('shell:signin.subtitle')
      : view === 'forgot-password'
        ? t('shell:signin.forgot_subtitle')
        : view === 'forgot-password-sent'
          ? t('shell:signin.forgot_sent_subtitle', { email })
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
        <div className="mt-1.5 font-sans text-xs leading-relaxed text-tx-2">{subtitle}</div>

        {view === 'signin' && (
          <div className="mt-6 flex flex-col gap-4">
            <Field
              label={
                hasLdapProvider ? t('shell:signin.email_or_username') : t('common:labels.email')
              }
              value={email}
              onChange={onEmailChange}
              placeholder={t('shell:signin.email_placeholder')}
              type={hasLdapProvider ? 'text' : 'email'}
              autoComplete="username"
              autoFocus
            />
            <Field
              label={t('common:labels.password')}
              value={password}
              onChange={onPasswordChange}
              placeholder={t('shell:signin.password_placeholder')}
              type="password"
              autoComplete="current-password"
            />

            <div className="flex items-center justify-between font-sans text-xs">
              <label className="flex items-center gap-1.5 text-tx-1">
                <Checkbox
                  checked={remember}
                  onCheckedChange={(checked) => onRememberChange(checked === true)}
                />
                {t('common:labels.remember_me')}
              </label>
              <button
                type="button"
                className="cursor-pointer text-[var(--blue-ink)] hover:underline"
                onClick={onOpenForgotPassword}
              >
                {t('shell:signin.forgot_password')}
              </button>
            </div>

            <PrimaryButton disabled={submitDisabled} busy={busy}>
              {busy ? t('shell:signin.signing_in') : `${t('common:actions.sign_in')} →`}
            </PrimaryButton>

            {ssoProviders.length > 0 && (
              <>
                <div className="flex items-center gap-2.5 font-sans text-xs text-tx-2">
                  <div className="h-px flex-1 bg-bd-0" />
                  {t('shell:signin.or')}
                  <div className="h-px flex-1 bg-bd-0" />
                </div>
                <div className="flex flex-wrap gap-2">
                  {ssoProviders.map((provider) => (
                    <ChromeButton
                      key={provider.id}
                      type="button"
                      onClick={() => onBeginSso(provider)}
                      disabled={
                        busy ||
                        (provider.kind === 'ldap' && (!email.trim() || !password))
                      }
                      disabledReason={
                        provider.kind === 'ldap' && (!email.trim() || !password)
                          ? t('shell:signin.ldap_credentials_required')
                          : undefined
                      }
                      className="flex-1 justify-center"
                      aria-label={t('shell:signin.sso_button_aria', { name: provider.name })}
                    >
                      {provider.name} · {provider.kind.toUpperCase()}
                    </ChromeButton>
                  ))}
                </div>
              </>
            )}
          </div>
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
