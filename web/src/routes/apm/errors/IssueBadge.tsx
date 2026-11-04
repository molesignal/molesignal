import { useTranslation } from 'react-i18next';

import type { ErrorIssue } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

/** What an error group's state reads as: a group nobody has touched is open; a resolved one seen again regressed. */
export type IssueState = 'open' | 'investigating' | 'regressed' | 'resolved' | 'ignored';

export function issueState(issue: ErrorIssue | null | undefined): IssueState {
  if (!issue) return 'open';
  if (issue.regressed) return 'regressed';
  return issue.status;
}

/** Each state in the ink of what it says. */
const CLASS: Record<IssueState, string> = {
  open: 'text-tx-1',
  investigating: 'text-[var(--blue-ink)]',
  regressed: 'text-[var(--red-ink)]',
  resolved: 'text-[var(--green-ink)]',
  ignored: 'text-tx-3',
};

export function IssueBadge({ issue, className }: { issue: ErrorIssue | null | undefined; className?: string }) {
  const { t } = useTranslation('apm');
  const state = issueState(issue);

  return (
    <span data-apm-issue-state={state} className={cn('shrink-0 font-sans text-xs font-strong', CLASS[state], className)}>
      {t(`errors.state.${state}`)}
    </span>
  );
}
