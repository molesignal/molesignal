import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

/**
 * How many labels a field is sized by: the longest ones. The width is the
 * widest of them as drawn, so a list of thousands costs no more than this.
 */
const SIZING_LABELS = 50;

/** About how wide a label is drawn, in Latin letters: a CJK character takes about two. */
function roughWidth(label: string): number {
  let width = 0;

  for (const character of label) width += (character.codePointAt(0) ?? 0) > 0x2e7f ? 2 : 1;

  return width;
}

/** The labels a field must be wide enough for, longest first. */
export function sizingLabels(labels: readonly string[]): string[] {
  return [...new Set(labels)]
    .sort((a, b) => roughWidth(b) - roughWidth(a))
    .slice(0, SIZING_LABELS);
}

export interface FittedSelectOption {
  value: string;
  label: string;
  /** Offered, but set back: a choice that leads to nothing just now. */
  muted?: boolean | undefined;
  disabled?: boolean | undefined;
}

/** A line between two groups of choices. */
export const FITTED_SELECT_SEPARATOR = 'separator';

export type FittedSelectEntry = FittedSelectOption | typeof FITTED_SELECT_SEPARATOR;

/** Says a label in full under the pointer where it is cut short, and only there. */
function tellWhenCut(event: React.PointerEvent<HTMLElement>) {
  const labels = [...event.currentTarget.querySelectorAll<HTMLElement>('[data-fitted-label]')];
  const cut = labels.some((label) => label.scrollWidth > label.clientWidth);

  if (cut) event.currentTarget.title = labels[0]?.textContent ?? '';
  else event.currentTarget.removeAttribute('title');
}

/**
 * A select as wide as the longest of its choices, up to a width it is given.
 *
 * It leaves the row the room short choices do not need, and does not change
 * width when another choice is made. A label longer than the width allows is
 * cut with an ellipsis, in the field and in the list, and said in full under
 * the pointer. The list opens exactly as wide as the field.
 */
export function FittedSelect({
  value,
  onValueChange,
  options,
  maxWidth,
  placeholder,
  prefix,
  className,
  ...trigger
}: {
  value: string;
  onValueChange: (value: string) => void;
  options: readonly FittedSelectEntry[];
  /** The widest the field and its list get, in px. */
  maxWidth: number;
  placeholder?: string | undefined;
  /**
   * What the value is a value of, said before it in the field: "Version v2.4.0".
   * For a field with no caption, where a bare value would not say what it narrows.
   */
  prefix?: string | undefined;
  className?: string | undefined;
} & Omit<React.ComponentPropsWithoutRef<typeof SelectTrigger>, 'children' | 'className' | 'value'>) {
  const widest = React.useMemo(
    () =>
      sizingLabels([
        ...options.flatMap((option) => (option === FITTED_SELECT_SEPARATOR ? [] : [option.label])),
        ...(placeholder ? [placeholder] : []),
      ]),
    [options, placeholder],
  );

  return (
    <Select value={value} onValueChange={onValueChange}>
      <SelectTrigger
        data-fitted-select
        onPointerEnter={tellWhenCut}
        {...trigger}
        // A row of the list is its label with 48px around it (the list's edge, the tick before the
        // label, the row's end). The field has the same around its longest label and 1px to spare,
        // its arrow and the gap before it included, so no row is cut while the field is not.
        className={cn(
          'h-8 w-auto shrink-0 gap-[calc(35px-1.25rem)] px-2.5 font-sans text-xs',
          className,
        )}
        style={{ maxWidth }}
      >
        {/* One cell holds the chosen label and, unseen, the longest labels of the list: it is as wide as the widest. */}
        <span className="grid min-w-0 text-left">
          <span className="col-start-1 row-start-1 flex min-w-0 items-center gap-1.5">
            {prefix ? (
              <span data-fitted-prefix className="shrink-0 font-body text-tx-3">
                {prefix}
              </span>
            ) : null}
            <span data-fitted-label className="min-w-0 truncate">
              <SelectValue placeholder={placeholder} />
            </span>
          </span>
          {widest.map((label) => (
            <span
              key={label}
              aria-hidden="true"
              // Laid out as the cell above is, so it is as wide as that label would be in it.
              className="invisible col-start-1 row-start-1 flex h-0 items-center gap-1.5 overflow-hidden whitespace-nowrap"
            >
              {prefix ? <span data-label={prefix} className="font-body before:content-[attr(data-label)]" /> : null}
              {/* Sized in the heaviest weight a row is drawn in: the chosen row of the list. */}
              <span data-label={label} data-fitted-sizer className="font-strong before:content-[attr(data-label)]" />
            </span>
          ))}
        </span>
      </SelectTrigger>
      <SelectContent data-fitted-select-list className="w-[var(--radix-select-trigger-width)] min-w-0">
        {options.map((option, index) =>
          option === FITTED_SELECT_SEPARATOR ? (
            // A separator has nothing of its own to be told by but its place.
            <SelectSeparator key={`separator-${index}`} />
          ) : (
            <SelectItem
              key={option.value}
              value={option.value}
              disabled={option.disabled === true}
              data-fitted-muted={option.muted ? 'true' : undefined}
              onPointerEnter={tellWhenCut}
              // The label is the row's only part in the flow: it gives way, so a long one is cut, not the row.
              className={cn('text-xs [&>span:last-child]:min-w-0', option.muted && 'text-tx-3')}
            >
              <span data-fitted-label className="block truncate">
                {option.label}
              </span>
            </SelectItem>
          ),
        )}
      </SelectContent>
    </Select>
  );
}
