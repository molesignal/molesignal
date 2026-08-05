import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { Switch } from '@/shell/ui/switch';

import { useDashboardText } from '../../i18n';

export function EditorSectionTitle({
  children,
}: {
  children: React.ReactNode;
}) {
  const tr = useDashboardText();
  return (
    <div className="mb-2 font-sans text-xs font-semibold text-tx-2">
      {typeof children === 'string' ? tr(children) : children}
    </div>
  );
}

export function EditorField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  const tr = useDashboardText();
  return (
    <label className="grid min-w-0 flex-1 gap-1 font-sans text-xs font-medium text-tx-3">
      {tr(label)}
      {children}
    </label>
  );
}

export function EditorInput({
  value,
  onChange,
  placeholder,
  mono = false,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  mono?: boolean;
}) {
  return (
    <input
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
      className={cn(
        'h-8 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 text-xs text-tx-1 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2',
        mono ? 'font-mono' : 'font-sans',
      )}
    />
  );
}

export function EditorTextarea({
  value,
  onChange,
  rows,
  placeholder,
  mono = false,
}: {
  value: string;
  onChange: (value: string) => void;
  rows: number;
  placeholder?: string;
  mono?: boolean;
}) {
  return (
    <textarea
      value={value}
      rows={rows}
      placeholder={placeholder}
      spellCheck={!mono}
      onChange={(event) => onChange(event.target.value)}
      className={cn(
        'min-w-0 resize-y rounded-md border border-bd-1 bg-bg-1 px-2 py-2 text-xs leading-5 text-tx-1 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2',
        mono ? 'font-mono' : 'font-sans',
      )}
    />
  );
}

export function EditorSelect({
  value,
  options,
  onChange,
}: {
  value: string;
  options: ReadonlyArray<readonly [string, string]>;
  onChange: (value: string) => void;
}) {
  const tr = useDashboardText();
  return (
    <select
      value={value}
      onChange={(event) => onChange(event.target.value)}
      className="h-8 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 font-sans text-xs text-tx-1 outline-none focus-visible:bg-bg-2"
    >
      {options.map(([optionValue, label]) => (
        <option key={optionValue} value={optionValue}>
          {tr(label)}
        </option>
      ))}
    </select>
  );
}

export function EditorNumber({
  value,
  onChange,
  min,
  max,
}: {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
}) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      onChange={(event) => {
        const parsed = Number(event.target.value);
        if (Number.isFinite(parsed)) onChange(parsed);
      }}
      className="h-8 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 font-mono text-xs text-tx-1 outline-none focus-visible:bg-bg-2"
    />
  );
}

export function OptionalNumberInput({
  value,
  onChange,
  placeholder,
}: {
  value: number | undefined;
  onChange: (value: number | undefined) => void;
  placeholder?: string | undefined;
}) {
  return (
    <input
      type="number"
      value={value ?? ''}
      placeholder={placeholder}
      onChange={(event) => {
        if (!event.target.value.trim()) {
          onChange(undefined);
          return;
        }
        const parsed = Number(event.target.value);
        if (Number.isFinite(parsed)) onChange(parsed);
      }}
      className="h-8 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 font-mono text-xs text-tx-1 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2"
    />
  );
}

export function ToggleField({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const tr = useDashboardText();
  return (
    <label className="flex items-center justify-between gap-3 rounded-md border border-bd-0 px-2 py-1.5 font-sans text-xs text-tx-2">
      {tr(label)}
      <Switch checked={checked} onCheckedChange={onChange} />
    </label>
  );
}
