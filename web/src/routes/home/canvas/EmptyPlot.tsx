import { ChromeButton } from '@/shell/chrome';

import { SignalLine } from './SignalLine';

/**
 * What the intake chart shows when the window holds no data: the signal line
 * running flat, a plain statement, and the next step. It replaces an axis
 * scaled to a made-up 0–100 range.
 */
export function EmptyPlot({
  title,
  hint,
  actionLabel,
  onAction,
}: {
  title: string;
  hint: string;
  actionLabel: string;
  onAction: () => void;
}) {
  return (
    <div
      role="status"
      data-testid="home-chart-empty"
      className="relative flex h-full min-h-[160px] flex-col items-center justify-center overflow-hidden rounded-md bg-[var(--control-surface)] px-6 text-center"
    >
      <SignalLine
        className="pointer-events-none absolute inset-x-0 top-1/2 h-12 w-full -translate-y-1/2 opacity-60"
      />
      <div className="relative z-10 flex max-w-[42ch] flex-col items-center gap-1 rounded-md bg-[var(--control-surface)] px-4 py-3">
        <p className="font-sans text-sm font-strong text-tx-0">{title}</p>
        <p className="text-pretty font-sans text-xs text-tx-2">{hint}</p>
        <ChromeButton size="sm" className="mt-2" onClick={onAction}>
          {actionLabel}
        </ChromeButton>
      </div>
    </div>
  );
}
