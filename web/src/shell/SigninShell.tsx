import { ArrowLeft, MailCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PublicSsoProvider } from '@/api/sso';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { LogoMark } from '@/shell/LogoMark';
import { Checkbox } from '@/shell/ui/checkbox';

export type SigninView = 'signin' | 'forgot-password' | 'forgot-password-sent' | 'reset-password';

/**
 * SigninShell — the public-facing sign-in face (brief Component Inventory:
 * a standalone 880px card with a 380px left brand panel and a right auth
 * `shadow-login`, no Topbar / Sidebar). Purely
 * presentational: all auth state and side effects live in the `/signin`
 * route, which passes them in as props.
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
  buildVersion?: string | undefined;
  releaseChannel?: string | undefined;
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
  releaseChannel,
  signupEnabled,
  onSignup,
}: SigninShellProps) {
  const { t } = useTranslation(['shell', 'common']);
  const passwordMismatch = Boolean(confirmPassword) && password !== confirmPassword;
  const shortPassword = Boolean(password) && [...password].length < 8;
  const hasLdapProvider = ssoProviders.some((provider) => provider.kind === 'ldap');
  const normalizedBuildVersion = buildVersion?.trim();
  const normalizedReleaseChannel = releaseChannel?.trim();
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
    <div className="grid h-[100dvh] w-full items-start justify-items-center overflow-y-auto bg-bg-0 p-[16px] sm:p-[24px] lg:place-items-center">
      <div
        className="grid w-full max-w-[880px] animate-fade-in grid-cols-1 overflow-hidden rounded-xl border border-bd-0 bg-bg-1 shadow-login lg:min-h-[520px] lg:grid-cols-[380px_minmax(0,1fr)]"
        data-testid="signin-card"
      >
        {/* left brand panel */}
        <div
          className="relative flex flex-col border-b border-bd-0 p-[24px] sm:p-8 lg:border-b-0 lg:border-r lg:pb-12"
          data-testid="signin-brand"
          style={{
            background:
              'radial-gradient(ellipse at 20% 0%, rgba(79,96,224,0.08), transparent 60%), radial-gradient(ellipse at 100% 100%, rgba(61,194,110,0.06), transparent 50%), var(--bg-1)',
          }}
        >
          <div
            className="flex translate-y-[2px] items-center gap-3"
            data-testid="signin-logo"
          >
            <LogoMark size={40} />
            <span className="font-sans text-[22px] font-strong tracking-tight text-tx-0">
              {t('common:app_name')}
            </span>
          </div>

          <div
            className="mt-9 font-sans lg:mt-[88px]"
            data-testid="signin-brand-content"
          >
            <div className="text-[19px] font-strong leading-[1.35] tracking-tight text-tx-0 sm:text-[20px]">
              <span className="whitespace-nowrap">
                {t('shell:signin.tagline_a')}{' '}
                <span className="text-indigo-soft">
                  {t('shell:signin.tagline_b')}
                </span>
              </span>
              <br />
              <span className="text-green-soft">
                {t('shell:signin.tagline_c')}
              </span>
            </div>
            <div className="mt-4 text-[13px] leading-relaxed text-tx-1">
              {t('shell:signin.promise')}
            </div>
            <div className="mt-6 text-xs leading-relaxed text-tx-2">
              <span className="block">{t('shell:signin.blurb_a')}</span>
              <span className="block">{t('shell:signin.blurb_b')}</span>
            </div>

            {/* Mini terminal stays English as literal CLI output. */}
            <div
              className="mt-9 rounded-md border border-bd-0 bg-bg-0 p-3 text-xs leading-[1.7] text-tx-1"
              data-testid="signin-status"
            >
              <div>
                <span className="text-tx-3">$</span> molesignal status
              </div>
              <div className="text-yellow-soft">
                18 services · 2 warnings
              </div>
              <div className="flex items-center gap-1.5 text-green-soft">
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full bg-green"
                  aria-hidden
                />
                All telemetry pipelines ready
              </div>
            </div>
          </div>
          {normalizedBuildVersion && normalizedReleaseChannel ? (
            <div
              className="mt-6 text-left font-sans text-xs text-tx-2 lg:mt-auto"
              data-testid="signin-build-info"
            >
              {t('shell:signin.version_line', {
                version: normalizedBuildVersion,
                channel: normalizedReleaseChannel,
              })}
            </div>
          ) : null}
        </div>

        {/* right form */}
        <form
          onSubmit={onSubmit}
          className="flex min-w-0 flex-col p-[24px] sm:p-12"
          data-testid="signin-form"
        >
          <div className="flex flex-1 flex-col justify-center">
            <h1 className="m-0 font-sans text-[22px] tracking-tight text-tx-0">
              {heading}
            </h1>
            <div className="mt-1.5 font-sans text-xs leading-relaxed text-tx-2">
              {subtitle}
            </div>

            {view === 'signin' && (
              <div className="mt-7 flex flex-col gap-3.5">
                <Field
                  label={
                    hasLdapProvider
                      ? t('shell:signin.email_or_username')
                      : t('common:labels.email')
                  }
                  value={email}
                  onChange={onEmailChange}
                  placeholder="admin@example.com"
                  type={hasLdapProvider ? 'text' : 'email'}
                  autoComplete="username"
                  autoFocus
                />
                <Field
                  label={t('common:labels.password')}
                  value={password}
                  onChange={onPasswordChange}
                  placeholder="••••••••••••"
                  type="password"
                  autoComplete="current-password"
                />

                <div className="mt-1 flex items-center justify-between font-sans text-xs">
                  <label className="flex items-center gap-1.5 text-tx-1">
                    <Checkbox
                      checked={remember}
                      onCheckedChange={(checked) => onRememberChange(checked === true)}
                    />
                    {t('common:labels.remember_me')}
                  </label>
                  <button
                    type="button"
                    className="cursor-pointer text-blue-soft hover:underline"
                    onClick={onOpenForgotPassword}
                  >
                    {t('shell:signin.forgot_password')}
                  </button>
                </div>

                <PrimaryButton disabled={submitDisabled}>
                  {busy ? t('shell:signin.signing_in') : `${t('common:actions.sign_in')} →`}
                </PrimaryButton>

                {ssoProviders.length > 0 && (
                  <>
                    <div className="my-2 flex items-center gap-2.5 font-sans text-xs text-tx-2">
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
                            (provider.kind === 'ldap' &&
                              (!email.trim() || !password))
                          }
                          disabledReason={
                            provider.kind === 'ldap' && (!email.trim() || !password)
                              ? t('shell:signin.ldap_credentials_required')
                              : undefined
                          }
                          className="h-8 flex-1 justify-center"
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
              <div className="mt-7 flex flex-col gap-3.5">
                <Field
                  label={t('common:labels.email')}
                  value={email}
                  onChange={onEmailChange}
                  placeholder="you@company.com"
                  type="email"
                  autoComplete="email"
                  autoFocus
                />
                <div className="font-sans text-xs leading-relaxed text-tx-3">
                  {t('shell:signin.forgot_privacy_hint')}
                </div>
                <PrimaryButton disabled={submitDisabled}>
                  {busy
                    ? t('shell:signin.sending_reset')
                    : `${t('shell:signin.send_reset')} →`}
                </PrimaryButton>
                <BackButton onClick={onBackToSignin}>
                  {t('shell:signin.back_to_signin')}
                </BackButton>
              </div>
            )}

            {view === 'forgot-password-sent' && (
              <div className="mt-7 flex flex-col">
                <div className="flex h-11 w-11 items-center justify-center rounded-lg border border-indigo/20 bg-indigo/10 text-indigo-soft">
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
              <div className="mt-7 flex flex-col gap-3.5">
                <Field
                  label={t('shell:signin.new_password')}
                  value={password}
                  onChange={onPasswordChange}
                  placeholder="••••••••••••"
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
                  placeholder="••••••••••••"
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
                <PrimaryButton disabled={submitDisabled}>
                  {busy
                    ? t('shell:signin.resetting_password')
                    : `${t('shell:signin.reset_password')} →`}
                </PrimaryButton>
                <BackButton onClick={onBackToSignin}>
                  {t('shell:signin.back_to_signin')}
                </BackButton>
              </div>
            )}
          </div>

          {view === 'signin' && signupEnabled && (
            <div className="pt-8 font-sans text-xs text-tx-2">
              {t('shell:signin.no_account')}{' '}
              <button type="button" className="cursor-pointer text-blue-soft" onClick={onSignup}>
                {t('shell:signin.signup_link')}
              </button>
            </div>
          )}
        </form>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
  autoFocus,
  autoComplete,
  minLength,
  maxLength,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  autoFocus?: boolean;
  autoComplete?: string;
  minLength?: number;
  maxLength?: number;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className={uiLabelClass}>{label}</span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        autoComplete={autoComplete}
        minLength={minLength}
        maxLength={maxLength}
        aria-label={label}
        className="h-11 min-w-0 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-[16px] text-tx-0 outline-none placeholder:text-tx-3 sm:h-9 sm:text-xs"
      />
    </label>
  );
}

function PrimaryButton({
  disabled,
  children,
}: {
  disabled: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="submit"
      disabled={disabled}
      className="auth-primary-button mt-2 flex h-11 items-center justify-center rounded-md font-sans text-xs font-bold tracking-wide text-white focus-visible:outline-none sm:h-9 disabled:cursor-not-allowed disabled:opacity-50"
    >
      {children}
    </button>
  );
}

function BackButton({
  onClick,
  children,
  className = '',
}: {
  onClick: () => void;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex h-11 items-center justify-center gap-1.5 font-sans text-xs text-tx-2 hover:text-tx-0 sm:h-8 ${className}`}
    >
      <ArrowLeft size={13} aria-hidden />
      {children}
    </button>
  );
}
