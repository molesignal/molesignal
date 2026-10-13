import { Plus, Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Popover, PopoverAnchor, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { FilterEditor } from './FilterEditor';
import {
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderStatement,
  clauseLabel,
  withClause,
  withoutClause,
  withoutUnsupported,
} from './model';

interface FilterBuilderProps {
  /** The field query, as text. The builder shows it and writes it back. */
  statement: string;
  onStatementChange: (statement: string) => void;
  dialect: BuilderDialect;
  fields: readonly BuilderField[];
  /** Values seen for a field, to pick from while writing a filter. */
  suggestValues?: ((field: string) => readonly string[]) | undefined;
  /** A field chosen elsewhere (the field list) to start a filter on. */
  pendingField?: string | null | undefined;
  onPendingFieldHandled?: (() => void) | undefined;
  /** Runs the query: Enter in the text box. */
  onRun?: (() => void) | undefined;
  /** Shown above the filters when there is something to say about them. */
  notice?: React.ReactNode;
  className?: string | undefined;
}

/** Which filter the editor is open on: one that exists, or a new one. */
type Editing = number | 'new' | null;

/**
 * A field query as a row of filters.
 *
 * It is not a second query: the filters are parsed from the statement the Code
 * editor holds and every change is written back to it, so the two modes can be
 * switched at any point. Whatever part of the statement is not a filter stays
 * visible, as written, instead of being dropped.
 */
export function FilterBuilder({
  statement,
  onStatementChange,
  dialect,
  fields,
  suggestValues,
  pendingField,
  onPendingFieldHandled,
  onRun,
  notice,
  className,
}: FilterBuilderProps) {
  const { t } = useTranslation('common');
  const parsed = React.useMemo(() => dialect.parse(statement), [dialect, statement]);
  const [editing, setEditing] = React.useState<Editing>(null);
  const [startField, setStartField] = React.useState<string | undefined>();
  // The text box is typed into freely and joins the statement when it is
  // committed: writing it back on every key would turn `status = 5` into a
  // filter halfway through the value.
  const [text, setText] = React.useState(parsed.text);

  // Whenever the statement changes, the box shows the search text it now has:
  // text committed as `status >= 500` has become a filter and leaves the box.
  React.useEffect(() => {
    setText((current) => (current.trim() === parsed.text ? current : parsed.text));
  }, [parsed.text, statement]);

  // A field picked in the field list opens the editor on it.
  React.useEffect(() => {
    if (!pendingField) return;
    setStartField(pendingField);
    setEditing('new');
    onPendingFieldHandled?.();
  }, [onPendingFieldHandled, pendingField]);

  // Enter runs the query, but only once the statement it just changed has
  // reached the page: `onRun` then closes over the new statement.
  const runPending = React.useRef(false);
  React.useEffect(() => {
    if (!runPending.current) return;
    runPending.current = false;
    onRun?.();
  });

  const write = (next: BuilderStatement, run = false) => {
    const written = dialect.serialize({ ...next, text });
    if (written === statement) {
      if (run) onRun?.();
      return;
    }
    runPending.current = run;
    onStatementChange(written);
  };

  const closeEditor = () => {
    setEditing(null);
    setStartField(undefined);
  };

  const editor = (initial: BuilderClause | undefined, index: number | null) => (
    <FilterEditor
      fields={fields}
      dialect={dialect}
      initial={initial}
      initialField={initial ? undefined : startField}
      suggestValues={suggestValues}
      onSubmit={(clause) => {
        write(withClause(parsed, index, clause));
        closeEditor();
      }}
      onCancel={closeEditor}
    />
  );

  return (
    <div data-query-builder className={cn('px-3 py-2.5', className)}>
      {notice ? (
        <p className="mb-2 font-sans text-xs leading-relaxed text-tx-2">{notice}</p>
      ) : null}
      <div
        role="group"
        aria-label={t('query_builder.filters')}
        className="flex flex-wrap items-center gap-[6px]"
      >
        {parsed.clauses.map((clause, index) => {
          const label = clauseLabel(clause);
          return (
            <Popover
              key={`${index}:${label}`}
              open={editing === index}
              onOpenChange={(open) => (open ? setEditing(index) : closeEditor())}
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
                      <span className="max-w-[260px] truncate font-semibold text-tx-0">
                        {clause.value}
                      </span>
                    </button>
                  </PopoverTrigger>
                  <button
                    type="button"
                    aria-label={t('query_builder.remove', { filter: label })}
                    onClick={() => write(withoutClause(parsed, index))}
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

        {parsed.unsupported.map((part, index) => (
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
              onClick={() => write(withoutUnsupported(parsed, index))}
              className="grid h-full w-[24px] shrink-0 place-items-center rounded-r-md hover:bg-yellow/20 focus-visible:bg-yellow/20"
            >
              <X className="h-3 w-3" aria-hidden="true" />
            </button>
          </span>
        ))}

        <Popover
          open={editing === 'new'}
          onOpenChange={(open) => (open ? setEditing('new') : closeEditor())}
        >
          <PopoverTrigger asChild>
            <button
              type="button"
              data-testid="query-builder-add"
              aria-label={t('query_builder.add_aria')}
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

        <label className="flex h-[28px] min-w-[200px] flex-1 items-center gap-2 rounded-md bg-[var(--functional-surface)] px-2.5 focus-within:bg-bg-2 hover:bg-bg-2">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
          <input
            value={text}
            onChange={(event) => setText(event.target.value)}
            onBlur={() => write(parsed)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                write(parsed, true);
              }
              // Backspace in an empty box takes back the last filter.
              if (event.key === 'Backspace' && text === '' && parsed.clauses.length > 0) {
                write(withoutClause(parsed, parsed.clauses.length - 1));
              }
            }}
            aria-label={t('query_builder.text')}
            placeholder={t('query_builder.text_placeholder')}
            className="min-w-0 flex-1 bg-transparent font-mono text-xs text-tx-0 outline-none placeholder:font-sans placeholder:text-tx-3"
          />
        </label>
      </div>
    </div>
  );
}
