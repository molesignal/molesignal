import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

interface PageSizeSelectProps {
  value: number;
  options: number[];
  ariaLabel: string;
  onChange: (pageSize: number) => void;
}

/**
 * How many rows a page holds, at the start of a pagination bar, in the type of
 * the labels beside it. The control and the list it opens are as wide as their
 * numbers and no wider: there is nothing in the list but numbers, so the chosen
 * one is marked by its fill and weight, without a tick column that would stand
 * empty before all the others, and the numbers line up on their last digit.
 */
export function PageSizeSelect({ value, options, ariaLabel, onChange }: PageSizeSelectProps) {
  const { i18n } = useTranslation();
  const numbers = React.useMemo(() => new Intl.NumberFormat(i18n.language), [i18n.language]);

  return (
    <Select value={String(value)} onValueChange={(next) => onChange(Number(next))}>
      <SelectTrigger
        aria-label={ariaLabel}
        data-page-size
        className="h-11 w-auto gap-1 rounded-md border-0 bg-transparent px-2 font-sans text-xs font-strong tabular-nums text-tx-1 shadow-none hover:border-0 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 data-[state=open]:border-0 data-[state=open]:bg-bg-3 lg:h-8"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="start" className="min-w-0">
        {options.map((option) => (
          <SelectItem
            key={option}
            value={String(option)}
            className="justify-end pl-[10px] font-sans text-xs tabular-nums [&>span:first-child]:hidden"
          >
            {numbers.format(option)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
