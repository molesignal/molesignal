import type { TFunction } from 'i18next';
import { Layers3, Plus, RotateCcw, X } from 'lucide-react';
import type * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import type { ChatContext } from '../workspace/types';

export function ContextChip({
  label,
  onRemove,
}: {
  label: string;
  onRemove?: () => void;
}) {
  return (
    <span className="inline-flex h-7 max-w-[240px] items-center gap-1 rounded-md border border-bd-0 bg-bg-2 px-2 font-sans type-micro text-tx-2">
      <span className="truncate">{label}</span>
      {onRemove && (
        <button
          type="button"
          aria-label={label}
          onClick={onRemove}
          className="-mr-1 grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </span>
  );
}

export function ContextEditorPopover({
  t,
  context,
  onChange,
  triggerClassName,
  side = 'top',
}: {
  t: TFunction<'agent'>;
  context: ChatContext;
  onChange: React.Dispatch<React.SetStateAction<ChatContext>>;
  triggerClassName?: string;
  side?: 'top' | 'bottom';
}) {
  const setField = (field: keyof ChatContext, value: string) => {
    onChange((current) => ({ ...current, [field]: value }));
  };
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            'inline-flex shrink-0 items-center gap-1.5 rounded-md px-2 font-sans text-xs font-strong text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
            triggerClassName,
          )}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('context.add')}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" side={side} className="w-[340px] max-w-[calc(100vw-24px)] p-4 !shadow-lg">
        <div className="flex items-start gap-3">
          <Layers3 className="mt-0.5 h-4 w-4 shrink-0 text-indigo" />
          <div>
            <h3 className="text-sm font-strong text-tx-0">
              {t('context.editor_title')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('context.editor_description')}
            </p>
          </div>
        </div>
        <div className="mt-4 space-y-3">
          {(['environment', 'service', 'alert'] as const).map((field) => (
            <label key={field} className="block">
              <span className="mb-1.5 block text-xs font-strong text-tx-2">
                {t(`context.${field}`)}
              </span>
              <input
                value={context[field]}
                onChange={(event) => setField(field, event.target.value)}
                placeholder={t(`context.${field}_placeholder`)}
                className="h-9 w-full rounded-md border border-bd-1 bg-[var(--control-surface)] px-3 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
              />
            </label>
          ))}
        </div>
        <button
          type="button"
          onClick={() =>
            onChange({ environment: '', service: '', alert: '' })
          }
          className="mt-3 inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-xs text-tx-3 hover:bg-bg-2 hover:text-tx-0"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          {t('context.clear')}
        </button>
      </PopoverContent>
    </Popover>
  );
}
