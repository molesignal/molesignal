import { cn } from '@/shell/lib/cn';

export interface SpectrumSegment {
  key: string;
  label: string;
  value: number;
  /** A CSS colour, normally a status token: `var(--green)`. */
  color: string;
}

/**
 * How a whole divides into parts, as one bar: each part takes the share of the
 * bar it has of the total, with what the parts are and how many spelled out
 * under it. A part of zero leaves the bar but stays in the list, so the reader
 * sees it was counted.
 */
export function SpectrumBar({
  label,
  segments,
  legend = true,
  className,
}: {
  /** What the bar divides, for assistive technology: "Service health". */
  label: string;
  segments: readonly SpectrumSegment[];
  /**
   * Spell the parts out under the bar. A row of a list that states them in
   * words beside the bar turns it off; the bar still names them to assistive
   * technology.
   */
  legend?: boolean | undefined;
  className?: string | undefined;
}) {
  const present = segments.filter((segment) => segment.value > 0);
  const summary = segments.map((segment) => `${segment.label} ${segment.value}`).join(', ');

  return (
    <div data-spectrum className={className}>
      <div
        role="img"
        aria-label={`${label}: ${summary}`}
        className={cn(
          'flex h-[8px] w-full gap-[2px] overflow-hidden rounded-full',
          present.length > 0 ? 'ms-wipe-in' : 'bg-bd-0',
        )}
      >
        {present.map((segment) => (
          <span
            key={segment.key}
            data-spectrum-segment={segment.key}
            // Even a single one among hundreds is wide enough to be seen.
            className="min-w-[6px] rounded-full"
            style={{ flexGrow: segment.value, flexBasis: 0, background: segment.color }}
          />
        ))}
      </div>
      {legend && <ul className="mt-[10px] flex flex-wrap gap-x-[16px] gap-y-[4px]">
        {segments.map((segment) => (
          <li
            key={segment.key}
            className={cn(
              'inline-flex items-center gap-[6px] font-sans text-xs',
              segment.value > 0 ? 'text-tx-1' : 'text-tx-3',
            )}
          >
            <span
              aria-hidden="true"
              className="h-[6px] w-[6px] shrink-0 rounded-full"
              style={{ background: segment.value > 0 ? segment.color : 'var(--bd-1)' }}
            />
            {segment.label}
            <strong className="font-strong tabular-nums text-tx-0">{segment.value}</strong>
          </li>
        ))}
      </ul>}
    </div>
  );
}
