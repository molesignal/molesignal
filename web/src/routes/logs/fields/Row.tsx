import { ChevronDown, Eye, EyeOff, Minus, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { FieldAnalysisTrigger } from '@/shell/query/fieldAnalysis/Trigger';
import {
  FIELD_CARD_CLASS,
  FIELD_CARD_HEAD_CLASS,
  FIELD_NEST_CLASS,
  FIELD_NESTED_ROW_CLASS,
} from '@/shell/query/fields/Group';
import { FieldTypeChip, type FieldTypeTone } from '@/shell/query/fields/TypeChip';
import { FieldValueList, type FieldValueItem } from '@/shell/query/FieldValues';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import type { LogFieldDef } from '../fieldQueryModel';
import { topLogFieldValues } from '../viewModel';

export const LOG_FIELD_TYPE_GLYPH: Record<LogFieldDef['type'], string> = {
  string: 'T',
  number: '#',
  timestamp: 'T',
  date: 'D',
  array: '[]',
  boolean: 'B',
  object: '{}',
};
export const LOG_FIELD_TYPE_TONE: Record<LogFieldDef['type'], FieldTypeTone> = {
  // Stays in sync with the JSON-tree value palette of the result rows: the same
  // data type reads as the same color whether it appears as a field-type tile
  // here or as a rendered value in a row: string green, number orange.
  string: 'green',
  number: 'orange',
  timestamp: 'green',
  date: 'green',
  array: 'purple',
  boolean: 'blue',
  object: 'purple',
};

const ROW_ACTION = 'grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-4 focus-visible:bg-bg-4';
/**
 * What can be asked of a field stands at the end of its row while the pointer is
 * on the row or the focus is in it, and takes no room otherwise: a name is cut
 * short for buttons that are there, not for the place where they would be. A row
 * that was clicked holds the focus, so Tab goes on to its buttons from there.
 */
const ROW_ACTIONS = 'hidden shrink-0 items-center group-hover:flex group-focus-within:flex';
/** The same for an action that stands outside that group: gone until the row is pointed at or holds the focus. */
const ROW_ACTION_AT_REST = 'hidden group-hover:grid group-focus-within:grid';

interface LogFieldRowProps {
  field: LogFieldDef;
  /** The name the row is listed under: the field's own, or the rest of it inside a group. */
  label?: string;
  /** The row is one of a group's, on the group's ground. */
  nested?: boolean;
  filterable: boolean;
  visible: boolean;
  expanded: boolean;
  /** The loaded records: an open row lists the field's most common values among them. */
  records: Record<string, unknown>[];
  /**
   * What the field holds across the results can be opened from the row. Absent
   * where there is nothing to analyse: no results, or a field of documents.
   */
  analysis?: { open: boolean; onToggle: () => void } | undefined;
  onExpandedChange: (open: boolean) => void;
  onToggleVisibility: (field: string) => void;
  onInsertFilter: (field: string) => void;
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
}

/** A field in the log explorer's field list: its type, how many records carry it, and its values. */
export function LogFieldRow({
  field,
  label = field.name,
  nested = false,
  filterable,
  visible,
  expanded,
  records,
  analysis,
  onExpandedChange,
  onToggleVisibility,
  onInsertFilter,
  onInsertValueFilter,
}: LogFieldRowProps) {
  const { t } = useTranslation('logs');
  const toggleLabel = visible
    ? t('explore.left_panel.hide_field_aria', { name: field.name })
    : t('explore.left_panel.show_field_aria', { name: field.name });
  const addLabel = filterable
    ? t('explore.left_panel.add_field_aria', { name: field.name })
    : t('explore.left_panel.json_query_required', { name: field.name });
  // Counted only for the row that is open: every row counting every record is the whole
  // result walked once per field on each render.
  const items = React.useMemo<FieldValueItem[]>(() => {
    if (!expanded) return [];

    return topLogFieldValues(records, field.name).map((topValue) => ({
      key: `${typeof topValue.value}:${topValue.label}`,
      label: topValue.label,
      count: topValue.count,
      share: records.length > 0 ? topValue.count / records.length : 0,
      actions: filterable
        ? [
            {
              id: 'include',
              icon: Plus,
              hint: t('explore.left_panel.include_value'),
              label: t('explore.left_panel.include_value_aria', { value: topValue.label }),
              onSelect: () => onInsertValueFilter(field.name, topValue.value, 'include'),
            },
            {
              id: 'exclude',
              icon: Minus,
              tone: 'remove' as const,
              hint: t('explore.left_panel.exclude_value'),
              label: t('explore.left_panel.exclude_value_aria', { value: topValue.label }),
              onSelect: () => onInsertValueFilter(field.name, topValue.value, 'exclude'),
            },
          ]
        : [],
    }));
  }, [expanded, field.name, filterable, onInsertValueFilter, records, t]);

  return (
    <div
      data-log-field-row={field.name}
      data-field-row
      // A field of the list is a card; a field under a group is a row on a ground of its own inside the group's card.
      className={nested ? FIELD_NESTED_ROW_CLASS : FIELD_CARD_CLASS}
    >
      {/* The head carries its own state, from edge to edge; the button inside has no fill of its
          own, which would light only the part of the row it covers. */}
      <div
        className={cn(
          'group flex items-center gap-1 pr-1 font-sans text-xs',
          nested ? 'min-h-8 rounded-[5px] pl-1.5 hover:bg-bg-3 has-[:focus-visible]:bg-bg-4' : cn('min-h-9 pl-2', FIELD_CARD_HEAD_CLASS),
        )}
      >
        <button
          type="button"
          onClick={() => onExpandedChange(!expanded)}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 py-1.5 text-left focus-visible:bg-transparent"
        >
          <FieldTypeChip glyph={LOG_FIELD_TYPE_GLYPH[field.type]} tone={LOG_FIELD_TYPE_TONE[field.type]} nested={nested} />
          <span className="min-w-0 truncate font-body text-tx-0" title={field.name}>
            {label}
          </span>
          {/* The row opens on its values: the arrow shows where the pointer or the keyboard is, and while it is open. */}
          <ChevronDown
            aria-hidden="true"
            className={cn(
              'h-3 w-3 shrink-0 text-tx-3 transition-transform',
              expanded ? 'rotate-180' : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100',
            )}
          />
          <span className="min-w-0 flex-1" />
          {/* A count is said where it tells something: a field that every loaded record carries
              would only repeat the number of records down the whole list. */}
          {Number(field.count) < records.length && (
            <span data-log-field-count className="type-micro shrink-0 font-mono tabular-nums text-tx-2">
              {field.count}
            </span>
          )}
        </button>
        {/* One button whether its panel is open or not, so the keyboard can be given back to it. It
            shows with the row's other actions, and stays in view for as long as its panel is open. */}
        {analysis && (
          <FieldAnalysisTrigger
            name={field.name}
            pressed={analysis.open}
            onPress={analysis.onToggle}
            className={cn(ROW_ACTION, !analysis.open && ROW_ACTION_AT_REST)}
          />
        )}
        <div className={ROW_ACTIONS}>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onToggleVisibility(field.name)}
                className={cn(ROW_ACTION, 'hover:text-tx-0')}
                aria-label={toggleLabel}
              >
                {visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
              </button>
            </TooltipTrigger>
            <TooltipContent>{toggleLabel}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => onInsertFilter(field.name)}
                disabled={!filterable}
                className={cn(
                  ROW_ACTION,
                  'hover:text-[var(--blue-ink)] disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3',
                )}
                aria-label={addLabel}
              >
                <Plus className="h-3 w-3" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{addLabel}</TooltipContent>
          </Tooltip>
        </div>
      </div>
      {expanded && (
        <div
          data-log-field-values={field.name}
          // What a field opens on is inside it, set in, on a ground of its own; a field under a
          // group stands on that ground already and takes the surface of the card instead.
          className={
            nested ? 'mx-[3px] mb-[3px] rounded-[4px] bg-[var(--functional-surface)] p-[4px]' : FIELD_NEST_CLASS
          }
        >
          {items.length === 0 ? (
            <div className="px-[6px] py-[6px] font-sans text-xs text-tx-2">
              {t('explore.left_panel.no_top_values')}
            </div>
          ) : (
            <FieldValueList items={items} />
          )}
        </div>
      )}
    </div>
  );
}
