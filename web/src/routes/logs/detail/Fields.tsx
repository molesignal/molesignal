import { Clipboard, Eye, EyeOff, Minus, Plus, Search, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import type { SignalReferenceTime } from '@/shell/SignalReference';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';

import { LogFieldValue, type LogFieldJumpHandler } from './FieldValue';
import { fieldValueText, isFilterableValue, matchFieldRows, type LogFieldRow } from './model';

/** With fewer fields than this the whole record is in view, and there is nothing to search. */
const SEARCH_FROM = 9;

interface LogDetailFieldsProps {
  rows: readonly LogFieldRow[];
  /** The whole record, which a value is read against. */
  record: Record<string, unknown>;
  time?: SignalReferenceTime | undefined;
  /** The fields shown as columns of the list. */
  visibleFields: readonly string[];
  onInclude: (field: string, value: unknown) => void;
  onExclude: (field: string, value: unknown) => void;
  onToggleVisibility: (field: string) => void;
  onJump: LogFieldJumpHandler;
}

/**
 * The record field by field: its name, its value, and what can be asked of
 * it. The actions come with the pointer or the keyboard, at the end of the
 * row, so a row is as wide as its value needs and none of them is cut off.
 */
export function LogDetailFields({
  rows,
  record,
  time,
  visibleFields,
  onInclude,
  onExclude,
  onToggleVisibility,
  onJump,
}: LogDetailFieldsProps) {
  const { t } = useTranslation('logs');
  const [needle, setNeedle] = React.useState('');
  const shown = React.useMemo(() => matchFieldRows(rows, needle), [needle, rows]);

  return (
    <TooltipProvider delayDuration={250}>
      {rows.length >= SEARCH_FROM && (
        <div className="mb-2 flex h-8 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs">
          <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
          <input
            value={needle}
            onChange={(event) => setNeedle(event.target.value)}
            placeholder={t('explore.detail.filter_placeholder')}
            aria-label={t('explore.detail.filter_placeholder')}
            className="min-w-0 flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:bg-transparent"
          />
          {needle && (
            <span className="shrink-0 font-mono tabular-nums text-tx-3">
              {shown.length} / {rows.length}
            </span>
          )}
        </div>
      )}
      {shown.length === 0 ? (
        <p className="py-6 text-center font-sans text-xs text-tx-2">{t('explore.detail.no_field_match')}</p>
      ) : (
        <dl data-log-detail-fields className="font-sans text-xs">
          {shown.map(({ path, value }) => {
            const column = visibleFields.includes(path);

            return (
              <div
                key={path}
                data-log-detail-field={path}
                className="group relative grid grid-cols-[minmax(0,38%)_minmax(0,1fr)] gap-3 border-b border-bd-0 py-[7px] hover:bg-bg-2 has-[:focus-visible]:bg-bg-2"
              >
                <dt className="min-w-0 break-all pl-[6px] font-mono text-tx-2">{path}</dt>
                <dd className="min-w-0 pr-[6px] text-tx-0">
                  <LogFieldValue field={path} value={value} record={record} time={time} onJump={onJump} />
                  <span className="absolute right-[2px] top-[2px] flex rounded-md bg-bg-2 opacity-0 group-hover:opacity-100 group-has-[:focus-visible]:opacity-100">
                    <FieldAction
                      label={t('explore.detail.include_filter')}
                      icon={Plus}
                      disabled={!isFilterableValue(value)}
                      onClick={() => onInclude(path, value)}
                    />
                    <FieldAction
                      label={t('explore.detail.exclude_filter')}
                      icon={Minus}
                      disabled={!isFilterableValue(value)}
                      onClick={() => onExclude(path, value)}
                    />
                    <FieldAction
                      label={t('explore.detail.copy_value')}
                      icon={Clipboard}
                      onClick={() => {
                        void writeClipboardText(fieldValueText(value)).then(() => {
                          toast.success(t('explore.detail.copied_value'));
                        });
                      }}
                    />
                    <FieldAction
                      label={column ? t('explore.detail.hide_column') : t('explore.detail.show_column')}
                      icon={column ? EyeOff : Eye}
                      pressed={column}
                      onClick={() => onToggleVisibility(path)}
                    />
                  </span>
                </dd>
              </div>
            );
          })}
        </dl>
      )}
    </TooltipProvider>
  );
}

function FieldAction({
  label,
  icon: Icon,
  onClick,
  disabled = false,
  pressed,
}: {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
  disabled?: boolean;
  pressed?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          disabled={disabled}
          aria-label={label}
          {...(pressed === undefined ? {} : { 'aria-pressed': pressed })}
          className="grid h-7 w-7 place-items-center rounded-md text-tx-2 hover:bg-bg-4 hover:text-tx-0 focus-visible:bg-bg-4 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent"
        >
          <Icon className="h-3 w-3" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
