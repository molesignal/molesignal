import { Check, ChevronDown, Clock3 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DateTimePicker } from '@/shell/DateTimePicker';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Dialog, DialogContent, DialogTitle } from '@/shell/ui/dialog';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { resolveWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

interface TimePickerProps {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}

const PRESETS: Array<{ id: string; labelKey: string; window: TimeWindow }> = [
  { id: '5m', labelKey: 'time_picker.presets.last_5m', window: { from: 'now-5m', to: 'now', mode: 'relative' } },
  { id: '15m', labelKey: 'time_picker.presets.last_15m', window: { from: 'now-15m', to: 'now', mode: 'relative' } },
  { id: '30m', labelKey: 'time_picker.presets.last_30m', window: { from: 'now-30m', to: 'now', mode: 'relative' } },
  { id: '1h', labelKey: 'time_picker.presets.last_1h', window: { from: 'now-1h', to: 'now', mode: 'relative' } },
  { id: '6h', labelKey: 'time_picker.presets.last_6h', window: { from: 'now-6h', to: 'now', mode: 'relative' } },
  { id: '12h', labelKey: 'time_picker.presets.last_12h', window: { from: 'now-12h', to: 'now', mode: 'relative' } },
  { id: '24h', labelKey: 'time_picker.presets.last_24h', window: { from: 'now-24h', to: 'now', mode: 'relative' } },
  { id: '2d', labelKey: 'time_picker.presets.last_2d', window: { from: 'now-2d', to: 'now', mode: 'relative' } },
  { id: '7d', labelKey: 'time_picker.presets.last_7d', window: { from: 'now-7d', to: 'now', mode: 'relative' } },
  { id: '30d', labelKey: 'time_picker.presets.last_30d', window: { from: 'now-30d', to: 'now', mode: 'relative' } },
];

interface TimeRangeControlProps {
  value?: string;
  className?: string;
  align?: 'start' | 'center' | 'end';
  onClick?: () => void;
}

export function TimeRangeControl({ value, className, align = 'end', onClick }: TimeRangeControlProps) {
  const { t } = useTranslation('common');
  const window = useTimeStore((s) => s.window);
  const [open, setOpen] = React.useState(false);
  const displayValue = value ?? formatTimeWindowLabel(window, t);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          data-time-range-trigger="true"
          aria-label={t('time_picker.button_aria', { value: displayValue })}
          className={cn(
            'group inline-flex h-8 max-w-[220px] shrink-0 items-center gap-2 rounded-md border-0 bg-[var(--control-surface)] px-[10px] font-sans text-[13px] font-strong text-tx-1 transition-colors duration-fast hover:bg-[var(--floating-item-hover)] hover:text-tx-0 focus-visible:bg-[var(--floating-item-hover)] focus-visible:text-tx-0 data-[state=open]:bg-[var(--floating-item-hover)] data-[state=open]:text-tx-0',
            className,
          )}
        >
          <Clock3 className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <span className="min-w-0 max-w-[150px] truncate">{displayValue}</span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast group-data-[state=open]:rotate-180" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align={align}
        sideOffset={8}
        collisionPadding={12}
        aria-label={t('time_picker.title')}
        data-time-picker-surface="workbench"
        className="w-[600px] max-w-[calc(100vw-24px)] overflow-hidden rounded-md p-0"
      >
        <TimePickerHeader />
        <TimeRangePanel onDone={() => setOpen(false)} />
      </PopoverContent>
    </Popover>
  );
}

