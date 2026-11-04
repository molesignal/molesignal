import { ArrowLeft, Eye, EyeOff, Mail, UserRound } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { IconSwap } from '@/shell/polish/IconSwap';
import { PulseLine } from '@/shell/PulseLine';

/**
 * A labelled text field. Its fill is its only edge (the same control surface
 * every field in the product uses); the label warms up while the field is being
 * typed in, so the eye keeps its place in a two-field form. A password can be
 * shown (`revealable`); a value filled in for the reader is replaced by typing
 * over it (`selectOnFocus`).
 */
export function Field({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
  name,
  autoFocus,
  autoComplete,
  minLength,
  maxLength,
  inputMode,
  hideLabel = false,
  revealable = false,
  selectOnFocus = false,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string | undefined;
  type?: string;
  name?: string;
  autoFocus?: boolean;
  autoComplete?: string;
  minLength?: number;
  maxLength?: number;
  inputMode?: React.HTMLAttributes<HTMLInputElement>['inputMode'];
  /** Only for a field something else on the card already names. */
  hideLabel?: boolean;
  revealable?: boolean;
  selectOnFocus?: boolean;
}) {
  const { t } = useTranslation('shell');
  const [revealed, setRevealed] = React.useState(false);

  const input = (
    <input
      type={revealable && revealed ? 'text' : type}
      name={name}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onFocus={selectOnFocus ? (e) => e.currentTarget.select() : undefined}
      placeholder={placeholder}
      autoFocus={autoFocus}
      autoComplete={autoComplete}
      minLength={minLength}
      maxLength={maxLength}
      inputMode={inputMode}
      aria-label={label}
      className={cn(
        'h-11 min-w-0 rounded-lg bg-[var(--control-surface)] px-3.5 font-sans text-[16px] text-tx-0 outline-none transition-colors duration-fast placeholder:text-tx-3 hover:bg-bg-3 focus:bg-bg-3 sm:h-10 sm:text-sm',
        revealable && 'pr-11',
      )}
    />
  );

  return (
    <label className="ms-auth-field flex flex-col gap-1">
      <span className={hideLabel ? 'sr-only' : uiLabelClass}>{label}</span>
      {revealable ? (
        <div className="relative flex min-w-0 flex-col">
          {input}
          <button
            type="button"
            onClick={() => setRevealed((shown) => !shown)}
            aria-label={revealed ? t('signin.hide_password') : t('signin.show_password')}
            aria-pressed={revealed}
            className="absolute right-1 top-1/2 grid h-9 w-9 -translate-y-1/2 cursor-pointer place-items-center rounded-md text-tx-2 transition-colors duration-fast hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none sm:h-8 sm:w-8"
          >
            <IconSwap
              active={revealed}
              inactiveIcon={<Eye size={14} aria-hidden />}
              activeIcon={<EyeOff size={14} aria-hidden />}
            />
          </button>
        </div>
      ) : (
        input
      )}
    </label>
  );
}

/**
 * The head of a step: the way back, and the account the step is for, so nobody has to remember
 * what they typed one screen ago. A long address is cut, not wrapped.
 */
export function StepHeader({
  onBack,
  backLabel,
  account,
  accountIsEmail = true,
}: {
  /** Leave it out where there is nothing to go back to. */
  onBack?: (() => void) | undefined;
  backLabel: string;
  account?: string;
  accountIsEmail?: boolean;
}) {
  if (!onBack && !account) return null;

  const AccountIcon = accountIsEmail ? Mail : UserRound;

  return (
    <div className="flex min-w-0 items-center gap-2" data-testid="signin-step-header">
      {onBack && (
        <BackButton onClick={onBack} className="-ml-1 shrink-0 px-1">
          {backLabel}
        </BackButton>
      )}
      {account && (
        <span
          className="inline-flex h-7 min-w-0 items-center gap-1.5 rounded-full bg-bg-2 px-2.5 font-sans text-xs text-tx-1"
          data-testid="signin-account"
        >
          <AccountIcon size={12} aria-hidden className="shrink-0 text-tx-2" />
          <span className="truncate">{account}</span>
        </span>
      )}
    </div>
  );
}

/**
 * The submit action. While a request is in flight the mark's pulse runs where
 * the label's arrow would be, instead of a spinner.
 */
export function PrimaryButton({
  disabled,
  busy = false,
  children,
}: {
  disabled: boolean;
  busy?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="submit"
      disabled={disabled}
      className="auth-primary-button ms-press mt-2 flex h-11 items-center justify-center gap-2 rounded-lg font-sans text-sm font-bold tracking-wide text-white focus-visible:outline-none sm:h-10 disabled:cursor-not-allowed"
    >
      {busy && <PulseLine variant="travel" className="h-3.5 w-8" />}
      {children}
    </button>
  );
}

export function BackButton({
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
      className={`ms-press flex h-11 items-center justify-center gap-1.5 font-sans text-xs text-tx-2 hover:text-tx-0 sm:h-9 ${className}`}
    >
      <ArrowLeft size={13} aria-hidden />
      {children}
    </button>
  );
}
