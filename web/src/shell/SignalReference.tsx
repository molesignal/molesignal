import {
  Activity,
  AlertCircle,
  Copy,
  ExternalLink,
  Filter,
  FilterX,
  Server,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import {
  buildSignalContext,
  buildSignalJumps,
  detectSignalTypeForLabel,
  type SignalReferenceOptions,
  type SignalReferenceStreamType,
  type SignalReferenceTime,
  type SignalReferenceType,
  type SignalReferenceSource,
  type SignalJumpAction,
  type SignalJumpKind,
} from '@/shell/signalReference/links';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';

export { buildSignalJumps, detectSignalTypeForLabel };
export type {
  SignalJumpAction,
  SignalJumpKind,
  SignalReferenceOptions,
  SignalReferenceSource,
  SignalReferenceStreamType,
  SignalReferenceTime,
  SignalReferenceType,
};

interface SignalReferenceProps extends SignalReferenceOptions {
  type: SignalReferenceType;
  value: string;
  /** Explicit scope for the source row. Falls back to the global investigation window. */
  time?: SignalReferenceTime | undefined;
  children?: React.ReactNode | undefined;
  className?: string | undefined;
  showIcon?: boolean | undefined;
}

const TYPE_META: Record<SignalReferenceType, { icon: LucideIcon; labelKey: string }> = {
  trace_id: { icon: Activity, labelKey: 'signal_reference.types.trace' },
  span_id: { icon: AlertCircle, labelKey: 'signal_reference.types.span' },
  service: { icon: Server, labelKey: 'signal_reference.types.service' },
  host: { icon: Server, labelKey: 'signal_reference.types.host' },
  stream: { icon: Server, labelKey: 'signal_reference.types.stream' },
};

/**
 * Canonical inline pivot for Logs, Metrics, Traces and Streams. Every jump
 * preserves pinned filters and an investigation time window.
 */
export function SignalReference({
  type,
  value,
  labelName,
  labels,
  metricQuery,
  streamType,
  streamId,
  source,
  time,
  children,
  className,
  showIcon = true,
}: SignalReferenceProps) {
  const { t } = useTranslation('shell');
  const meta = TYPE_META[type];
  const Icon = meta.icon;
  const [open, setOpen] = React.useState(false);
  const [copiedKey, setCopiedKey] = React.useState<string | null>(null);
  const globalFilters = useFiltersStore((state) => state.filters);
  const setFilter = useFiltersStore((state) => state.setFilter);
  const globalTimeWindow = useTimeStore((state) => state.window);
  const effectiveTime = React.useMemo(() => {
    if (time) return time;
    const resolved = resolveWindow(globalTimeWindow);
    return {
      from: resolved.from.toISOString(),
      to: resolved.to.toISOString(),
    };
  }, [globalTimeWindow, time]);

  const filterKey = (labelName?.trim() || type).toLowerCase();
  const isFiltered = globalFilters.some(
    (filter) =>
      filter.key === filterKey &&
      filter.value === value &&
      filter.operator !== '!=',
  );
  const isExcluded = globalFilters.some(
    (filter) =>
      filter.key === filterKey &&
      filter.value === value &&
      filter.operator === '!=',
  );
  const context = React.useMemo(
    () => buildSignalContext(type, value, { labelName, labels, metricQuery }),
    [labelName, labels, metricQuery, type, value],
  );
  const jumps = React.useMemo(
    () =>
      buildSignalJumps(
        type,
        value,
        effectiveTime,
        { labelName, labels, metricQuery, streamType, streamId, source },
        globalFilters,
      ),
    [
      effectiveTime,
      globalFilters,
      labelName,
      labels,
      metricQuery,
      source,
      streamId,
      streamType,
      type,
      value,
    ],
  );

  const handleCopy = React.useCallback(async (copyValue: string, key: string) => {
    try {
      await navigator.clipboard.writeText(copyValue);
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey(null), 1200);
    } catch {
      // Clipboard access can be blocked by browser policy.
    }
  }, []);

  const copyLabelKey =
    type === 'span_id'
      ? 'signal_reference.actions.copy_span_id'
      : type === 'trace_id'
        ? 'signal_reference.actions.copy_trace_id'
        : type === 'service'
          ? 'signal_reference.actions.copy_service'
          : 'signal_reference.actions.copy_value';

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-signal-type={type}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setOpen(true);
          }}
          className={cn(
            'inline-flex items-center gap-1 rounded font-sans tabular-nums',
            'text-indigo-soft underline decoration-dotted decoration-1 underline-offset-2',
            'transition-all duration-fast ease-default hover:text-indigo hover:decoration-solid active:scale-[0.97]',
            'focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none',
            className,
          )}
        >
          {showIcon && <Icon aria-hidden className="h-3 w-3 stroke-[1.6]" />}
          <span>{children ?? value}</span>
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={6}
        className="z-[70] w-72 border-bd-1 bg-surface p-0 shadow-popup"
      >
        <header className="border-b border-bd-0 px-3 py-2.5">
          <div className="flex min-w-0 items-center gap-1.5">
            <Icon aria-hidden className="h-3.5 w-3.5 stroke-[1.6] text-tx-2" />
            <span className="font-sans text-xs font-strong uppercase tracking-wider text-tx-2">
              {t(meta.labelKey)}
            </span>
          </div>
          <div className="mt-1.5 break-all font-sans text-xs font-semibold tabular-nums text-tx-0">
            {type === 'span_id' && context.operation ? context.operation : value}
          </div>
          {type === 'span_id' && context.operation && (
            <div className="mt-0.5 break-all font-mono text-xs tabular-nums text-tx-2">
              {value}
            </div>
          )}
        </header>
        <div className="py-1">
          <MenuButton
            icon={Filter}
            disabled={isFiltered}
            onClick={() => setFilter(filterKey, value, '=')}
            testId="signal-pin-filter"
          >
            {t(
              isFiltered
                ? 'signal_reference.actions.filtered'
                : 'signal_reference.actions.add_filter',
            )}
          </MenuButton>
          <MenuButton
            icon={FilterX}
            disabled={isExcluded}
            onClick={() => setFilter(filterKey, value, '!=')}
            testId="signal-exclude-filter"
          >
            {t(
              isExcluded
                ? 'signal_reference.actions.excluded'
                : 'signal_reference.actions.exclude_value',
            )}
          </MenuButton>
          <MenuButton
            icon={Copy}
            onClick={() => void handleCopy(value, 'value')}
            testId="signal-copy"
          >
            {t(
              copiedKey === 'value'
                ? 'signal_reference.actions.copied'
                : copyLabelKey,
            )}
          </MenuButton>
          {type === 'span_id' && context.traceId && (
            <MenuButton
              icon={Copy}
              onClick={() => void handleCopy(context.traceId!, 'trace')}
            >
              {t(
                copiedKey === 'trace'
                  ? 'signal_reference.actions.copied'
                  : 'signal_reference.actions.copy_trace_id',
              )}
            </MenuButton>
          )}
        </div>
        <ul className="border-t border-bd-0 py-1">
          {jumps.map((jump) => (
            <li key={`${jump.id}:${jump.to}`}>
              <Link
                to={jump.to}
                onClick={() => setOpen(false)}
                className="flex items-center gap-2 px-3 py-1.5 font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
              >
                <ExternalLink className="h-3 w-3 shrink-0 text-tx-2" />
                <span className="flex-1 truncate">{t(jump.labelKey)}</span>
                <span
                  className={cn(
                    'shrink-0 rounded px-1.5 py-0.5 text-type-micro leading-none',
                    jump.relation === 'exact'
                      ? 'bg-green-dim text-green-soft'
                      : 'bg-bg-3 text-tx-2',
                  )}
                >
                  {t(`signal_reference.relations.${jump.relation}`)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}

function MenuButton({
  icon: Icon,
  disabled,
  onClick,
  testId,
  children,
}: {
  icon: LucideIcon;
  disabled?: boolean;
  onClick: () => void;
  testId?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="flex h-8 w-full items-center gap-2 px-3 text-left font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none disabled:cursor-default disabled:text-tx-3 disabled:hover:bg-transparent"
      {...(testId ? { 'data-testid': testId } : {})}
    >
      <Icon className="h-3.5 w-3.5 text-tx-2" />
      <span>{children}</span>
    </button>
  );
}
