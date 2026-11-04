import { ChevronDown } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { FieldTypeChip } from './TypeChip';

/**
 * A field of a field list: a small card lifted off the ground of the list. What
 * the field opens on (the fields under it, or its values) is inside the card.
 */
export const FIELD_CARD_CLASS = 'mb-1.5 flow-root rounded-md bg-[var(--functional-surface)] shadow-sm';
/** A field under a group: a row on a ground of its own inside the group's card, not a card itself. */
export const FIELD_NESTED_ROW_CLASS = 'flow-root rounded-[5px] bg-[var(--control-surface)]';
/** The head of a card: the row that names the field, lit as a whole under the pointer or the keyboard. */
export const FIELD_CARD_HEAD_CLASS = 'rounded-md hover:bg-bg-2 has-[:focus-visible]:bg-bg-3';
/** What a card opens on, under its head: set in from the card's edge and on a ground of its own. */
export const FIELD_NEST_CLASS = 'mx-1.5 mb-1.5 mt-0.5 rounded-[5px] bg-[var(--control-surface)] p-[4px]';
/** The ground the cards of a list stand on: a step off the panel, so a white card reads as lifted. */
export const FIELD_LIST_GROUND_CLASS = 'rounded-md bg-[var(--page-canvas)] px-1.5 pt-0.5';

/**
 * A field of a flat field list: a row from edge to edge with a line under it,
 * as a row of the table beside the list is, so list and table read as one
 * surface. What the field opens on is under its row.
 */
export const FIELD_FLAT_ROW_CLASS = 'flow-root border-b border-[var(--row-rule)]';
/**
 * The head of a flat row: the line that names the field, lit as a whole under the pointer or the
 * keyboard. Its name sets its height, as a table row's line does: 12px on an 18px line in Logs,
 * Traces and RUM alike.
 */
export const FIELD_FLAT_HEAD_CLASS = 'font-sans text-xs hover:bg-bg-2 has-[:focus-visible]:bg-bg-3';
/** The line inside a flat row's head, and a flat group's: 6px over and under the 18px name, so a row with the rule under it is 31px. */
export const FIELD_FLAT_LINE_CLASS = 'py-[6px]';
/** What a flat row opens on, under its head: on a ground a step off the list. */
export const FIELD_FLAT_NEST_CLASS = 'border-t border-bd-0 bg-[var(--page-canvas)] px-3 py-1.5';

interface FieldGroupProps {
  /** The first part the fields share. */
  name: string;
  /** How many fields are under it. */
  count: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The group said in full, for a reader who cannot see the count beside the name. */
  label: string;
  /** The fields, each a row of the list it is in. */
  children: React.ReactNode;
  /** A card lifted off the list's ground, or a row of a flat list with its fields in rows under it. */
  variant?: 'card' | 'flat';
}

/**
 * Fields that share the first part of their names, as one card of a field
 * list. It says how many they are and opens on them: the fields stand inside
 * the card, each a row on a ground of its own, so what belongs to what is seen
 * and not read out of the names.
 */
export function FieldGroup({ name, count, open, onOpenChange, label, children, variant = 'card' }: FieldGroupProps) {
  const id = React.useId();
  const flat = variant === 'flat';

  return (
    // A flat group's rows draw their own lines: its head carries the one under it.
    <div data-field-group={name} className={flat ? 'flow-root' : FIELD_CARD_CLASS}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={label}
        onClick={() => onOpenChange(!open)}
        className={cn(
          'flex w-full items-center gap-2 text-left font-sans text-xs hover:bg-bg-2 focus-visible:bg-bg-3',
          flat ? cn('border-b border-bd-0 px-3', FIELD_FLAT_LINE_CLASS) : 'min-h-9 rounded-md px-2',
        )}
      >
        <FieldTypeChip glyph="{}" tone="neutral" />
        <span className="min-w-0 truncate font-body text-tx-0">{name}</span>
        <span
          data-field-group-count
          className="type-micro shrink-0 rounded-[4px] bg-bg-3 px-[5px] tabular-nums leading-[16px] text-tx-2"
        >
          {count}
        </span>
        <ChevronDown
          aria-hidden="true"
          className={cn('h-3 w-3 shrink-0 text-tx-3 transition-transform', open && 'rotate-180')}
        />
      </button>
      {open && (
        // Each field under the group is a row on a ground of its own inside the group's card, or a row of the flat list set in under it.
        <div id={id} data-field-group-fields={name} className={flat ? 'flex flex-col' : 'mx-1.5 flex flex-col gap-1 pb-1.5 pt-0.5'}>
          {children}
        </div>
      )}
    </div>
  );
}
