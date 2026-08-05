import { AlertTriangle, Search } from 'lucide-react';
import * as React from 'react';

import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';
import {
  Table as ShadTable,
  TableCell as ShadTableCell,
  TableHead as ShadTableHead,
  TableRow as ShadTableRow,
} from '@/shell/ui/table';
import { TimeRangeControl } from '@/time/TimePicker';

export const uiLabelClass = 'type-label font-sans font-semibold tracking-normal text-tx-2';
export const uiLabelStrongClass = 'type-label font-sans font-semibold tracking-normal text-tx-1';
export const uiTableHeaderClass = 'type-caption font-sans font-semibold tracking-normal text-tx-2';
export const cardTextActionClass =
  'inline-flex h-8 shrink-0 items-center gap-1 px-1 font-sans text-xs font-strong text-tx-2 transition-colors duration-fast ease-default hover:text-tx-0 focus-visible:outline-none focus-visible:text-tx-0 focus-visible:underline focus-visible:underline-offset-4';

/* ───────────────────────── Pill ───────────────────────── */

export type PillTone =
  | 'neutral'
  | 'indigo'
  | 'orange'
  | 'blue'
  | 'green'
  | 'red'
  | 'yellow'
  | 'purple'
  | 'dim';

const PILL_TONE: Record<PillTone, string> = {
  neutral: 'bg-bg-3 text-tx-1 border-bd-0',
  // Phase 4: indigo pill for brand-affiliated tags (e.g. "Default rule",
  // active filter). Mirrors the brand pattern used by primary buttons.
  indigo: 'bg-indigo-dim text-indigo-soft border-indigo/30',
  orange: 'bg-orange-dim text-orange-soft border-orange/30',
  blue: 'bg-blue-dim text-blue-soft border-blue/30',
  green: 'bg-green-dim text-green-soft border-green/30',
  red: 'bg-red-dim text-red-soft border-red/30',
  yellow: 'bg-yellow-dim text-yellow-soft border-yellow/30',
  purple: 'bg-purple-dim text-purple-soft border-purple/30',
  dim: 'bg-bg-2 text-tx-3 border-bd-0',
};

