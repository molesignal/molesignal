import { ChevronRight, KeyRound, Mail } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Pill } from '@/shell/chrome';
import { PulseLine } from '@/shell/PulseLine';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';
import { Checkbox } from '@/shell/ui/checkbox';

import { Field, PrimaryButton, StepHeader } from './AuthControls';
import type { ChannelRow, VisibleStep } from './useSigninSteps';

export interface SigninStepsProps {
  step: VisibleStep;
  rows: ChannelRow[];
  canGoBack: boolean;
  onBack: () => void;
  onChoose: (row: ChannelRow) => void;
  /** The browser is being asked for a passkey. */
  passkeyBusy: boolean;
  /** The email field also offers the browser's passkeys, and a request waits behind it. */
  passkeyAutofill: boolean;
  email: string;
  onEmailChange: (value: string) => void;
  password: string;
  onPasswordChange: (value: string) => void;
  remember: boolean;
  onRememberChange: (value: boolean) => void;
  busy: boolean;
  onOpenForgotPassword: () => void;
}

/** The heading and the sentence under it, for the step on screen. */
export function useStepHeading(step: VisibleStep): { title: string; subtitle: string } {
  const { t } = useTranslation(['shell', 'common']);
  const title = t('common:actions.sign_in');

  if (step.name === 'identifier') {
    return step.directory
      ? {
          title: t('shell:signin.directory_title', { name: step.directory.name }),
          subtitle: t('shell:signin.directory_subtitle', { name: step.directory.name }),
        }
      : { title, subtitle: t('shell:signin.email_subtitle') };
  }

  if (step.name === 'password') {
    return step.directory
      ? {
          title: t('shell:signin.directory_title', { name: step.directory.name }),
          subtitle: t('shell:signin.directory_password_subtitle', { name: step.directory.name }),
        }
      : { title, subtitle: t('shell:signin.password_subtitle') };
  }

  return { title, subtitle: t('shell:signin.subtitle') };
}

function RowMark({ row }: { row: ChannelRow }) {
  return (
    <span className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-bg-2 text-tx-2">
      {row.kind === 'account' && <Mail size={14} aria-hidden />}
      {row.kind === 'passkey' && <KeyRound size={14} aria-hidden />}
      {row.kind === 'provider' && (
        <SsoPlatformMark platform={row.provider.platform} kind={row.provider.kind} />
      )}
    </span>
  );
}

function ChannelRowButton({
  row,
  disabled,
  passkeyBusy,
  onChoose,
}: {
  row: ChannelRow;
  disabled: boolean;
  passkeyBusy: boolean;
  onChoose: (row: ChannelRow) => void;
}) {
  const { t } = useTranslation('shell');

  const label =
    row.kind === 'account'
      ? t('signin.continue_with_email')
      : row.kind === 'provider'
        ? t('signin.continue_with', { name: row.provider.name })
        : passkeyBusy
          ? t('signin.passkey_waiting')
          : t('signin.passkey');

  return (
    <button
      type="button"
      onClick={() => onChoose(row)}
      disabled={disabled}
      data-testid={row.kind === 'passkey' ? 'signin-passkey' : undefined}
      className="ms-press flex min-h-12 w-full min-w-0 cursor-pointer items-center gap-3 px-3 py-2 text-left font-sans text-sm text-tx-0 transition-colors duration-fast enabled:hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none disabled:cursor-not-allowed disabled:text-tx-3"
    >
      <RowMark row={row} />
      <span className="min-w-0 flex-1">
        <span className="block truncate">{label}</span>
        {row.account && (
          <span className="mt-0.5 block truncate text-xs text-tx-2">{row.account}</span>
        )}
      </span>
      {row.lastUsed && <Pill tone="blue">{t('signin.last_used')}</Pill>}
      <ChevronRight size={14} aria-hidden className="shrink-0 text-tx-3" />
    </button>
  );
}

function RememberMe({
  remember,
  onChange,
}: {
  remember: boolean;
  onChange: (value: boolean) => void;
}) {
  const { t } = useTranslation('common');

  return (
    <label className="flex items-center gap-1.5 font-sans text-xs text-tx-1">
      <Checkbox checked={remember} onCheckedChange={(checked) => onChange(checked === true)} />
      {t('labels.remember_me')}
    </label>
  );
}

