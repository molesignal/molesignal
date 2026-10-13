import { cn } from '@/shell/lib/cn';
import type { LabelMatcher, MatchOp } from '@/types/alerting';

/** A matcher's operator as it is written wherever matchers are read: the notation of PromQL. */
export const MATCH_OP_SYMBOL: Record<MatchOp, string> = {
  eq: '=',
  neq: '!=',
  re: '=~',
  nre: '!~',
};

/**
 * Label matchers as they are read: `service = payments`, one chip each. A list
 * shows what a policy catches with them, so nobody has to open the policy to
 * learn what it is about. All of them have to hit, which the row of chips
 * says by being a row.
 */
export function MatcherChips({
  matchers,
  empty,
  className,
}: {
  matchers: readonly LabelMatcher[];
  /** What to say for a policy with no matcher: it catches everything. */
  empty: string;
  className?: string | undefined;
}) {
  if (matchers.length === 0) {
    return <span className={cn('text-xs text-tx-2', className)}>{empty}</span>;
  }

  return (
    <ul className={cn('flex min-w-0 flex-wrap gap-[6px]', className)}>
      {matchers.map((matcher, index) => (
        <li
          key={`${matcher.label}:${matcher.op}:${matcher.value}:${index}`}
          className="inline-flex max-w-full items-center gap-[4px] rounded-md bg-bg-2 px-[8px] py-[3px] font-mono text-xs"
        >
          <span className="truncate text-tx-1">{matcher.label}</span>
          <span className="shrink-0 text-tx-3">{MATCH_OP_SYMBOL[matcher.op]}</span>
          <span className="truncate text-tx-0">{matcher.value}</span>
        </li>
      ))}
    </ul>
  );
}