export function Pill({
  tone = 'neutral',
  children,
  className,
}: {
  tone?: PillTone;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'inline-flex h-[22px] items-center gap-1 whitespace-nowrap rounded-full border px-2 font-sans text-xs font-semibold leading-none tracking-normal',
        PILL_TONE[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

/* ───────────────────────── Dot ───────────────────────── */

// Phase 4: status dots are flat — the legacy glow shadows used
// hardcoded hex matching the old Terminal palette, which broke once the
// palette migrated. A 1px ring of the same color matched against the
// surface reads sharp without the Confident-quiet-violating bloom.
const DOT_TONE = {
  green: 'bg-green',
  orange: 'bg-orange',
  blue: 'bg-blue',
  red: 'bg-red',
  yellow: 'bg-yellow',
  indigo: 'bg-indigo',
  dim: 'bg-tx-3',
} as const;

export function Dot({ tone = 'green', className }: { tone?: keyof typeof DOT_TONE; className?: string }) {
  return <span className={cn('inline-block h-1.5 w-1.5 shrink-0 rounded-full', DOT_TONE[tone], className)} />;
}

/* ───────────────────────── CriticalAlertBanner ───────────────────────── */

export interface CriticalAlertItem {
  id: string;
  label: string;
  meta?: string;
}

/**
 * Red, non-collapsible banner that leads the Home command center. Turns
 * "N firing" from an abstract number into the actual incidents an operator
 * has to triage — name + service + how long it's been burning. Renders
 * nothing when there's nothing on fire.
 */
export function CriticalAlertBanner({
  title,
  items,
  viewAllLabel,
  onViewAll,
}: {
  title: string;
  items: readonly CriticalAlertItem[];
  viewAllLabel?: string;
  onViewAll?: () => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="overflow-hidden rounded-lg border border-red/40 bg-red-dim">
      <div className="flex items-center gap-2.5 px-5 py-3">
        <AlertTriangle className="h-4.5 w-4.5 shrink-0 text-red-soft" />
        <span className="font-sans text-xs font-bold uppercase tracking-wide text-red-soft">{title}</span>
        {onViewAll && (
          <button
            type="button"
            onClick={onViewAll}
            className="ml-auto inline-flex items-center gap-1 rounded font-sans text-xs font-strong text-red-soft hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red"
          >
            {viewAllLabel} →
          </button>
        )}
      </div>
      <div className="border-t border-red/20">
        {items.map((item) => (
          <div
            key={item.id}
            className="flex min-h-9 items-center gap-2.5 border-b border-red/10 px-5 py-2 last:border-b-0"
          >
            <Dot tone="red" />
            <span className="min-w-0 flex-1 truncate font-sans text-xs font-strong text-tx-0">{item.label}</span>
            {item.meta && <span className="shrink-0 font-mono text-xs text-tx-2">{item.meta}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}

/* ───────────────────────── Button ───────────────────────── */

type ButtonVariant = 'default' | 'primary' | 'ghost';
type ButtonSize = 'sm' | 'md';

export const ChromeButton = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement> & {
    variant?: ButtonVariant;
    size?: ButtonSize;
    disabledReason?: React.ReactNode;
  }
>(function ChromeButton(
  {
    variant = 'default',
    size = 'md',
    className,
    disabled,
    disabledReason,
    ...rest
  },
  ref,
) {
  const button = (
    <button
      ref={ref}
      type="button"
      {...rest}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className={cn(
        'inline-flex shrink-0 items-center gap-2 whitespace-nowrap rounded-md font-sans font-strong transition-colors duration-fast ease-default disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-2 disabled:text-tx-3 disabled:opacity-100 disabled:shadow-none',
        size === 'md' ? 'h-9 px-3 text-sm' : 'h-8 px-2.5 text-xs',
        variant === 'default' &&
          'border border-bd-1 bg-bg-2 text-tx-1 enabled:hover:border-bd-2 enabled:hover:bg-bg-3 enabled:hover:text-tx-0',
        variant === 'primary' &&
          // Phase 4: primary button is the brand surface — Indigo. Text
          // is white (--primary-fg). No border, no glow — the elevation
          // comes purely from the saturated fill against bg-1.
          'bg-indigo font-bold text-white enabled:hover:bg-indigo-soft',
        variant === 'ghost' &&
          'border border-transparent bg-transparent text-tx-1 enabled:hover:bg-bg-3',
        className,
      )}
    />
  );
  return (
    <DisabledControl disabled={Boolean(disabled)} reason={disabledReason}>
      {button}
    </DisabledControl>
  );
});

/* ───────────────────────── Icon Button ───────────────────────── */

export const IconButton = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement> & {
    active?: boolean;
    disabledReason?: React.ReactNode;
  }
>(function IconButton(
  { children, active, className, disabled, disabledReason, ...rest },
  ref,
) {
  const button = (
    <button
      ref={ref}
      type="button"
      {...rest}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className={cn(
        'flex h-8 w-8 items-center justify-center rounded-md text-tx-2 transition-colors duration-fast enabled:hover:bg-bg-3 enabled:hover:text-tx-0 disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-transparent disabled:text-tx-3 disabled:opacity-100',
        active && 'bg-bg-3 text-tx-0',
        className,
      )}
    >
      {children}
    </button>
  );
  return (
    <DisabledControl disabled={Boolean(disabled)} reason={disabledReason}>
      {button}
    </DisabledControl>
  );
});

/* ───────────────────────── Card ───────────────────────── */

export function Card({
  children,
  className,
  bodyClassName,
}: {
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <div className={cn('rounded-lg border border-bd-0 bg-bg-1', className)}>
      <div className={cn(bodyClassName)}>{children}</div>
    </div>
  );
}

export function CardHeader({
  title,
  actions,
  className,
}: {
  title: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex min-h-11 items-center gap-3 border-b border-bd-0 px-4 py-3',
        uiLabelStrongClass,
        className,
      )}
    >
      <div className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">{title}</div>
      {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
    </div>
  );
}

export function CardBody({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('p-4', className)}>{children}</div>;
}

/* ───────────────────────── StatCard ───────────────────────── */

export function StatCard({
  label,
  value,
  unit,
  delta,
  deltaDir,
  spark,
  valueColor,
  className,
}: {
  label: string;
  value: React.ReactNode;
  unit?: string;
  delta?: string;
  deltaDir?: 'up' | 'down';
  spark?: React.ReactNode;
  valueColor?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'relative flex min-h-[112px] flex-col gap-2 overflow-hidden rounded-lg border border-bd-0 bg-bg-1 px-4 py-4',
        className,
      )}
    >
      <div className={cn('whitespace-nowrap', uiLabelClass)}>
        {label}
      </div>
      <div className="overflow-hidden text-ellipsis whitespace-nowrap font-sans [font-size:32px] font-display-strong leading-none tracking-[-0.025em]" style={valueColor ? { color: valueColor } : undefined}>
        {value}
        {unit && <span className="ml-1 font-strong text-xs text-tx-2">{unit}</span>}
      </div>
      {delta && (
        <div className="font-sans text-xs">
          {/* Delta down = degradation — use the error/red status token,
              not the brand-secondary orange. The caller can override
              direction semantics (e.g. "lower is better") at the
              callsite. */}
          <span className={deltaDir === 'up' ? 'text-green-soft' : 'text-red-soft'}>
            {deltaDir === 'up' ? '↑' : '↓'} {delta}
          </span>
          <span className="ml-1 text-tx-3">vs 1h ago</span>
        </div>
      )}
      {spark && (
        <div className="pointer-events-none absolute bottom-0 right-0 h-9 w-3/5 opacity-55">{spark}</div>
      )}
    </div>
  );
}

/* ───────────────────────── Tabs ───────────────────────── */

export function TabBar({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'flex border-b border-bd-0 bg-bg-1 px-3',
        className,
      )}
    >
      {children}
    </div>
  );
}

export function TabItem({
  active,
  count,
  onClick,
  children,
}: {
  active?: boolean;
  count?: number | string;
  onClick?: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        '-mb-px flex min-h-9 items-center gap-2 border-b-2 px-3 py-2 font-sans text-sm font-strong',
        active ? 'border-indigo font-bold text-tx-0' : 'border-transparent text-tx-2 hover:text-tx-0',
      )}
    >
      {children}
      {count !== undefined && (
        <span className="grid min-h-5 min-w-5 place-items-center rounded-full bg-bg-3 px-1.5 font-sans text-xs text-tx-2">
          {count}
        </span>
      )}
    </button>
  );
}

/* ───────────────────────── DataTable ───────────────────────── */

// Thin pass-throughs over the token-aware shadcn Table primitive in
// `shell/ui/table.tsx`. Keeping the chrome-level names (`DataTable` / `Th`
// / `Td` / `Tr`) so the existing call sites in fixtures and ad-hoc tables
// don't churn, while every visual token (row-height, header tracking,
// hover layer) flows from the primitive.

export function DataTable({ children, className }: { children: React.ReactNode; className?: string }) {
  return <ShadTable className={cn('font-strong', className)}>{children}</ShadTable>;
}

export function Th({ children, className }: { children?: React.ReactNode; className?: string }) {
  return (
    <ShadTableHead className={cn('sticky top-0 bg-bg-1', className)}>{children}</ShadTableHead>
  );
}

export function Td({ children, className }: { children?: React.ReactNode; className?: string }) {
  return (
    <ShadTableCell
      className={cn('overflow-hidden text-ellipsis whitespace-nowrap text-tx-1', className)}
    >
      {children}
    </ShadTableCell>
  );
}

/* ───────────────────────── Row ───────────────────────── */

export function Tr({
  children,
  className,
  onClick,
}: {
  children: React.ReactNode;
  className?: string;
  onClick?: () => void;
}) {
  return (
    <ShadTableRow onClick={onClick} className={cn(onClick && 'cursor-pointer', className)}>
      {children}
    </ShadTableRow>
  );
}

/* ───────────────────────── TimeRangeChip ───────────────────────── */

export function TimeRangeChip({
  value,
  onClick,
}: {
  value?: string;
  onClick?: () => void;
}) {
  return (
    <TimeRangeControl
      {...(value !== undefined ? { value } : {})}
      {...(onClick !== undefined ? { onClick } : {})}
    />
  );
}

/* ───────────────────────── QueryInput (single-line) ───────────────────────── */

export function QueryInput({
  value,
  onChange,
  placeholder,
  lang,
  className,
}: {
  value: string;
  onChange?: (v: string) => void;
  placeholder?: string;
  lang?: 'sql' | 'promql' | 'text';
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex h-9 items-center gap-2.5 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0',
        className,
      )}
    >
      <Search className="h-4 w-4 text-tx-3" />
      <input
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        placeholder={placeholder}
        className="flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:outline-none"
      />
      {lang && (
        <span className="rounded-sm border border-bd-1 bg-bg-3 px-1.5 py-px font-sans text-xs text-tx-2">
          {lang}
        </span>
      )}
    </div>
  );
}
