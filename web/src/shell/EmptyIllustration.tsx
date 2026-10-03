import { cn } from '@/shell/lib/cn';

export type EmptyIllustrationKind = 'schedule' | 'activity' | 'streams';

/**
 * Line illustrations for empty states, drawn in the same stroke language as
 * the product mark: round caps, one weight, and a single indigo pulse as the
 * only accent. They are decorative; the heading beside them says the thing.
 */
export function EmptyIllustration({
  kind,
  className,
}: {
  kind: EmptyIllustrationKind;
  className?: string | undefined;
}) {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      width="72"
      height="48"
      viewBox="0 0 72 48"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn('shrink-0 text-tx-3', className)}
    >
      {kind === 'schedule' && (
        <>
          {/* a rota: calendar frame, pegs, a row of days with one slot taken */}
          <rect x="14" y="8" width="44" height="34" rx="6" />
          <path d="M14 18h44" />
          <path d="M26 5v6M46 5v6" />
          <path d="M23 28h.01M32 28h.01M41 28h.01M50 28h.01M23 35h.01M32 35h.01" />
          <circle cx="41" cy="35" r="3.25" stroke="var(--indigo)" />
        </>
      )}
      {kind === 'activity' && (
        <>
          {/* a timeline: rail, nodes, and the lines of text they point at */}
          <path d="M18 8v32" strokeDasharray="1 4" />
          <circle cx="18" cy="10" r="3.25" stroke="var(--indigo)" />
          <path d="M28 10h30" />
          <circle cx="18" cy="24" r="2.5" />
          <path d="M28 24h22" />
          <circle cx="18" cy="38" r="2.5" />
          <path d="M28 38h26" />
        </>
      )}
      {kind === 'streams' && (
        <>
          {/* three feeds; the top one carries the pulse */}
          <path d="M6 12h14l4-7 5 14 4-7h33" stroke="var(--indigo)" />
          <path d="M6 26h60" />
          <path d="M6 40h40" />
          <path d="M66 40h.01" />
        </>
      )}
    </svg>
  );
}
