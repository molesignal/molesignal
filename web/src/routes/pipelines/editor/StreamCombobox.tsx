import { Check, ChevronDown, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/shell/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { isStreamName } from '../PipelineGraph/streamName';

/**
 * A stream, found among those the workspace has or, where `creatable`, named
 * anew: a stream that does not exist yet is created by the first event
 * written to it. A name already chosen that the workspace does not list is
 * still shown, so an old pipeline does not open with a blank field.
 */
export function StreamCombobox({
  value,
  streams,
  onChange,
  creatable = false,
  pending = false,
  disabled = false,
}: {
  value: string;
  /** The names on offer. */
  streams: readonly string[];
  onChange: (stream: string) => void;
  creatable?: boolean;
  /** The list is still on its way. */
  pending?: boolean;
  disabled?: boolean;
}) {
  const { t } = useTranslation('pipelines');
  const [open, setOpen] = React.useState(false);
  const [search, setSearch] = React.useState('');
  const typed = search.trim();
  const matches = typed
    ? streams.filter((stream) => stream.toLowerCase().includes(typed.toLowerCase()))
    : streams;
  // A name the workspace does not have yet: the stream is made by what is written to it.
  const fresh = creatable && !streams.includes(typed) && isStreamName(typed);
  const choose = (stream: string) => {
    onChange(stream);
    setOpen(false);
    setSearch('');
  };

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
          disabled={disabled}
          data-stream-combobox
          className="flex h-[44px] w-full min-w-0 items-center justify-between gap-2 rounded-md bg-[var(--control-surface)] px-3 text-left font-sans text-sm text-tx-0 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-50 data-[state=open]:bg-bg-3 sm:h-9 [&>svg]:data-[state=open]:text-indigo [&>svg]:data-[state=open]:opacity-100"
        >
          <span className={cn('min-w-0 flex-1 truncate', !value && 'text-tx-3')}>
            {value ||
              t(
                pending
                  ? 'graph.stream_loading'
                  : creatable
                    ? 'streams.choose_or_name'
                    : 'graph.stream_select_placeholder',
              )}
          </span>
          <ChevronDown aria-hidden className="h-[14px] w-[14px] shrink-0 opacity-50 transition-colors" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-[var(--radix-popover-trigger-width)] min-w-[260px] max-w-[calc(100vw-32px)] p-0"
      >
        {/* The list is narrowed here, so the offer of a new name is never filtered away. */}
        <Command shouldFilter={false}>
          <CommandInput
            value={search}
            onValueChange={setSearch}
            placeholder={t(creatable ? 'streams.search_or_name' : 'streams.search')}
            className="text-xs"
          />
          <CommandList className="max-h-[260px]">
            {matches.length === 0 && !fresh ? (
              <p className="m-0 px-[12px] py-[14px] font-sans text-xs leading-relaxed text-tx-3">
                {t(
                  creatable && typed
                    ? 'streams.name_invalid'
                    : typed
                      ? 'streams.no_match'
                      : creatable
                        ? 'streams.none_yet_creatable'
                        : 'graph.stream_empty',
                )}
              </p>
            ) : (
              <CommandGroup>
                {matches.map((stream) => (
                  <CommandItem key={stream} value={stream} onSelect={() => choose(stream)}>
                    <Check
                      aria-hidden
                      className={cn(
                        'h-3.5 w-3.5 shrink-0 text-indigo',
                        stream === value ? 'opacity-100' : 'opacity-0',
                      )}
                    />
                    <span className="truncate">{stream}</span>
                  </CommandItem>
                ))}
                {fresh && (
                  // No stream is named with a space, so this value is no stream's.
                  <CommandItem value={`${typed} `} onSelect={() => choose(typed)}>
                    <Plus aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-2" />
                    <span className="truncate">{t('streams.use_new', { name: typed })}</span>
                  </CommandItem>
                )}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
