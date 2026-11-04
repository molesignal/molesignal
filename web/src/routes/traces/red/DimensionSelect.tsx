import { Check, ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Command, CommandGroup, CommandInput, CommandItem, CommandList } from '@/shell/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import type { TraceFieldDef } from '../fieldQueryModel';
import { RED_DIMENSION_ORDER } from './model';

/**
 * The field the requests are broken down by, found by typing among the
 * stream's fields: the ones a drill goes down first, then the rest by name.
 */
export function DimensionSelect({
  value,
  fields,
  onChange,
}: {
  value: string | null;
  /** The fields that can group requests, in the order they are offered. */
  fields: readonly TraceFieldDef[];
  onChange: (field: string) => void;
}) {
  const { t } = useTranslation('traces');
  const [open, setOpen] = React.useState(false);
  const [search, setSearch] = React.useState('');
  const typed = search.trim().toLowerCase();
  const matches = typed ? fields.filter((field) => field.name.toLowerCase().includes(typed)) : fields;
  const common = matches.filter((field) => RED_DIMENSION_ORDER.includes(field.name));
  const others = matches.filter((field) => !RED_DIMENSION_ORDER.includes(field.name));
  const choose = (field: string) => {
    onChange(field);
    setOpen(false);
    setSearch('');
  };
  const item = (field: TraceFieldDef) => (
    <CommandItem key={field.name} value={field.name} onSelect={() => choose(field.name)}>
      <Check
        aria-hidden
        className={cn('h-3.5 w-3.5 shrink-0 text-indigo', field.name === value ? 'opacity-100' : 'opacity-0')}
      />
      <span className="truncate">{field.name}</span>
    </CommandItem>
  );

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setSearch('');
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-expanded={open}
          aria-label={t('explore.red.breakdown.dimension')}
          data-red-dimension
          className="inline-flex h-7 min-w-0 max-w-[260px] items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-0 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3"
        >
          <span className="min-w-0 truncate">{value ?? t('explore.red.breakdown.no_dimension')}</span>
          <ChevronDown aria-hidden className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[280px] max-w-[calc(100vw-32px)] p-0">
        <Command shouldFilter={false}>
          <CommandInput
            value={search}
            onValueChange={setSearch}
            placeholder={t('explore.red.breakdown.search')}
            className="text-xs"
          />
          <CommandList className="max-h-[300px]">
            {matches.length === 0 ? (
              <p className="m-0 px-[12px] py-[14px] font-sans text-xs text-tx-3">
                {t('explore.red.breakdown.no_match')}
              </p>
            ) : (
              <>
                {common.length > 0 && (
                  <CommandGroup heading={t('explore.red.breakdown.common')}>{common.map(item)}</CommandGroup>
                )}
                {others.length > 0 && (
                  <CommandGroup heading={t('explore.red.breakdown.other')}>{others.map(item)}</CommandGroup>
                )}
              </>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
