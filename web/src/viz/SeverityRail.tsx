import { cn } from '@/shell/lib/cn';

/**
 * Severity color grammar — the single source of truth for how the product
 * encodes incident/alert severity as color:
 *   critical = red    (firing / on fire)
 *   error    = red    (graded just below critical; shares the "fire" red and
 *                      is distinguished by position/label — matches the
 *                      `AlertsInsights` severity→tone grammar)
 *   warning  = yellow (pending / needs attention)
 *   info     = blue   (informational)
 *
 * `critical: bg-orange` was a legacy Terminal-palette artifact — orange now
 * belongs to brand-secondary, not "this is on fire."
 */
export type Severity = 'critical' | 'error' | 'warning' | 'info';

export const SEVERITY_BAR_CLASS: Record<Severity, string> = {
  critical: 'bg-red',
  error: 'bg-red',
  warning: 'bg-yellow',
  info: 'bg-blue',
};

const CATEGORICAL_BAR_CLASS: Record<string, string> = {
  fatal: 'bg-red',
  interrupted: 'bg-red',
  warn: 'bg-yellow',
  delayed: 'bg-yellow',
  unknown: 'bg-orange',
  healthy: 'bg-green',
  debug: 'bg-tx-3',
  trace: 'bg-tx-3',
  idle: 'bg-tx-3',
  unused: 'bg-tx-3',
};

/** Bar fill class for alert, log-level, and stream-health categories. */
export function severityBarClass(severity?: string | null): string {
  const normalized = severity?.trim().toLowerCase() ?? 'info';
  return CATEGORICAL_BAR_CLASS[normalized]
    ?? SEVERITY_BAR_CLASS[normalized as Severity]
    ?? SEVERITY_BAR_CLASS.info;
}

/**
 * SeverityRail — the thin colored bar the product uses to flag the severity
 * of a row (alerts table, NOC incident list, log level, and stream health).
 * Centralizes the severity → color grammar so every surface agrees. Sizing
 * is left to the caller via `className` (e.g. `h-row w-[3px]` in a table
 * cell, `h-9` inside a fixed-width grid column).
 */
export function SeverityRail({
  severity,
  className,
}: {
  severity?: string | null | undefined;
  className?: string | undefined;
}) {
  return <span className={cn('block', severityBarClass(severity), className)} />;
}
