import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';

import type * as webApi from '@/api/web';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import {
  parseTraceListSort,
  TRACE_LIST_SORT_OPTIONS,
} from '../sort';

export function TraceListSortSelect({
  value,
  onChange,
}: {
  value: webApi.TraceListSort;
  onChange: (value: webApi.TraceListSort) => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <Select
      value={value}
      onValueChange={(nextValue) => onChange(parseTraceListSort(nextValue))}
    >
      <SelectTrigger
        aria-label={t('explore.sort.aria')}
        className="h-[44px] w-fit min-w-0 rounded-sm border-0 bg-transparent px-[10px] py-0 text-xs font-semibold text-tx-2 shadow-none hover:bg-[var(--control-surface)] hover:text-tx-0 data-[state=open]:border-0 data-[state=open]:bg-[var(--control-surface)] data-[state=open]:text-tx-0 sm:h-[28px]"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="end" className="min-w-[136px]">
        {TRACE_LIST_SORT_OPTIONS.map((option) => (
          <SelectItem key={option.value} value={option.value} className="h-8 text-xs">
            {t(option.labelKey)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export function formatTraceStart(startNs: number, tz: string): string {
  if (!Number.isFinite(startNs) || startNs <= 0) return '-';
  const date = dayjs(Math.floor(startNs / 1_000_000)).tz(tz);
  return date.isValid() ? date.format('HH:mm:ss.SSS') : '-';
}
