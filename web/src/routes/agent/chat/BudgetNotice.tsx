import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { BudgetNotice } from '@/api/agent/chat';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import { formatTokens, usedPercent } from '../settings/Usage/model';

/**
 * The organization's day budget, said where the question is typed: close to the
 * limit, past it with chats still answered, or past it with this question not sent.
 */
export function ChatBudgetNotice({ notice }: { notice: BudgetNotice | null }) {
  const { t } = useTranslation('agent');
  if (!notice) return null;

  const state = notice.blocked ? 'blocked' : notice.level;
  const values = {
    percent: usedPercent(notice) ?? 0,
    used: formatTokens(notice.used_tokens),
    limit: formatTokens(notice.limit_tokens),
    time: formatMicrosActive(notice.resets_at_micros, false),
  };

  return (
    <div
      data-agent-budget-notice={state}
      role="status"
      className={cn(
        'mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md px-3 py-2 font-sans text-xs',
        state === 'blocked'
          ? 'bg-red-dim text-[var(--red-ink)]'
          : 'bg-[var(--yellow-tint)] text-[var(--yellow-ink)]',
      )}
    >
      <span className="min-w-0 flex-1">{t(`chat_budget.${state}`, values)}</span>
      <Link
        to="/agent/settings/usage"
        className="shrink-0 rounded-md bg-bg-1 px-2.5 py-1 font-strong text-tx-0 hover:bg-bg-2 focus-visible:bg-bg-2"
      >
        {t('chat_budget.open_usage')}
      </Link>
    </div>
  );
}
