import { ArrowLeft } from 'lucide-react';
import type * as React from 'react';

import { uiLabelClass } from '@/shell/chrome';
import { PulseLine } from '@/shell/PulseLine';

/**
 * A labelled text field. Its fill is its only edge (the same control surface
 * every field in the product uses); the label warms up while the field is being
 * typed in, so the eye keeps its place in a two-field form.
 */
export function Field({
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
    <label className="ms-auth-field flex flex-col gap-1">
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
        className="h-11 min-w-0 rounded-lg bg-[var(--control-surface)] px-3.5 font-sans text-[16px] text-tx-0 outline-none transition-colors duration-fast placeholder:text-tx-3 hover:bg-bg-3 focus:bg-bg-3 sm:h-10 sm:text-sm"
      />
    </label>
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
