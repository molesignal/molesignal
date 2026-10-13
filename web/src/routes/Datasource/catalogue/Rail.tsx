import { Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, type To } from 'react-router-dom';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import type { Source } from '../sources';
import type { SourceText } from '../sourceText';
import { sourceIcon } from './sourceIcon';

export interface CatalogueEmpty {
  message: string;
  /** A way out, when the list is the place to offer it. */
  action?: { label: string; run: () => void } | undefined;
}

/**
 * The sources of the category in view, or what a search found in all of them.
 * A row says what the source is and which signals it sends; whether data is
 * arriving is the guide's to say, where it can be checked.
 */
export function CatalogueRail({
  sources,
  selectedId,
  query,
  onQueryChange,
  linkFor,
  onPick,
  onSubmit,
  categoryOf,
  empty,
  text,
}: {
  sources: readonly Source[];
  selectedId: string | undefined;
  query: string;
  onQueryChange: (query: string) => void;
  linkFor: (source: Source) => To;
  /** A row was followed. */
  onPick: () => void;
  /** Enter in the search field: go to the first source found. */
  onSubmit: () => void;
  /** Named on a search result, which may come from any category. */
  categoryOf: (source: Source) => string;
  empty: CatalogueEmpty;
  text: SourceText;
}) {
  const { t } = useTranslation('onboarding');
  const searching = query.trim() !== '';

  return (
    <aside
      data-datasource-catalog-surface
      className={cn('flex w-[272px] shrink-0 flex-col overflow-hidden', surfacePanelClass)}
    >
      <div className="shrink-0 p-[10px] pb-[6px]">
        <label
          data-ui="input-control"
          className="flex h-9 items-center gap-2 rounded-md border-0 bg-[var(--control-surface)] px-3 transition-colors focus-within:bg-bg-3 hover:bg-bg-3"
        >
          <Search aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') onSubmit();
              if (event.key === 'Escape') onQueryChange('');
            }}
            placeholder={t('datasource_page.search_placeholder')}
            aria-label={t('datasource_page.search_placeholder')}
            className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
          />
          {searching && (
            <button
              type="button"
              onClick={() => onQueryChange('')}
              aria-label={t('datasource_page.clear_search')}
              className="-mr-1 grid h-6 w-6 shrink-0 place-items-center rounded text-tx-3 transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 focus-visible:outline-none"
            >
              <X aria-hidden className="h-3.5 w-3.5" />
            </button>
          )}
        </label>
      </div>

      {searching && sources.length > 0 && (
        <div className="type-caption shrink-0 px-[20px] pb-[4px] pt-[2px] font-sans text-tx-3">
          {t('datasource_page.matching', { count: sources.length })}
        </div>
      )}

      <nav
        aria-label={t('datasource_page.source_navigation')}
        className="min-h-0 flex-1 overflow-y-auto px-[6px] pb-[8px]"
      >
        {sources.length === 0 ? (
          <div className="px-[14px] py-[28px] text-center">
            <p className="font-sans text-xs leading-relaxed text-tx-2">{empty.message}</p>
            {empty.action && (
              <ChromeButton size="sm" className="mt-3" onClick={empty.action.run}>
                {empty.action.label}
              </ChromeButton>
            )}
          </div>
        ) : (
          <ul className="m-0 list-none space-y-px p-0">
            {sources.map((source) => (
              <li key={source.id}>
                <SourceRow
                  source={source}
                  selected={source.id === selectedId}
                  to={linkFor(source)}
                  onPick={onPick}
                  name={text.name(source)}
                  detail={[
                    ...(searching ? [categoryOf(source)] : []),
                    ...source.signals.map((signal) => t(`datasource_page.signals.${signal}`)),
                  ].join(' · ')}
                />
              </li>
            ))}
          </ul>
        )}
      </nav>
    </aside>
  );
}

function SourceRow({
  source,
  selected,
  to,
  onPick,
  name,
  detail,
}: {
  source: Source;
  selected: boolean;
  to: To;
  onPick: () => void;
  name: string;
  detail: string;
}) {
  const Icon = sourceIcon(source);
  const ref = React.useRef<HTMLAnchorElement>(null);

  // A source reached from a link elsewhere may sit below the fold of its list.
  React.useEffect(() => {
    if (selected) ref.current?.scrollIntoView?.({ block: 'nearest' });
  }, [selected]);

  return (
    <Link
      ref={ref}
      to={to}
      onClick={onPick}
      data-datasource-source={source.id}
      aria-current={selected ? 'page' : undefined}
      className={cn(
        'group grid grid-cols-[16px_minmax(0,1fr)] gap-x-[10px] rounded-md px-[10px] py-[7px] outline-none transition-colors duration-fast',
        selected ? 'bg-indigo-dim/60' : 'hover:bg-bg-2 focus-visible:bg-bg-2',
      )}
    >
      <Icon
        aria-hidden
        strokeWidth={1.8}
        className={cn(
          'mt-[2px] h-4 w-4 transition-colors duration-fast',
          selected ? 'text-indigo-soft' : 'text-tx-3 group-hover:text-tx-1',
        )}
      />
      <span className="min-w-0">
        <span
          className={cn(
            'block truncate font-sans text-sm',
            selected ? 'font-strong text-tx-0' : 'font-body text-tx-1 group-hover:text-tx-0',
          )}
        >
          {name}
        </span>
        <span className="block truncate font-sans text-xs text-tx-3">{detail}</span>
      </span>
    </Link>
  );
}
