import { ChevronLeft } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/shell/ui/command';
import { Input } from '@/shell/ui/input';

import {
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderOperator,
  makeClause,
} from './model';

const KIND_GLYPH = { text: 'T', number: '#', boolean: 'B' } as const;

/**
 * Writes one filter: pick a field, then say how it compares and to what.
 * Opening it on a filter that exists starts at the second step.
 */
export function FilterEditor({
  fields,
  dialect,
  initial,
  initialField,
  suggestValues,
  onSubmit,
  onCancel,
}: {
  fields: readonly BuilderField[];
  dialect: BuilderDialect;
  /** The filter being changed; none when adding. */
  initial?: BuilderClause | undefined;
  /** A field to start a new filter on. */
  initialField?: string | undefined;
  suggestValues?: ((field: string) => readonly string[]) | undefined;
  onSubmit: (clause: BuilderClause) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation('common');
  const [fieldName, setFieldName] = React.useState<string | null>(
    initial?.field ?? initialField ?? null,
  );
  const [search, setSearch] = React.useState('');
  const [op, setOp] = React.useState<BuilderOperator | null>(initial?.op ?? null);
  const [value, setValue] = React.useState(initial?.value ?? '');
  const valueRef = React.useRef<HTMLInputElement>(null);

  const field = fields.find((candidate) => candidate.name === fieldName);
  const operators = dialect.operators(field);
  const activeOp = op && operators.includes(op) ? op : operators[0]!;

  React.useEffect(() => {
    if (fieldName !== null) valueRef.current?.focus();
  }, [fieldName]);

  if (fieldName === null) {
    const typed = search.trim();
    const listed = fields.some((candidate) => candidate.name === typed);
    return (
      <Command className="w-[300px]">
        <CommandInput
          autoFocus
          value={search}
          onValueChange={setSearch}
          placeholder={t('query_builder.search_fields')}
        />
        <CommandList className="max-h-[280px]">
          <CommandEmpty>{t('query_builder.no_fields')}</CommandEmpty>
          <CommandGroup>
            {fields.map((candidate) => (
              <CommandItem
                key={candidate.name}
                value={candidate.name}
                onSelect={() => setFieldName(candidate.name)}
                className="gap-2"
              >
                <span
                  aria-hidden="true"
                  className="type-micro w-4 shrink-0 text-center font-mono font-semibold text-tx-3"
                >
                  {KIND_GLYPH[candidate.kind]}
                </span>
                <span className="truncate font-mono text-xs text-tx-0">{candidate.name}</span>
              </CommandItem>
            ))}
            {/* A field the schema does not list can still be filtered on by name. */}
            {typed && !listed && /^[a-zA-Z_][\w.]*$/.test(typed) ? (
              <CommandItem
                value={`${typed} `}
                forceMount
                onSelect={() => setFieldName(typed)}
                className="text-xs text-tx-1"
              >
                {t('query_builder.use_field', { field: typed })}
              </CommandItem>
            ) : null}
          </CommandGroup>
        </CommandList>
      </Command>
    );
  }

  const suggestions = (suggestValues?.(fieldName) ?? []).filter((item) => item !== value).slice(0, 8);
  const submit = (next = value) => {
    if (!next.trim()) return;
    onSubmit(makeClause(field ?? { name: fieldName }, activeOp, next));
  };

  return (
    <form
      className="w-[300px] p-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <button
        type="button"
        aria-label={t('query_builder.change_field')}
        title={t('query_builder.change_field')}
        onClick={() => {
          setSearch('');
          setFieldName(null);
        }}
        className="-ml-1 flex h-7 max-w-full items-center gap-1 rounded px-1 font-mono text-xs font-semibold text-tx-0 hover:bg-bg-2 focus-visible:bg-bg-2"
      >
        <ChevronLeft className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
        <span className="truncate">{fieldName}</span>
      </button>

      <div
        role="group"
        aria-label={t('query_builder.operator')}
        className="mt-2 flex flex-wrap gap-1"
      >
        {operators.map((item) => (
          <button
            key={item}
            type="button"
            aria-pressed={item === activeOp}
            onClick={() => {
              setOp(item);
              valueRef.current?.focus();
            }}
            className={cn(
              'h-7 rounded px-2 font-mono text-xs transition-colors',
              item === activeOp
                ? 'bg-indigo-dim font-semibold text-indigo-soft'
                : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
            )}
          >
            {item}
          </button>
        ))}
      </div>

      <Input
        ref={valueRef}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        aria-label={t('query_builder.value')}
        placeholder={t('query_builder.value_placeholder')}
        className="mt-2 h-8 font-mono text-xs"
      />

      {suggestions.length > 0 && (
        <div className="mt-2">
          <div className="type-micro mb-1 font-sans text-tx-3">
            {t('query_builder.suggestions')}
          </div>
          <div className="flex flex-wrap gap-1">
            {suggestions.map((item) => (
              <button
                key={item}
                type="button"
                title={item}
                onClick={() => submit(item)}
                className="h-6 max-w-full truncate rounded bg-[var(--control-surface)] px-1.5 font-mono text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
              >
                {item}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="mt-3 flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" onClick={onCancel}>
          {t('query_builder.cancel')}
        </Button>
        <Button type="submit" size="sm" disabled={!value.trim()}>
          {initial ? t('query_builder.submit_update') : t('query_builder.submit_add')}
        </Button>
      </div>
    </form>
  );
}
