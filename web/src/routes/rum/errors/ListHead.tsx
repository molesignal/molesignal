import { Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumIssueFilter } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';
import type { UserLite } from '@/shell/useUsers';

import { DIMENSIONS, type DimensionKey, STATUS_FILTERS } from './model';
import type { ErrorsView } from './useErrors';

const ANYONE = '__anyone__';

const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/** How long typing waits before the list is asked again. */
const TYPING_MS = 250;

/**
 * What narrows the list: words, a state, a holder. Everything lives in the address, so a view of
 * the list can be sent; what is picked in the distributions above is told here, to be let go of.
 */
export function ListHead({
  view,
  onChange,
  onPick,
  users,
  total,
  sources,
  source,
  onSource,
}: {
  view: ErrorsView;
  onChange: (changes: Record<string, string | null>) => void;
  onPick: (key: DimensionKey, value: string | null) => void;
  users: readonly UserLite[];
  total: number | null;
  /** What sent the errors, when more than one thing did. */
  sources: readonly string[];
  source: string;
  onSource: (source: string) => void;
}) {
  const { t } = useTranslation('rum');
  const [draft, setDraft] = React.useState(view.search);

  // What the address says wins over what was typed when it changes elsewhere.
  React.useEffect(() => setDraft(view.search), [view.search]);

  React.useEffect(() => {
    if (draft === view.search) return undefined;

    const timer = globalThis.setTimeout(() => onChange({ q: draft.trim() ? draft : null }), TYPING_MS);

    return () => globalThis.clearTimeout(timer);
  }, [draft, view.search, onChange]);

  const picked = DIMENSIONS.flatMap(({ key }) => (view.picked[key] ? [{ key, value: view.picked[key] as string }] : []));

  return (
    <div data-rum-list-head="" className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-bd-0 px-3 py-2">
      <label className="relative">
        <span className="sr-only">{t('errors.search.label')}</span>
        <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
        <input
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={t('errors.search.placeholder')}
          className="h-8 w-72 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
        />
      </label>
      <FittedSelect
        aria-label={t('errors.filters.status')}
        data-rum-filter="status"
        value={view.status}
        onValueChange={(status) => onChange({ status: status === 'unresolved' ? null : (status as RumIssueFilter) })}
        options={STATUS_FILTERS.map((value) => ({ value, label: t(`errors.filter.${value}`) }))}
        maxWidth={200}
        prefix={t('errors.filters.status')}
        className={cn(CONTROL, 'font-strong text-tx-1')}
      />
      <FittedSelect
        aria-label={t('errors.filters.holder')}
        data-rum-filter="holder"
        value={view.assignee || ANYONE}
        onValueChange={(value) => onChange({ assignee: value === ANYONE ? null : value })}
        options={[
          { value: ANYONE, label: t('errors.holder.anyone') },
          { value: 'none', label: t('errors.holder.nobody') },
          ...users.map((user) => ({ value: user.id, label: user.name })),
        ]}
        maxWidth={220}
        prefix={t('errors.filters.holder')}
        className={cn(CONTROL, 'font-strong text-tx-1')}
      />
      {sources.length > 1 && (
        <FittedSelect
          aria-label={t('errors.filters.source')}
          data-rum-filter="source"
          value={source || ANYONE}
          onValueChange={(value) => onSource(value === ANYONE ? '' : value)}
          options={[{ value: ANYONE, label: t('errors.holder.anyone') }, ...sources.map((name) => ({ value: name, label: name }))]}
          maxWidth={200}
          prefix={t('errors.filters.source')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
      )}
      {total !== null && (
        <span data-rum-issue-count="" className="font-sans text-xs text-tx-3">
          {t('errors.count', { count: total })}
        </span>
      )}
      {picked.map(({ key, value }) => (
        <button
          key={key}
          type="button"
          onClick={() => onPick(key, null)}
          data-rum-picked={key}
          aria-label={t('errors.picked.clear', { dimension: t(`errors.spread.${key}`), value })}
          className="inline-flex h-8 items-center gap-1.5 rounded-md border-0 bg-transparent px-1.5 font-sans text-xs text-tx-1 outline-none hover:text-tx-0 focus-visible:text-tx-0"
        >
          <span className="text-tx-3">{t(`errors.spread.${key}`)}</span>
          <span className="font-strong">{value}</span>
          <X aria-hidden className="h-3 w-3 text-tx-3" />
        </button>
      ))}
    </div>
  );
}
