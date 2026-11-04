import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { FilterEditor } from './FilterEditor';
import { FilterGroup, type GroupEditing } from './FilterGroup';
import {
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderStatement,
  EMPTY_GROUP,
  withAlternative,
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

/** Where the editor is open: on a filter of a group, a new filter of one, or a new alternative. */
type Editing = { group: number; at: Exclude<GroupEditing, null> } | 'alternative' | null;

/**
 * A field query as rows of filters, one row for each alternative.
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
  // An empty statement is one empty row, to start writing in.
  const groups = parsed.groups.length > 0 ? parsed.groups : [EMPTY_GROUP];
  const parsedTexts = groups.map((group) => group.text);
  const parsedKey = parsedTexts.join('\u0000');
  const [editing, setEditing] = React.useState<Editing>(null);
  const [startField, setStartField] = React.useState<string | undefined>();
  // A text box is typed into freely and joins the statement when it is
  // committed: writing it back on every key would turn `status = 5` into a
  // filter halfway through the value.
  const [texts, setTexts] = React.useState<string[]>(parsedTexts);

  // Whenever the statement changes, each box shows the search text its group
  // now has: text committed as `status >= 500` has become a filter and leaves it.
  React.useEffect(() => {
    const now = parsedKey.split('\u0000');
    setTexts((current) => now.map((text, index) => (current[index]?.trim() === text ? current[index]! : text)));
  }, [parsedKey, statement]);

  // A field picked in the field list opens the editor of the last row on it.
  const lastGroup = groups.length - 1;
  React.useEffect(() => {
    if (!pendingField) return;
    setStartField(pendingField);
    setEditing({ group: lastGroup, at: 'new' });
    onPendingFieldHandled?.();
  }, [lastGroup, onPendingFieldHandled, pendingField]);

  // Enter runs the query, but only once the statement it just changed has
  // reached the page: `onRun` then closes over the new statement.
  const runPending = React.useRef(false);
  React.useEffect(() => {
    if (!runPending.current) return;
    runPending.current = false;
    onRun?.();
  });

  // The statement with what the boxes hold: an edit writes the typed text along.
  const current: BuilderStatement = {
    groups: groups.map((group, index) => ({ ...group, text: texts[index] ?? group.text })),
  };

  const write = (next: BuilderStatement, run = false) => {
    const written = dialect.serialize(next);
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

  const editor = (initial: BuilderClause | undefined, submit: (clause: BuilderClause) => void) => (
    <FilterEditor
      fields={fields}
      dialect={dialect}
      initial={initial}
      initialField={initial ? undefined : startField}
      suggestValues={suggestValues}
      onSubmit={(clause) => {
        submit(clause);
        closeEditor();
      }}
      onCancel={closeEditor}
    />
  );

  const alternative = dialect.alternatives ? (
    <Popover
      open={editing === 'alternative'}
      onOpenChange={(open) => (open ? setEditing('alternative') : closeEditor())}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          data-testid="query-builder-add-alternative"
          aria-label={t('query_builder.add_alternative_aria')}
          className="inline-flex h-[28px] shrink-0 items-center gap-1 rounded-md bg-[var(--functional-surface)] pl-2 pr-2.5 font-sans text-xs font-strong text-tx-1 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 data-[state=open]:bg-bg-2"
        >
          <Plus className="h-3.5 w-3.5" aria-hidden="true" />
          {t('query_builder.add_alternative')}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-auto p-0">
        {editor(undefined, (clause) => write(withAlternative(current, clause)))}
      </PopoverContent>
    </Popover>
  ) : null;

  return (
    <div data-query-builder className={cn('px-3 py-2.5', className)}>
      {notice ? (
        <p className="mb-2 font-sans text-xs leading-relaxed text-tx-2">{notice}</p>
      ) : null}
      <div
        {...(groups.length > 1 ? { role: 'group', 'aria-label': t('query_builder.filters') } : {})}
        className="flex flex-col gap-[6px]"
      >
        {groups.map((group, index) => (
          <FilterGroup
            key={index}
            group={group}
            position={index}
            count={groups.length}
            text={texts[index] ?? group.text}
            onTextChange={(text) =>
              setTexts((now) =>
                groups.map((other, position) => (position === index ? text : (now[position] ?? other.text))),
              )
            }
            onTextCommit={(run) => write(current, run)}
            editing={editing !== null && editing !== 'alternative' && editing.group === index ? editing.at : null}
            onEditingChange={(at) => (at === null ? closeEditor() : setEditing({ group: index, at }))}
            editor={(initial, at) =>
              editor(initial, (clause) => write(withClause(current, index, at, clause)))
            }
            onRemoveClause={(at) => write(withoutClause(current, index, at))}
            onRemoveUnsupported={(at) => write(withoutUnsupported(current, index, at))}
            trailing={index === lastGroup ? alternative : null}
          />
        ))}
      </div>
    </div>
  );
}