export function TimePicker({ open, onOpenChange }: TimePickerProps) {
  const { t } = useTranslation('common');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        data-time-picker-surface="workbench"
        className="w-[600px] max-w-[calc(100vw-24px)] gap-0 overflow-hidden rounded-md border-0 bg-[var(--floating-surface)] p-0 shadow-popup"
      >
        <TimePickerHeader
          title={(
            <DialogTitle className="font-sans text-sm font-strong leading-5 tracking-normal text-tx-0">
              {t('time_picker.title')}
            </DialogTitle>
          )}
        />
        <TimeRangePanel onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

function TimePickerHeader({ title }: { title?: React.ReactNode }) {
  const { t } = useTranslation('common');

  return (
    <div className="px-3 pb-2 pt-3">
      {title ?? (
        <div className="font-sans text-sm font-strong leading-5 text-tx-0">
          {t('time_picker.title')}
        </div>
      )}
    </div>
  );
}

function TimeRangePanel({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation('common');
  const current = useTimeStore((s) => s.window);
  const setWindow = useTimeStore((s) => s.setWindow);
  const [from, setFrom] = React.useState(() => toLocalInputValue(resolveWindow(current).from));
  const [to, setTo] = React.useState(() => toLocalInputValue(resolveWindow(current).to));
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    const resolved = resolveWindow(current);
    setFrom(toLocalInputValue(resolved.from));
    setTo(toLocalInputValue(resolved.to));
    setError(null);
  }, [current]);

  const submitAbsolute = () => {
    const fromDate = from ? new Date(from) : null;
    const toDate = to ? new Date(to) : null;
    if (!fromDate || !toDate || Number.isNaN(fromDate.getTime()) || Number.isNaN(toDate.getTime()) || fromDate >= toDate) {
      setError(t('time_picker.invalid'));
      return;
    }
    setWindow({ from: fromDate.toISOString(), to: toDate.toISOString(), mode: 'absolute' });
    onDone();
  };

  return (
    <div className="grid grid-cols-1 gap-3 px-3 pb-3 pt-1 sm:grid-cols-[300px_minmax(0,1fr)]">
      <section className="min-w-0">
        <div className="px-2 pb-2 font-sans text-xs font-strong text-tx-3">
          {t('time_picker.quick_ranges')}
        </div>
        <div className="grid grid-flow-col grid-rows-5 gap-0.5">
          {PRESETS.map((preset) => {
            const selected = sameWindow(current, preset.window);
            return (
              <button
                key={preset.id}
                type="button"
                aria-pressed={selected}
                onClick={() => {
                  setWindow(preset.window);
                  onDone();
                }}
                className={cn(
                  'flex h-8 items-center gap-2 rounded-sm px-[10px] text-left font-sans text-xs font-body transition-colors duration-fast',
                  selected
                    ? 'bg-[var(--floating-item-selected)] font-strong text-indigo-soft'
                    : 'text-tx-1 hover:bg-[var(--floating-item-hover)] hover:text-tx-0 focus-visible:bg-[var(--floating-item-hover)] focus-visible:text-tx-0',
                )}
              >
                <span className="min-w-0 flex-1 truncate">{t(preset.labelKey)}</span>
                {selected && <Check className="h-3.5 w-3.5 shrink-0" />}
              </button>
            );
          })}
        </div>
      </section>

      <section className="flex min-w-0 flex-col px-1">
        <div>
          <div className="font-sans text-xs font-strong text-tx-1">{t('time_picker.absolute')}</div>
          <div className="mt-0.5 font-sans text-xs leading-4 text-tx-3">{t('time_picker.local_time')}</div>
        </div>
        <label className="mt-2.5 flex flex-col gap-1 font-sans text-xs font-strong text-tx-2">
          {t('time_picker.from')}
          <DateTimePicker
            value={from}
            onChange={setFrom}
            includeSeconds
            aria-invalid={!!error}
            className="h-8 border-0 bg-[var(--control-surface)] text-xs font-body text-tx-0 shadow-none enabled:hover:border-0 enabled:hover:bg-[var(--floating-item-hover)]"
          />
        </label>
        <label className="mt-2.5 flex flex-col gap-1 font-sans text-xs font-strong text-tx-2">
          {t('time_picker.to')}
          <DateTimePicker
            value={to}
            onChange={setTo}
            includeSeconds
            aria-invalid={!!error}
            className="h-8 border-0 bg-[var(--control-surface)] text-xs font-body text-tx-0 shadow-none enabled:hover:border-0 enabled:hover:bg-[var(--floating-item-hover)]"
          />
        </label>
        {error && <div role="alert" className="mt-2 font-sans text-xs font-strong text-red-soft">{error}</div>}
        <div className="mt-auto flex justify-end pt-3">
          <Button
            size="sm"
            onClick={submitAbsolute}
            disabled={!from || !to}
            className="bg-indigo font-bold text-white hover:brightness-90"
          >
            {t('time_picker.apply')}
          </Button>
        </div>
      </section>
    </div>
  );
}

function sameWindow(a: TimeWindow, b: TimeWindow): boolean {
  return a.mode === b.mode && a.from === b.from && a.to === b.to;
}

export function formatTimeWindowLabel(window: TimeWindow, t: (key: string) => string): string {
  const preset = PRESETS.find((item) => sameWindow(window, item.window));
  if (preset) return t(preset.labelKey);
  if (window.mode === 'relative') return `${window.from} - ${window.to}`;
  try {
    const from = new Date(window.from);
    const to = new Date(window.to);
    if (from.toDateString() === to.toDateString()) {
      return `${from.toISOString().slice(11, 16)} - ${to.toISOString().slice(11, 16)} UTC`;
    }
    return `${from.toISOString().slice(0, 10)} - ${to.toISOString().slice(0, 10)}`;
  } catch {
    return `${window.from} - ${window.to}`;
  }
}

function toLocalInputValue(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0');
  return [
    date.getFullYear(),
    '-',
    pad(date.getMonth() + 1),
    '-',
    pad(date.getDate()),
    'T',
    pad(date.getHours()),
    ':',
    pad(date.getMinutes()),
    ':',
    pad(date.getSeconds()),
  ].join('');
}
