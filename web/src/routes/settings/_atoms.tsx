import * as React from 'react';

import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

/**
 * Section-body shell with a 24px gutter — every Settings sub-page
 * mounts its content inside this so spacing stays consistent.
 */
export function SectionBody({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return <div className={cn('p-4 lg:p-6', className)}>{children}</div>;
}

/**
 * Settings forms use one vertical group stack. Data tables and functional
 * filter grids stay outside this wrapper so their information density is
 * unaffected by the form layout contract.
 */
export function SettingsGroupStack({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-settings-layout="single-column"
      className={cn('flex w-full min-w-0 flex-col gap-4', className)}
    >
      {children}
    </div>
  );
}

/**
 * A Settings topic is one light card. Fields stay flat inside it so the page
 * gains hierarchy without turning every control into another nested panel.
 */
export function SettingsSection({
  title,
  description,
  children,
  tone = 'default',
  className,
  contentClassName,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  tone?: 'default' | 'danger';
  className?: string;
  contentClassName?: string;
}) {
  return (
    <section
      data-settings-section
      className={cn(
        'w-full min-w-0 rounded-lg border bg-bg-1 px-5 py-5 lg:px-6',
        tone === 'danger' ? 'border-red/30 bg-red-dim/30' : 'border-bd-0',
        className,
      )}
    >
      <header>
        <div className="type-section-title font-sans font-display-strong text-tx-0">
          {title}
        </div>
        {description && (
          <div className="mt-1 max-w-3xl font-sans text-base leading-relaxed text-tx-2 lg:text-sm">
            {description}
          </div>
        )}
      </header>
      <div className={cn('mt-5 flex min-w-0 flex-col gap-5', contentClassName)}>
        {children}
      </div>
    </section>
  );
}

/**
 * A named topic inside a Settings card. Adjacent topics use one deliberately
 * weak divider so related controls stay grouped without becoming nested cards.
 */
export function SettingsSubsection({
  title,
  description,
  children,
  className,
}: {
  title?: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-settings-subsection
      className={cn(
        'min-w-0 [&+&]:mt-4 [&+&]:border-t [&+&]:border-bd-0 [&+&]:pt-4',
        className,
      )}
    >
      {(title || description) && (
        <header>
          {title && (
            <div className="font-sans text-base font-strong text-tx-0 lg:text-sm">
              {title}
            </div>
          )}
          {description && (
            <div className="mt-1 max-w-3xl font-sans text-base leading-relaxed text-tx-2 lg:text-sm">
              {description}
            </div>
          )}
        </header>
      )}
      <div
        className={cn(
          'flex min-w-0 flex-col gap-5',
          (title || description) && 'mt-5',
        )}
      >
        {children}
      </div>
    </div>
  );
}

export function SettingsRow({
  label,
  description,
  children,
  controlClassName,
  className,
}: {
  label: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  controlClassName?: string;
  className?: string;
}) {
  return (
    <div
      data-settings-row
      className={cn(
        'grid w-full min-w-0 grid-cols-1 items-start gap-3 min-[1100px]:grid-cols-[260px_minmax(420px,1fr)] min-[1100px]:gap-8',
        className,
      )}
    >
      <div className="min-w-0">
        <div className="font-sans text-base font-strong text-tx-0 lg:text-sm">{label}</div>
        {description && (
          <div className="mt-1 max-w-[260px] font-sans text-base leading-relaxed text-tx-2 lg:text-sm">
            {description}
          </div>
        )}
      </div>
      <div
        className={cn(
          'flex min-h-11 w-full min-w-0 items-center lg:min-h-9',
          controlClassName,
        )}
      >
        {children}
      </div>
    </div>
  );
}

export function CopyableValue({
  value,
  copyLabel,
  copiedLabel,
}: {
  value: string;
  copyLabel: string;
  copiedLabel: string;
}) {
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | null>(null);

  React.useEffect(
    () => () => {
      if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    },
    [],
  );

  const copy = React.useCallback(async () => {
    if (!value || value === '—' || !navigator.clipboard?.writeText) return;
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      return;
    }
    setCopied(true);
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopied(false), 1600);
  }, [value]);

  return (
    <div className="flex min-h-11 w-full min-w-0 items-center rounded-md border border-bd-0 bg-bg-2 pl-3 lg:min-h-9">
      <code className="min-w-0 flex-1 truncate font-mono text-base text-tx-1 lg:text-xs">{value}</code>
      <CopyIconButton
        onClick={() => void copy()}
        disabled={!value || value === '—'}
        label={copyLabel}
        copied={copied}
        copiedLabel={copiedLabel}
        className="h-11 w-11 lg:h-8 lg:w-8"
      />
    </div>
  );
}

export function SettingsDraftStatus({
  dirty,
  error,
  modifiedLabel,
  undoLabel,
  errorLabel,
  retryLabel,
  onUndo,
  onRetry,
}: {
  dirty: boolean;
  error: boolean;
  modifiedLabel: string;
  undoLabel: string;
  errorLabel: string;
  retryLabel: string;
  onUndo: () => void;
  onRetry: () => void;
}) {
  if (!dirty && !error) return null;
  return (
    <div
      aria-live="polite"
      className={cn(
        'flex min-h-8 flex-wrap items-center justify-end gap-1 font-sans text-sm',
        error ? 'text-red-soft' : 'text-tx-3',
      )}
    >
      <span>{error ? errorLabel : modifiedLabel}</span>
      <span aria-hidden>·</span>
      {error && (
        <>
          <button
            type="button"
            onClick={onRetry}
            className="inline-flex min-h-11 items-center rounded px-1 font-strong text-red-soft hover:bg-red-dim lg:min-h-8"
          >
            {retryLabel}
          </button>
          <span aria-hidden>·</span>
        </>
      )}
      <button
        type="button"
        onClick={onUndo}
        className="inline-flex min-h-11 items-center rounded px-1 font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 lg:min-h-8"
      >
        {undoLabel}
      </button>
    </div>
  );
}

/** Read-only metadata follows the same responsive field grid as forms. */
export function KvRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div
      data-settings-row
      className="grid min-h-11 grid-cols-1 items-start gap-2 min-[1100px]:grid-cols-[260px_minmax(420px,1fr)] min-[1100px]:gap-8"
    >
      <div>
        <div className="font-sans text-base font-strong text-tx-1 lg:text-sm">{label}</div>
        {hint && <div className="mt-1 text-base text-tx-3 lg:text-sm">{hint}</div>}
      </div>
      <div className="font-sans text-base text-tx-0 lg:text-sm">{children}</div>
    </div>
  );
}
