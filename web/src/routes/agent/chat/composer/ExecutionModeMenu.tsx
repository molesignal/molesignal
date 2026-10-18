import type { TFunction } from 'i18next';
import { Check, ChevronDown, Lock } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItemIndicator,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { EXECUTION_POLICIES, type ExecutionPolicy } from '../workspace/types';

/** The mode a new chat starts in. */
const RECOMMENDED: ExecutionPolicy = 'advice_only';

/**
 * What Mole Agent may run in this chat. Each mode says in a line what it does,
 * the recommended one is marked, and its number picks it while the menu is open.
 * Bypass permissions stays locked until the chat's Agent Profile allows it, and
 * reads in the warning tone once it is on.
 */
export function ExecutionModeMenu({
  t,
  value,
  onChange,
  bypassAllowed,
}: {
  t: TFunction<'agent'>;
  value: ExecutionPolicy;
  onChange: (value: ExecutionPolicy) => void;
  bypassAllowed: boolean;
}) {
  const [open, setOpen] = React.useState(false);
  const locked = (policy: ExecutionPolicy) => policy === 'bypass_permissions' && !bypassAllowed;
  const choose = (policy: ExecutionPolicy) => {
    if (locked(policy)) return;
    onChange(policy);
    setOpen(false);
  };

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger
        aria-label={t('controls.execution')}
        data-agent-execution-mode={value}
        className="inline-flex h-10 max-w-[220px] shrink-0 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3 md:h-8"
      >
        <span
          className={cn(
            'truncate',
            value === 'bypass_permissions' && 'text-[var(--orange-ink)]',
          )}
        >
          {t(`execution_policy.${value}`)}
        </span>
        <ChevronDown aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-indigo" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align="start"
        sideOffset={6}
        className="w-[300px] max-w-[calc(100vw-32px)] !shadow-lg"
        onKeyDown={(event) => {
          if (event.metaKey || event.ctrlKey || event.altKey) return;
          const policy = EXECUTION_POLICIES[Number(event.key) - 1];
          if (!policy) return;
          event.preventDefault();
          choose(policy);
        }}
      >
        <DropdownMenuLabel>{t('execution_mode.title')}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={value}
          onValueChange={(next) => choose(next as ExecutionPolicy)}
        >
          {EXECUTION_POLICIES.map((policy, index) => (
            <DropdownMenuRadioItem
              key={policy}
              value={policy}
              disabled={locked(policy)}
              indicator="none"
              className="items-start gap-3 py-[6px] data-[state=checked]:bg-transparent data-[state=checked]:font-body data-[state=checked]:text-tx-1"
            >
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5">
                  <span className="font-strong text-tx-0">{t(`execution_policy.${policy}`)}</span>
                  {policy === RECOMMENDED && (
                    <span className="type-micro rounded bg-bg-3 px-1.5 text-tx-2">
                      {t('execution_mode.recommended')}
                    </span>
                  )}
                </span>
                <span className="mt-0.5 block text-tx-3">
                  {t(`execution_mode.descriptions.${policy}`)}
                </span>
                {locked(policy) && (
                  <span
                    data-agent-mode-locked
                    className="mt-0.5 flex items-center gap-1 type-micro text-tx-3"
                  >
                    <Lock aria-hidden="true" className="h-3 w-3 shrink-0" />
                    {t('execution_mode.bypass_locked')}
                  </span>
                )}
              </span>
              <span className="flex shrink-0 items-center gap-2 pt-0.5">
                <DropdownMenuItemIndicator>
                  <Check aria-hidden="true" className="h-3.5 w-3.5 text-indigo-soft" />
                </DropdownMenuItemIndicator>
                <kbd className="type-micro font-sans tabular-nums text-tx-3">{index + 1}</kbd>
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
