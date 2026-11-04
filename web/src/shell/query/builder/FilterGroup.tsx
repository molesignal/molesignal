import { Plus, Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { Popover, PopoverAnchor, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { type BuilderClause, type BuilderGroup, clauseLabel } from './model';

/** Which filter of the group the editor is open on: one that exists, or a new one. */
export type GroupEditing = number | 'new' | null;

interface FilterGroupProps {
  group: BuilderGroup;
  /** Where the group stands among the alternatives, from 0. */
  position: number;
  /** How many alternatives the statement has. */
  count: number;
  /** The text box's content, typed and not yet written into the statement. */
  text: string;
  onTextChange: (text: string) => void;
  /** Writes the typed text into the statement; Enter also runs the query. */
  onTextCommit: (run: boolean) => void;
  editing: GroupEditing;
  onEditingChange: (editing: GroupEditing) => void;
  /** The editor for a filter of this group, or for a new one (`null`). */
  editor: (initial: BuilderClause | undefined, index: number | null) => React.ReactNode;
  onRemoveClause: (index: number) => void;
  onRemoveUnsupported: (index: number) => void;
  /** Shown after the group's own button: what adds another alternative. */
  trailing?: React.ReactNode;
}

/**
 * One alternative of a field query as a row of filters: its filters, what it
 * holds that is no filter, a button for one more, and its text to find. After
 * the first, a row says "or".
 */
export function FilterGroup({
  group,
  position,
  count,
  text,
  onTextChange,
  onTextCommit,
  editing,
  onEditingChange,
  editor,
  onRemoveClause,
  onRemoveUnsupported,
  trailing,
}: FilterGroupProps) {
  const { t } = useTranslation('common');
  const several = count > 1;
  const close = () => onEditingChange(null);

  const parts = (
    <>
      {group.clauses.map((clause, index) => {
        const label = clauseLabel(clause);
        return (
          <Popover
            key={`${index}:${label}`}
            open={editing === index}
            onOpenChange={(open) => (open ? onEditingChange(index) : close())}
          >
            <PopoverAnchor asChild>
              <span
                data-query-filter
                className="inline-flex h-[28px] max-w-full items-center rounded-md bg-[var(--functional-surface)] font-mono text-xs"
              >
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    aria-label={t('query_builder.edit', { filter: label })}
                    title={label}
                    className="flex h-full min-w-0 items-center gap-1.5 rounded-l-md pl-2.5 pr-1.5 hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    <span className="truncate text-tx-2">{clause.field}</span>
                    <span className="shrink-0 font-semibold text-indigo-soft">{clause.op}</span>
                    <span className="max-w-[260px] truncate font-semibold text-tx-0">{clause.value}</span>
                  </button>
                </PopoverTrigger>
                <button
                  type="button"
                  aria-label={t('query_builder.remove', { filter: label })}
                  onClick={() => onRemoveClause(index)}
                  className="grid h-full w-[24px] shrink-0 place-items-center rounded-r-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
                >
                  <X className="h-3 w-3" aria-hidden="true" />
                </button>
              </span>
            </PopoverAnchor>
            <PopoverContent align="start" className="w-auto p-0">
              {editor(clause, index)}
            </PopoverContent>
          </Popover>
        );
      })}

      {group.unsupported.map((part, index) => (
        <span
          key={`${index}:${part}`}
          data-query-filter-unsupported
          title={t('query_builder.unsupported')}
          className="inline-flex h-[28px] max-w-full items-center rounded-md bg-[var(--yellow-tint)] pl-2.5 font-mono text-xs text-[var(--yellow-ink)]"
        >
          <span className="max-w-[320px] truncate">{part}</span>
          <button
            type="button"
            aria-label={t('query_builder.remove_unsupported', { part })}
            onClick={() => onRemoveUnsupported(index)}
            className="grid h-full w-[24px] shrink-0 place-items-center rounded-r-md hover:bg-yellow/20 focus-visible:bg-yellow/20"
          >
            <X className="h-3 w-3" aria-hidden="true" />
          </button>
        </span>
      ))}

      <Popover open={editing === 'new'} onOpenChange={(open) => (open ? onEditingChange('new') : close())}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="query-builder-add"
            aria-label={several ? t('query_builder.add_in_group', { number: position + 1 }) : t('query_builder.add_aria')}
            className="inline-flex h-[28px] shrink-0 items-center gap-1 rounded-md bg-[var(--functional-surface)] pl-2 pr-2.5 font-sans text-xs font-strong text-tx-1 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 data-[state=open]:bg-bg-2"
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            {t('query_builder.add')}
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-auto p-0">
          {editor(undefined, null)}
        </PopoverContent>
      </Popover>

      {trailing}

      <label className="flex h-[28px] min-w-[200px] flex-1 items-center gap-2 rounded-md bg-[var(--functional-surface)] px-2.5 focus-within:bg-bg-2 hover:bg-bg-2">
        <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
        <input
          value={text}
          onChange={(event) => onTextChange(event.target.value)}
          onBlur={() => onTextCommit(false)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onTextCommit(true);
            }
            // Backspace in an empty box takes back the group's last filter.
            if (event.key === 'Backspace' && text === '' && group.clauses.length > 0) {
              onRemoveClause(group.clauses.length - 1);
            }
          }}
          aria-label={several ? t('query_builder.text_in_group', { number: position + 1 }) : t('query_builder.text')}
          placeholder={t('query_builder.text_placeholder')}
          className="min-w-0 flex-1 bg-transparent font-mono text-xs text-tx-0 outline-none placeholder:font-sans placeholder:text-tx-3"
        />
      </label>
    </>
  );

  if (!several) {
    return (
      <div
        role="group"
        data-query-builder-group={position}
        aria-label={t('query_builder.filters')}
        className="flex flex-wrap items-center gap-[6px]"
      >
        {parts}
      </div>
    );
  }

  // Every row keeps a column for the word: the filters of all rows start at one edge.
  return (
    <div
      role="group"
      data-query-builder-group={position}
      aria-label={t('query_builder.group', { number: position + 1 })}
      className="flex items-start gap-[6px]"
    >
      <span
        {...(position > 0 ? { 'data-query-builder-or': '' } : { 'aria-hidden': true })}
        className="flex h-[28px] w-6 shrink-0 items-center font-sans text-xs font-strong text-tx-2"
      >
        {position > 0 ? t('query_builder.or') : null}
      </span>
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-[6px]">{parts}</div>
    </div>
  );
}
