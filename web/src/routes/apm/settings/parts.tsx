import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';

/** A field of the settings: filled, never outlined. */
export const FIELD =
  'h-8 min-w-0 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-60 aria-invalid:text-[var(--red-ink)]';

/** What a card edits, the saved value until it is changed. */
export function useDraft<T>(saved: T) {
  const [draft, setDraft] = React.useState<T | null>(null);
  const value = draft ?? saved;

  return {
    value,
    dirty: draft !== null && JSON.stringify(draft) !== JSON.stringify(saved),
    set: (next: T) => setDraft(next),
    reset: () => setDraft(null),
  };
}

/** Save and undo in a card's head, while the card has changes. */
export function CardSave({
  dirty,
  invalid,
  pending,
  access,
  onSave,
  onReset,
  name,
}: {
  dirty: boolean;
  invalid: boolean;
  pending: boolean;
  access: ActionAccess;
  onSave: () => void;
  onReset: () => void;
  /** Which card, for tests. */
  name: string;
}) {
  const { t } = useTranslation('apm');

  if (!dirty) return null;

  return (
    <>
      <Button type="button" variant="outline" size="sm" onClick={onReset} disabled={pending}>
        {t('org_settings.reset')}
      </Button>
      <Button type="button" size="sm" onClick={onSave} disabled={invalid || pending || access.disabled} title={access.reason} data-apm-settings-save={name}>
        {t('org_settings.save')}
      </Button>
    </>
  );
}

/** A number in a field, its unit after it. */
export function NumberField({
  label,
  value,
  unit,
  onChange,
  invalid = false,
  disabled = false,
  width = 'w-20',
  className,
}: {
  label: string;
  value: string;
  unit?: string;
  onChange: (value: string) => void;
  invalid?: boolean;
  disabled?: boolean;
  width?: string;
  className?: string;
}) {
  return (
    <label className={cn('inline-flex items-center gap-1.5 font-sans text-xs text-tx-2', className)}>
      {/* Spread over a row, the label takes the room and the fields line up at its end. */}
      <span className={cn('text-tx-2', className && 'min-w-0 flex-1 truncate')}>{label}</span>
      <input
        inputMode="decimal"
        aria-label={label}
        aria-invalid={invalid || undefined}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        className={cn(FIELD, width, 'text-right tabular-nums')}
      />
      {unit && <span className="text-tx-3">{unit}</span>}
    </label>
  );
}

/** Rows of a list, each removable, and a control to add one under them. */
export function ListRows<T>({
  rows,
  onChange,
  blank,
  addLabel,
  disabled,
  empty,
  render,
  head,
  name,
}: {
  rows: readonly T[];
  onChange: (rows: T[]) => void;
  blank: () => T;
  addLabel: string;
  disabled: boolean;
  empty: string;
  render: (row: T, change: (row: T) => void, index: number) => React.ReactNode;
  /** What each field of a row is, over the rows, laid out as a row's fields are. */
  head?: React.ReactNode;
  /** Which list, for tests. */
  name: string;
}) {
  const { t } = useTranslation('apm');

  return (
    <div data-apm-settings-list={name} className="space-y-1.5 p-3">
      {rows.length === 0 && <p className="py-1 font-sans text-xs text-tx-3">{empty}</p>}
      {rows.length > 0 && head && (
        // The remove control's room kept at the end, so each title stands over its field.
        <div aria-hidden className="flex items-center gap-2 pr-10 font-sans text-xs font-strong text-tx-2">
          {head}
        </div>
      )}
      {rows.map((row, index) => (
        <div key={index} data-apm-settings-row={index} className="flex items-center gap-2">
          <div className="flex min-w-0 flex-1 items-center gap-2">{render(row, (next) => onChange(rows.map((item, at) => (at === index ? next : item))), index)}</div>
          <button
            type="button"
            aria-label={t('org_settings.remove')}
            disabled={disabled}
            onClick={() => onChange(rows.filter((_, at) => at !== index))}
            className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 disabled:opacity-40"
          >
            <Trash2 aria-hidden className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}
      <button
        type="button"
        disabled={disabled}
        onClick={() => onChange([...rows, blank()])}
        className="inline-flex h-8 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3 disabled:opacity-40"
      >
        <Plus aria-hidden className="h-3.5 w-3.5" />
        {addLabel}
      </button>
    </div>
  );
}

/** Why a row cannot be saved, in red under the list. */
export function Issues({ issues }: { issues: readonly string[] }) {
  if (issues.length === 0) return null;

  return (
    <div className="space-y-0.5 border-t border-bd-0 px-3 py-2 font-sans text-xs text-[var(--red-ink)]">
      {issues.map((issue) => (
        <p key={issue}>{issue}</p>
      ))}
    </div>
  );
}
