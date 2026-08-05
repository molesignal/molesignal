import { Check, ChevronDown, Search, X } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/shell/ui/popover';
import type { TimeSeriesLegendStat } from '@/viz/timeseries/types';

export const LEGEND_STAT_VALUES: readonly TimeSeriesLegendStat[] = [
  'last',
  'min',
  'max',
  'mean',
  'sum',
];

interface LegendStatsControlProps {
  value: readonly TimeSeriesLegendStat[];
  label: string;
  placeholder: string;
  searchPlaceholder: string;
  emptyLabel: string;
  removeText: string;
  optionLabel: (value: TimeSeriesLegendStat) => string;
  onChange: (value: TimeSeriesLegendStat[]) => void;
}

export function LegendStatsControl({
  value,
  label,
  placeholder,
  searchPlaceholder,
  emptyLabel,
  removeText,
  optionLabel,
  onChange,
}: LegendStatsControlProps) {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState('');
  const selected = LEGEND_STAT_VALUES.filter((stat) => value.includes(stat));
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleOptions = LEGEND_STAT_VALUES.filter((stat) => {
    if (!normalizedQuery) return true;
    return `${optionLabel(stat)} ${stat}`
      .toLocaleLowerCase()
      .includes(normalizedQuery);
  });
  const selectedLabel = selected.map(optionLabel).join(', ');

  const toggle = (stat: TimeSeriesLegendStat) => {
    const next = new Set(selected);
    if (next.has(stat)) next.delete(stat);
    else next.add(stat);
    onChange(LEGEND_STAT_VALUES.filter((candidate) => next.has(candidate)));
  };

  const remove = (stat: TimeSeriesLegendStat) => {
    onChange(selected.filter((candidate) => candidate !== stat));
  };

  return (
    <Popover
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (!nextOpen) setQuery('');
      }}
    >
      <div className="relative min-w-0 rounded-md border border-bd-1 bg-bg-1">
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            aria-label={selectedLabel ? `${label}: ${selectedLabel}` : label}
            aria-expanded={open}
            className="peer absolute inset-0 rounded-md bg-transparent outline-none hover:bg-bg-2 focus-visible:bg-bg-2 data-[state=open]:bg-bg-2"
          />
        </PopoverTrigger>
        <div className="pointer-events-none relative z-10 flex min-h-11 min-w-0 items-center gap-1.5 px-2 py-1 sm:min-h-8">
          <span className="flex min-w-0 flex-1 flex-wrap gap-1">
            {selected.length ? (
              selected.map((stat) => {
                const text = optionLabel(stat);
                return (
                  <span
                    key={stat}
                    className="pointer-events-none inline-flex h-6 max-w-full items-center gap-1 rounded-sm bg-bg-3 pl-1.5 pr-1 font-sans text-type-micro font-semibold text-tx-1"
                  >
                    <span className="truncate">{text}</span>
                    <button
                      type="button"
                      aria-label={`${removeText} ${text}`}
                      onClick={() => remove(stat)}
                      className="pointer-events-auto inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-sm text-tx-3 outline-none hover:bg-bg-4 hover:text-tx-0 focus-visible:bg-bg-4 focus-visible:text-tx-0"
                    >
                      <X className="h-3 w-3" aria-hidden="true" />
                    </button>
                  </span>
                );
              })
            ) : (
              <span className="self-center truncate font-sans text-xs text-tx-3">
                {placeholder}
              </span>
            )}
          </span>
          <ChevronDown
            className={cn(
              'h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform',
              open && 'rotate-180',
            )}
            aria-hidden="true"
          />
        </div>
      </div>
      <PopoverContent
        align="end"
        className="w-[var(--radix-popover-trigger-width)] min-w-48 p-0 shadow-none"
      >
        <label className="flex h-11 items-center gap-2 border-b border-bd-0 px-2.5 sm:h-9">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label={searchPlaceholder}
            placeholder={searchPlaceholder}
            className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-1 outline-none placeholder:text-tx-3 sm:text-xs"
          />
        </label>
        <div
          role="listbox"
          aria-label={label}
          aria-multiselectable="true"
          className="max-h-64 overflow-y-auto p-1"
        >
          {visibleOptions.length ? (
            visibleOptions.map((stat) => {
              const checked = selected.includes(stat);
              return (
                <button
                  key={stat}
                  type="button"
                  role="option"
                  aria-selected={checked}
                  onClick={() => toggle(stat)}
                  className={cn(
                    'flex min-h-11 w-full items-center gap-2 rounded-sm px-2 text-left font-sans text-sm outline-none sm:min-h-8 sm:text-xs',
                    checked
                      ? 'bg-bg-2 text-tx-0'
                      : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0',
                    'focus-visible:bg-bg-2 focus-visible:text-tx-0',
                  )}
                >
                  <Check
                    className={cn(
                      'h-3.5 w-3.5 shrink-0 text-indigo-soft',
                      checked ? 'opacity-100' : 'opacity-0',
                    )}
                    aria-hidden="true"
                  />
                  <span>{optionLabel(stat)}</span>
                </button>
              );
            })
          ) : (
            <p className="px-2 py-5 text-center font-sans text-xs text-tx-3">
              {emptyLabel}
            </p>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