/**
 * The sign-in view, one question at a time: the ways in (the one used last time marked), then the
 * email, then the password. The email field is what password managers and the browser's passkey
 * suggestions fill; the password step repeats the account in a field nobody sees, so a password
 * manager saves the two together.
 */
export function SigninSteps({
  step,
  rows,
  canGoBack,
  onBack,
  onChoose,
  passkeyBusy,
  passkeyAutofill,
  email,
  onEmailChange,
  password,
  onPasswordChange,
  remember,
  onRememberChange,
  busy,
  onOpenForgotPassword,
}: SigninStepsProps) {
  const { t } = useTranslation(['shell', 'common']);
  const back = canGoBack ? onBack : undefined;

  if (step.name === 'waiting') {
    return (
      <div className="mt-6 flex h-24 items-center justify-center" data-testid="signin-waiting">
        <PulseLine variant="travel" className="h-6 w-20 text-tx-3" />
      </div>
    );
  }

  if (step.name === 'channels') {
    return (
      <div className="mt-6 flex flex-col gap-4">
        <ul
          className="m-0 flex list-none flex-col divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0 p-0"
          data-testid="signin-channels"
        >
          {rows.map((row) => (
            <li
              key={
                row.kind === 'provider' ? row.provider.id : row.kind
              }
            >
              <ChannelRowButton
                row={row}
                disabled={busy || passkeyBusy}
                passkeyBusy={passkeyBusy}
                onChoose={onChoose}
              />
            </li>
          ))}
        </ul>

        <div className="flex items-center gap-2.5 font-sans text-xs text-tx-2">
          <div className="h-px flex-1 bg-bd-0" />
          {t('shell:signin.or')}
          <div className="h-px flex-1 bg-bd-0" />
        </div>

        <PrimaryButton disabled={busy || passkeyBusy} busy={busy}>
          {busy ? t('shell:signin.signing_in') : `${t('shell:signin.email_button')} →`}
        </PrimaryButton>

        <RememberMe remember={remember} onChange={onRememberChange} />
      </div>
    );
  }

  const directory = step.directory;

  if (step.name === 'identifier') {
    return (
      <div className="mt-6 flex flex-col gap-4">
        <StepHeader onBack={back} backLabel={t('shell:signin.back')} />
        <Field
          key={directory?.id ?? 'email'}
          label={directory ? t('shell:signin.directory_username') : t('common:labels.email')}
          value={email}
          onChange={onEmailChange}
          placeholder={directory ? undefined : t('shell:signin.email_placeholder')}
          type={directory ? 'text' : 'email'}
          name="username"
          autoComplete={passkeyAutofill ? 'username webauthn' : 'username'}
          selectOnFocus
          autoFocus
        />
        <PrimaryButton disabled={busy || !email.trim()}>
          {`${t('shell:signin.continue')} →`}
        </PrimaryButton>
      </div>
    );
  }

  return (
    <div className="mt-6 flex flex-col gap-4">
      <StepHeader
        onBack={back}
        backLabel={t('shell:signin.back')}
        account={email}
        accountIsEmail={!directory}
      />
      <input
        type={directory ? 'text' : 'email'}
        name="username"
        autoComplete="username"
        value={email}
        readOnly
        tabIndex={-1}
        aria-hidden="true"
        className="sr-only"
      />
      <Field
        label={t('common:labels.password')}
        value={password}
        onChange={onPasswordChange}
        placeholder={t('shell:signin.password_placeholder')}
        type="password"
        name="password"
        autoComplete="current-password"
        revealable
        autoFocus
      />
      <div className="flex items-center justify-between font-sans text-xs">
        <RememberMe remember={remember} onChange={onRememberChange} />
        {!directory && (
          <button
            type="button"
            className="cursor-pointer text-[var(--blue-ink)] hover:underline"
            onClick={onOpenForgotPassword}
          >
            {t('shell:signin.forgot_password')}
          </button>
        )}
      </div>
      <PrimaryButton disabled={busy || !password} busy={busy}>
        {busy ? t('shell:signin.signing_in') : `${t('common:actions.sign_in')} →`}
      </PrimaryButton>
    </div>
  );
}
