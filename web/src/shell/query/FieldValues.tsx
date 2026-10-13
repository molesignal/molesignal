import type { LucideIcon } from 'lucide-react';

import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

/** Something a value in the field list can be asked of: kept, left out, taken as a bound. */
export interface FieldValueAction {
  id: string;
  /** What the button does, said with the value: its accessible name. */
  label: string;
  /** The same, short, for the tooltip. */
  hint: string;
  /** An icon, or a one-character sign such as ≥. */
  icon: LucideIcon | string;
  /** A filter that takes away reads warmer than one that narrows down. */
  tone?: 'narrow' | 'remove';
  /** The filter is in the query: the button stays in view, and takes it out again. */
  pressed?: boolean;
  onSelect: () => void;
}

export interface FieldValueItem {
  key: string;
  /** What the value is of, for a row that needs saying: Min, Max. */
  caption?: string;
  label: string;
  /** The value in full, when the label is shortened or reworded. */
  title?: string;
  /** How many of the results have it. */
  count?: number;
  /** The count as it is written, where it runs to more figures than are read at a glance. */
  countLabel?: string;
  /** What the count is of, in words. */
  countTitle?: string;
  /** The share of the results that have it, 0 to 1: the bar under the value. */
  share?: number;
  /** The colour the value is drawn in on a chart beside the list: the row is its legend. */
  swatch?: string;
  actions: FieldValueAction[];
}

/**
 * The values of a field under its row in an explorer's field list: each with
 * how common it is among the results, and what it can be asked of.
 *
 * The bar under a value is its share of the results, so a list reads as a
 * distribution at a glance: a track as long as the value and its count, filled
 * as far as the share goes, in the blue the charts beside the list count in.
 * It stands under the words, not behind them, so neither has to be read
 * through the other. The actions keep their place whether or not a row has
 * them, which keeps the counts in one column.
 */
export function FieldValueList({ items, className }: { items: FieldValueItem[]; className?: string }) {
  const slots = Math.max(0, ...items.map((item) => item.actions.length));

  return (
    <ul className={cn('flex flex-col gap-px', className)}>
      {items.map((item) => (
        <li
          key={item.key}
          data-field-value={item.title ?? item.label}
          className="group/value relative flex min-h-[28px] items-center gap-[6px] rounded-[4px] pl-[6px] pr-[2px] hover:bg-bg-3 has-[:focus-visible]:bg-bg-3"
        >
          {item.swatch && (
            <span
              aria-hidden="true"
              data-field-value-swatch
              className="relative h-[8px] w-[8px] shrink-0 rounded-full"
              style={{ background: item.swatch }}
            />
          )}
          {item.caption && (
            <span className="relative w-[52px] shrink-0 truncate font-sans text-xs text-tx-3">{item.caption}</span>
          )}
          <span className="relative flex min-w-0 flex-1 flex-col justify-center gap-[2px] py-[3px]">
            <span className="flex min-w-0 items-baseline gap-[6px]">
              <span
                className={cn(
                  'min-w-0 flex-1 truncate font-mono text-xs',
                  item.actions.some((action) => action.pressed) ? 'font-strong text-tx-0' : 'text-tx-1',
                )}
                title={item.title ?? item.label}
              >
                {item.label}
              </span>
              {item.count !== undefined && (
                <span className="shrink-0 font-mono text-xs tabular-nums text-tx-2" title={item.countTitle}>
                  {item.countLabel ?? item.count}
                </span>
              )}
            </span>
            {item.share !== undefined && (
              <span
                aria-hidden="true"
                data-field-value-track
                className="block h-[3px] overflow-hidden rounded-full bg-bd-1"
              >
                <span
                  data-field-value-share
                  className="block h-full rounded-full bg-blue"
                  // The least share still shows as a mark at the start of the track.
                  style={{ width: `${Math.min(100, Math.max(2, item.share * 100))}%` }}
                />
              </span>
            )}
          </span>
          {slots > 0 && (
            <span className="relative flex shrink-0 justify-end" style={{ width: slots * 24 }}>
              {item.actions.map((action) => (
                <FieldValueButton key={action.id} action={action} />
              ))}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

function FieldValueButton({ action }: { action: FieldValueAction }) {
  const Icon = action.icon;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={action.onSelect}
          aria-label={action.label}
          {...(action.pressed === undefined ? {} : { 'aria-pressed': action.pressed })}
          data-field-value-action={action.id}
          className={cn(
            'grid h-6 w-6 place-items-center rounded font-mono text-xs leading-none hover:bg-bg-4 focus-visible:bg-bg-4',
            action.pressed
              ? action.tone === 'remove' ? 'bg-[var(--orange-tint)] text-[var(--orange-ink)]' : 'bg-[var(--blue-tint)] text-[var(--blue-ink)]'
              // Shown with the pointer over the row, or under the keyboard. Where nothing hovers
              // (a touch screen) they are always there.
              : cn(
                  'text-tx-3 [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:focus-visible:opacity-100 [@media(hover:hover)]:group-hover/value:opacity-100',
                  action.tone === 'remove'
                    ? 'hover:text-[var(--orange-ink)] focus-visible:text-[var(--orange-ink)]'
                    : 'hover:text-[var(--blue-ink)] focus-visible:text-[var(--blue-ink)]',
                ),
          )}
        >
          {typeof Icon === 'string' ? <span aria-hidden="true">{Icon}</span> : <Icon className="h-3 w-3" aria-hidden="true" />}
        </button>
      </TooltipTrigger>
      <TooltipContent>{action.hint}</TooltipContent>
    </Tooltip>
  );
}
