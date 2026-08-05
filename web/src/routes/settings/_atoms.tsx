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
  return <div className={cn('p-6', className)}>{children}</div>;
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
      className={cn('flex w-full min-w-0 flex-col gap-8', className)}
    >
      {children}
    </div>
  );
}

/**
 * A Settings topic uses rules and spacing instead of a surrounding card.
 * This keeps dense admin forms readable without stacking borders.
 */
export function SettingsSection({
  title,
  description,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      data-settings-section
      className={cn('w-full min-w-0 border-t border-bd-1', className)}
    >
      <header className="border-b border-bd-0 py-4">
        <div className="type-section-title font-sans font-display-strong text-tx-0">
          {title}
        </div>
        {description && (
          <div className="mt-1 max-w-3xl font-sans text-xs leading-relaxed text-tx-2">
            {description}
          </div>
        )}
      </header>
      <div>{children}</div>
    </section>
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
        'flex min-h-20 w-full min-w-0 flex-col items-stretch gap-3 border-b border-bd-0 py-4 last:border-b-0',
        className,
      )}
    >
      <div className="min-w-0">
        <div className="font-sans text-sm font-strong text-tx-0">{label}</div>
        {description && (
          <div className="mt-1 max-w-2xl font-sans text-xs leading-relaxed text-tx-2">
            {description}
          </div>
        )}
      </div>
      <div
        className={cn(
          'flex w-full min-w-0 max-w-2xl items-center',
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
    <div className="flex min-h-9 w-full min-w-0 items-center rounded-md border border-bd-0 bg-bg-1 pl-3">
      <code className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1">{value}</code>
      <CopyIconButton
        onClick={() => void copy()}
        disabled={!value || value === '—'}
        label={copyLabel}
        copied={copied}
        copiedLabel={copiedLabel}
      />
    </div>
  );
}

/** Read-only metadata follows the same single-column rhythm as form rows. */
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
      className="flex min-h-16 flex-col gap-2 border-b border-bd-0 py-3 last:border-b-0"
    >
      <div>
        <div className="font-sans text-xs font-strong text-tx-1">{label}</div>
        {hint && <div className="mt-1 text-xs text-tx-3">{hint}</div>}
      </div>
      <div className="font-sans text-xs text-tx-0">{children}</div>
    </div>
  );
}
