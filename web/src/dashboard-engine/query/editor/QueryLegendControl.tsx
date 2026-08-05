import * as React from 'react';

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from '@/shell/ui/select';

import { useDashboardText } from '../../i18n';
import {
  DEFAULT_QUERY_LEGEND_TEMPLATE,
  QUERY_LEGEND_AUTO,
  type QueryLegendMode,
  queryLegendValueForMode,
  resolveQueryLegendMode,
} from '../legend';

const LEGEND_MODES: ReadonlyArray<{
  mode: QueryLegendMode;
  label: string;
  description: string;
}> = [
  {
    mode: 'auto',
    label: 'Auto',
    description: 'Only includes unique labels',
  },
  {
    mode: 'verbose',
    label: 'Verbose',
    description: 'All label names and values',
  },
  {
    mode: 'custom',
    label: 'Custom',
    description: 'Provide a naming template',
  },
];

export function QueryLegendControl({
  value,
  onChange,
}: {
  value: string | undefined;
  onChange: (value: string | undefined) => void;
}) {
  const tr = useDashboardText();
  const mode = resolveQueryLegendMode(value);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const focusCustom = React.useRef(false);

  React.useLayoutEffect(() => {
    if (mode !== 'custom' || !focusCustom.current) return;
    focusCustom.current = false;
    inputRef.current?.focus();
    inputRef.current?.setSelectionRange(2, 12, 'forward');
  }, [mode]);

  if (mode === 'custom') {
    return (
      <input
        ref={inputRef}
        aria-label={tr('Custom legend')}
        value={value ?? ''}
        placeholder={DEFAULT_QUERY_LEGEND_TEMPLATE}
        onChange={(event) =>
          onChange(event.target.value || QUERY_LEGEND_AUTO)
        }
        className="h-11 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 font-mono text-base text-tx-1 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2 sm:h-8 sm:text-xs"
      />
    );
  }

  return (
    <Select
      value={mode}
      onValueChange={(nextMode: QueryLegendMode) => {
        if (nextMode === 'custom') focusCustom.current = true;
        onChange(queryLegendValueForMode(nextMode));
      }}
    >
      <SelectTrigger
        aria-label={tr('Legend mode')}
        className="h-11 bg-bg-1 px-2 text-base sm:h-8 sm:text-xs"
      >
        <span>{tr(LEGEND_MODES.find((item) => item.mode === mode)!.label)}</span>
      </SelectTrigger>
      <SelectContent>
        {LEGEND_MODES.map((item) => (
          <SelectItem
            key={item.mode}
            value={item.mode}
            className="h-auto min-h-11 py-2"
          >
            <span className="grid gap-0.5">
              <span className="font-semibold text-tx-1">
                {tr(item.label)}
              </span>
              <span className="text-type-micro text-tx-3">
                {tr(item.description)}
              </span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
