import { CircleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { GlobalFilter } from '@/stores/useFiltersStore';

import type { LogLinkGap } from './model';

interface LogConditionNoticeProps {
  stream: string;
  /** What the link that opened the page asked for, that the stream keeps no field for. */
  link: readonly LogLinkGap[];
  /** The pinned filters the stream keeps no field for. */
  pinned: readonly GlobalFilter[];
}

/**
 * Under the query, what was asked of it that the stream cannot answer: a
 * condition of the link that opened the page, or a pinned filter, with no
 * field in the stream to put it on. It is left out of the query rather than
 * written on a field that is not there, and this says which, and why once.
 */
export function LogConditionNotice({ stream, link, pinned }: LogConditionNoticeProps) {
  const { t, i18n } = useTranslation('logs');
  const list = new Intl.ListFormat(i18n.resolvedLanguage ?? i18n.language, { type: 'conjunction', style: 'narrow' });
  const linkConditions = list.format(
    link.map((gap) => `${t(`explore.conditions.meanings.${gap.meaning}`)} = ${gap.value}`),
  );
  const pinnedConditions = t('explore.conditions.pinned', {
    count: pinned.length,
    conditions: list.format(
      pinned.map((filter) => `${filter.key} ${filter.operator === '!=' ? '!=' : '='} ${filter.value}`),
    ),
  });
  const conditions =
    link.length > 0 && pinned.length > 0
      ? t('explore.conditions.groups', { link: linkConditions, pinned: pinnedConditions })
      : link.length > 0
        ? linkConditions
        : pinnedConditions;

  return (
    <p data-log-condition-notice className="flex items-start gap-1.5 font-sans text-xs text-tx-2">
      <CircleAlert className="mt-px h-3.5 w-3.5 shrink-0 text-[var(--yellow-ink)]" aria-hidden="true" />
      <span>
        {t('explore.conditions.unmatched', { count: link.length + pinned.length, conditions, stream })}
      </span>
    </p>
  );
}
