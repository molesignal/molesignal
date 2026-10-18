import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MessageReference } from '@/api/agent/chat';
import { type ReferenceCandidate, searchReferences } from '@/api/agent/references';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import {
  floatingMenuContentClass,
  floatingMenuItemClass,
  floatingMenuLabelClass,
} from '@/shell/ui/floating';

import { attached, type Mention, mentionAt } from './mentions';
import { REFERENCE_ICON } from './ReferenceChips';
import { OVER_CHAT } from '../CommandMenu';

/**
 * `@` in the composer: what is typed after it searches the incidents, alert
 * rules, services, dashboards and streams the person can read; choosing one
 * attaches it to the question and takes the word out of the text.
 */
export function useReferenceMention({
  input,
  caret,
  references,
  onReferencesChange,
  onRemoveMention,
}: {
  /** The composer's text, as plain text, and where its caret is in it. */
  input: string;
  caret: number;
  references: readonly MessageReference[];
  onReferencesChange: (references: MessageReference[]) => void;
  /** Takes the `@word` out of the composer once it became a reference. */
  onRemoveMention: (mention: Mention) => void;
}) {
  const [dismissedAt, setDismissedAt] = React.useState<string | null>(null);
  const [active, setActive] = React.useState(0);
  const mention = dismissedAt === input ? null : mentionAt(input, caret);
  const query = mention?.query ?? null;

  const search = useQuery({
    queryKey: ['agent', 'references', query],
    queryFn: () => searchReferences(query ?? ''),
    enabled: query !== null,
    staleTime: 30_000,
    retry: false,
  });

  const candidates = (search.data ?? []).filter((candidate) => !attached(references, candidate));
  React.useEffect(() => setActive(0), [query]);

  // The menu shows while an `@` is typed, also when there is nothing to offer: it says what
  // can be attached, that nothing matched, or why the search failed.
  const open = mention !== null;

  const choose = (index: number) => {
    const candidate = candidates[index];
    if (!candidate || !mention) return;
    onReferencesChange([
      ...references,
      { kind: candidate.kind, id: candidate.id, label: candidate.label },
    ]);
    onRemoveMention(mention);
  };

  /** Arrows move, Enter or Tab attach, Escape closes; true when the key was the menu's. */
  const onKeyDown = (
    event: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'preventDefault' | 'stopPropagation'>,
  ): boolean => {
    if (!open) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      setDismissedAt(input);
      return true;
    }
    if (candidates.length === 0) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const step = event.key === 'ArrowDown' ? 1 : -1;
      setActive((current) => (current + step + candidates.length) % candidates.length);
      return true;
    }
    if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
      event.preventDefault();
      choose(active);
      return true;
    }
    return false;
  };

  return {
    open,
    query: query ?? '',
    candidates,
    active,
    setActive,
    choose,
    onKeyDown,
    loading: search.isFetching,
    error: search.isError ? toApiError(search.error).message : null,
  };
}

/** The menu over the composer while an `@` is typed. */
export function ReferenceMenu({
  query,
  candidates,
  active,
  loading,
  error,
  onActivate,
  onChoose,
}: {
  query: string;
  candidates: readonly ReferenceCandidate[];
  active: number;
  loading: boolean;
  error: string | null;
  onActivate: (index: number) => void;
  onChoose: (index: number) => void;
}) {
  const { t } = useTranslation('agent');
  const note =
    candidates.length > 0
      ? query
        ? null
        : t('references.what')
      : loading
        ? t('references.searching')
        : error
          ? t('references.failed', { reason: error })
          : query
            ? t('references.no_match', { query })
            : t('references.none');
  return (
    <div data-agent-reference-menu className={cn(floatingMenuContentClass, OVER_CHAT, 'max-h-72 overflow-auto')}>
      <div className={floatingMenuLabelClass}>{t('references.menu_title')}</div>
      {note && (
        <p data-agent-reference-note className="px-[10px] pb-1.5 text-xs text-tx-3">
          {note}
        </p>
      )}
      <ul role="listbox" aria-label={t('references.menu_title')}>
        {candidates.map((candidate, index) => {
          const Icon = REFERENCE_ICON[candidate.kind];
          return (
            <li
              key={`${candidate.kind}:${candidate.id}`}
              role="option"
              aria-selected={index === active}
              data-highlighted={index === active ? '' : undefined}
              onMouseEnter={() => onActivate(index)}
              onMouseDown={(event) => {
                // The composer keeps the focus and the caret.
                event.preventDefault();
                onChoose(index);
              }}
              className={cn(floatingMenuItemClass, 'min-w-0 gap-2')}
            >
              <Icon aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-indigo" />
              <span className="shrink-0 text-xs text-tx-3">
                {t(`references.kinds.${candidate.kind}`)}
              </span>
              <span className="min-w-0 truncate text-xs font-strong text-tx-0">{candidate.label}</span>
              {candidate.detail && (
                <span className="ml-auto shrink-0 type-micro text-tx-3">{candidate.detail}</span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
