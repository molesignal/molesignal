import { X } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

export function ProfileTagInput({
  values,
  onChange,
  placeholder,
  id,
  describedBy,
  removeLabel,
}: {
  values: string[];
  onChange: (values: string[]) => void;
  placeholder: string;
  id: string;
  describedBy: string;
  removeLabel: (value: string) => string;
}) {
  const inputRef = React.useRef<HTMLInputElement>(null);
  const [pending, setPending] = React.useState('');

  const commit = (source: string) => {
    const additions = splitTagInput(source);
    if (additions.length) {
      onChange(Array.from(new Set([...values, ...additions])));
    }
    setPending('');
  };

  const remove = (value: string) => {
    onChange(values.filter((candidate) => candidate !== value));
  };

  return (
    <div
      className={cn(
        'flex min-h-10 w-full flex-wrap items-center gap-1.5 rounded-md border border-bd-1 bg-bg-2 px-2 py-1.5',
        'focus-within:bg-bg-1',
      )}
      onClick={() => inputRef.current?.focus()}
    >
      {values.map((value) => (
        <span
          key={value}
          className="inline-flex min-h-7 max-w-full items-center gap-1 rounded-full bg-indigo-dim py-0.5 pl-3 pr-1.5 text-xs font-strong text-indigo"
        >
          <span className="max-w-72 truncate" title={value}>
            {value}
          </span>
          <button
            type="button"
            aria-label={removeLabel(value)}
            className="grid h-5 w-5 shrink-0 place-items-center rounded-full text-indigo/75 hover:bg-indigo/15 hover:text-indigo"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => remove(value)}
          >
            <X className="h-3 w-3" />
          </button>
        </span>
      ))}
      <input
        ref={inputRef}
        id={id}
        value={pending}
        aria-describedby={describedBy}
        onChange={(event) => setPending(event.target.value)}
        onBlur={() => commit(pending)}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === 'Enter' || event.key === ',') {
            event.preventDefault();
            commit(pending);
          } else if (event.key === 'Backspace' && !pending) {
            const lastValue = values.at(-1);
            if (lastValue) remove(lastValue);
          }
        }}
        onPaste={(event) => {
          const pasted = event.clipboardData.getData('text');
          if (!/[,\n]/.test(pasted)) return;
          event.preventDefault();
          commit(`${pending},${pasted}`);
        }}
        placeholder={values.length ? '' : placeholder}
        className="h-7 min-w-32 flex-1 bg-transparent px-1 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
      />
    </div>
  );
}

export function splitTagInput(source: string): string[] {
  return Array.from(
    new Set(
      source
        .split(/[,\n]/)
        .map((value) => value.trim())
        .filter(Boolean),
    ),
  );
}
