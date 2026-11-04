import { Check, ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Command, CommandGroup, CommandInput, CommandItem, CommandList } from '@/shell/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

/**
 * A value to look for, chosen from the commonest the data has or typed: a field with a list of
 * suggestions that does not insist on them. What is typed is offered first, to be chosen like the
 * rest.
 */
export function ValueCombobox({
  value,
  onChange,
  suggestions,
  placeholder,
  label,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  suggestions: readonly string[];
  placeholder: string;
  label: string;
  className?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const [open, setOpen] = React.useState(false);
  const [typed, setTyped] = React.useState('');
  const text = typed.trim();

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setTyped('');
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          data-rum-analytics-value=""
          className={cn(
            'flex h-8 min-w-0 items-center justify-between gap-2 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs text-tx-1 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3',
            className,
          )}
        >
          <span className={cn('truncate text-left', value === '' && 'text-tx-3')}>{value === '' ? placeholder : value}</span>
          <ChevronDown aria-hidden className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[var(--radix-popover-trigger-width)] min-w-[280px] p-0">
        <Command>
          <CommandInput value={typed} onValueChange={setTyped} placeholder={t('analytics.value.search')} />
          <CommandList className="max-h-72">
            <CommandGroup>
              {text !== '' && !suggestions.includes(text) && (
                <CommandItem forceMount value={`__typed__${text}`} onSelect={() => choose(text)} className="min-h-9">
                  <span className="truncate">{t('analytics.value.use', { value: text })}</span>
                </CommandItem>
              )}
              {suggestions.map((suggestion) => (
                <CommandItem key={suggestion} value={suggestion} onSelect={() => choose(suggestion)} className="min-h-9">
                  <Check aria-hidden className={cn('h-4 w-4 shrink-0 text-indigo', suggestion === value ? 'opacity-100' : 'opacity-0')} />
                  <span className="truncate">{suggestion}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
