import * as React from 'react';

import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

/**
 * Flat page body used below the local settings header. The management page
 * already owns the outer gutter, so section content should align directly
 * with its title instead of creating another inset panel.
 */
export function SectionBody({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn('px-0 pb-8 pt-5 lg:px-0 lg:pb-10 lg:pt-6', className)}
    >
      {children}
    </div>
  );
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
      className={cn('flex w-full min-w-0 flex-col gap-0', className)}
    >
      {children}
    </div>
  );
}

/**
 * A Settings topic is a flat region on one continuous management canvas.
 * Adjacent topics use a single low-contrast divider; controls keep their own
 * functional boundaries, while the section itself never becomes a card.
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
        'w-full min-w-0 py-1 [&+&]:mt-7 [&+&]:border-t [&+&]:border-bd-0 [&+&]:pt-8',
        className,
      )}
    >
      <header>
        <div
          data-settings-section-title
          className={cn(
            'type-section-title font-sans font-display-strong',
            tone === 'danger' ? 'text-red-soft' : 'text-tx-0',
          )}
        >
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
 * A named topic inside a Settings region. Whitespace, rather than another
 * border, separates related topics and keeps long forms calm.
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
        'min-w-0 [&+&]:mt-8 [&+&]:pt-1',
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
        'grid w-full min-w-0 grid-cols-1 items-start gap-3 min-[1100px]:grid-cols-[minmax(220px,280px)_minmax(420px,1fr)] min-[1100px]:gap-8',
        className,
      )}
    >
      <div className="min-w-0">
        <div className="font-sans text-base font-strong text-tx-0 lg:text-sm">{label}</div>
        {description && (
          <div className="mt-1 max-w-[280px] font-sans text-base leading-relaxed text-tx-2 lg:text-sm">
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
    if (!value || value === '—') return;
    try {
      await writeClipboardText(value);
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
      className="grid min-h-11 grid-cols-1 items-start gap-2 min-[1100px]:grid-cols-[minmax(220px,280px)_minmax(420px,1fr)] min-[1100px]:gap-8"
    >
      <div>
        <div className="font-sans text-base font-strong text-tx-1 lg:text-sm">{label}</div>
        {hint && <div className="mt-1 text-base text-tx-3 lg:text-sm">{hint}</div>}
      </div>
      <div className="font-sans text-base text-tx-0 lg:text-sm">{children}</div>
    </div>
  );
}
