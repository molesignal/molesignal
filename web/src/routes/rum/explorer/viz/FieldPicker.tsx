import { ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/shell/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

/** A control of the row under the query: as tall as the selects beside it, on the control surface. */
export const CHIP =
  'inline-flex h-8 min-w-0 max-w-full shrink-0 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs text-tx-0 hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none data-[state=open]:bg-bg-3';

/** The control that adds one more: no fill until the pointer is on it. */
export const GHOST =
  'inline-flex h-8 shrink-0 items-center gap-1 rounded-md px-2 font-sans text-xs text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none data-[state=open]:bg-bg-3';

const glyphOf = (field: RumField) => (field.dataType === 'bool' ? 'B' : field.dataType === 'int64' || field.dataType === 'float64' ? '#' : 'T');

/** The fields to choose from, searched by name, as the filter builder lists them. */
export function FieldList({
  fields,
  selected,
  onSelect,
}: {
  fields: readonly RumField[];
  selected: string | null;
  onSelect: (name: string) => void;
}) {
  const { t } = useTranslation('rum');

  return (
    <Command className="w-[300px]">
      <CommandInput autoFocus placeholder={t('explorer.viz.search_fields')} />
      <CommandList className="max-h-[280px]">
        <CommandEmpty>{t('explorer.viz.no_fields')}</CommandEmpty>
        <CommandGroup>
          {fields.map((field) => (
            <CommandItem key={field.name} value={field.name} onSelect={() => onSelect(field.name)} className="gap-2">
              <span aria-hidden="true" className="type-micro w-4 shrink-0 text-center font-mono font-semibold text-tx-3">
                {glyphOf(field)}
              </span>
              <span className={cn('truncate font-mono text-xs', field.name === selected ? 'text-indigo-soft' : 'text-tx-0')}>
                {field.name}
              </span>
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </Command>
  );
}

/**
 * A field as a control: what it is of (`Group by`), and the field. It opens the list of fields; a
 * choice closes it. With a `trigger` of its own it is the way to add a field instead.
 */
export function FieldPicker({
  fields,
  value,
  prefix,
  label,
  testId,
  trigger,
  onSelect,
}: {
  fields: readonly RumField[];
  value: string | null;
  prefix?: string;
  label: string;
  testId?: string;
  trigger?: React.ReactNode;
  onSelect: (name: string) => void;
}) {
  const [open, setOpen] = React.useState(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {trigger ?? (
          <button type="button" aria-label={label} data-rum-viz-control={testId} className={CHIP}>
            {prefix ? <span className="shrink-0 text-tx-3">{prefix}</span> : null}
            <span className="min-w-0 max-w-[260px] truncate font-mono">{value}</span>
            <ChevronDown aria-hidden="true" className="h-[14px] w-[14px] shrink-0 opacity-50" />
          </button>
        )}
      </PopoverTrigger>
      <PopoverContent align="start" className="w-auto p-0">
        <FieldList
          fields={fields}
          selected={value}
          onSelect={(name) => {
            onSelect(name);
            setOpen(false);
          }}
        />
      </PopoverContent>
    </Popover>
  );
}
