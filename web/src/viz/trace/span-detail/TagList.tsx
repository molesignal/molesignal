import { Braces, List, MoreHorizontal, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { traceFieldQueryFromParams } from '@/routes/traces/urlState';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { JsonView } from '@/shell/JsonEditor';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { appendAttributeClause, attributeClause, AttributeFilterContext } from './model';
import { type AttributeEntry, attributeText, groupAttributes } from '../model/span';

/** With no more tags than this there is nothing to search. */
const SEARCHABLE_AFTER = 8;

/**
 * Tags as terms and their values, each group of the span's parts under its
 * own heading: what the span says, in an order that reads. A tag can be
 * copied, or added to the query or left out of it, from the end of its line.
 */
export function TagList({
  entries,
  label,
  emptyText,
  filterable = true,
}: {
  entries: readonly AttributeEntry[];
  /** What the list is of: a span's tags, its resource's. */
  label: string;
  emptyText: string;
  /** The tags are a span's own: a query can be narrowed by them. */
  filterable?: boolean;
}) {
  const { t } = useTranslation('traces');
  const [search, setSearch] = React.useState('');
  const [raw, setRaw] = React.useState(false);
  const [copied, setCopied] = React.useState<string | null>(null);
  const filter = React.useContext(AttributeFilterContext);
  const navigate = useNavigate();
  const location = useLocation();

  React.useEffect(() => {
    if (copied === null) return undefined;
    const timer = window.setTimeout(() => setCopied(null), 1800);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const needle = search.trim().toLowerCase();
  const matches = needle
    ? entries.filter(([key, value]) => `${key} ${attributeText(value)}`.toLowerCase().includes(needle))
    : entries;
  const groups = groupAttributes(matches);

  const copy = async (text: string, key: string) => {
    try {
      await writeClipboardText(text);
      setCopied(key);
    } catch {
      toast.error(t('attributes.copy_failed'));
    }
  };

  const addFilter = (key: string, value: string | number | boolean, operator: '=' | '!=') => {
    if (filter) {
      filter(key, value, operator);
      return;
    }

    // A page without a query of its own opens the list of spans with the tag.
    const params = new URLSearchParams(location.search);
    const query = traceFieldQueryFromParams(params);
    params.delete('sql');
    params.set('q', appendAttributeClause(query, attributeClause(key, value, operator)));
    params.set('tab', 'spans');
    navigate(`/traces?${params.toString()}`);
  };

  const searchable = entries.length > SEARCHABLE_AFTER;
  const viewSwitch = (
    <div className="flex shrink-0 rounded-md bg-bg-2 p-[2px]" role="group" aria-label={t('attributes.view')}>
      <Button variant="ghost" size="icon" className={cn('h-[24px] w-[24px]', !raw && 'bg-indigo-dim text-indigo-soft')} aria-label={t('attributes.list')} aria-pressed={!raw} onClick={() => setRaw(false)}>
        <List className="h-3.5 w-3.5" />
      </Button>
      <Button variant="ghost" size="icon" className={cn('h-[24px] w-[24px]', raw && 'bg-indigo-dim text-indigo-soft')} aria-label={t('attributes.json')} aria-pressed={raw} onClick={() => setRaw(true)}>
        <Braces className="h-3.5 w-3.5" />
      </Button>
    </div>
  );

  return (
    <section aria-label={label} className="relative min-w-0">
      {searchable ? (
        <div className="mb-[2px] flex h-[30px] items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <Search aria-hidden="true" className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
            <Input
              aria-label={t('attributes.search')}
              placeholder={t('attributes.search')}
              className="h-[28px] min-w-0 pl-7 text-xs"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
          {viewSwitch}
        </div>
      ) : (
        // With nothing to search, the switch stands at the end of the first heading's line.
        entries.length > 0 && <div className="absolute right-0 top-0 z-[1]">{viewSwitch}</div>
      )}

      {matches.length === 0 ? (
        <p role="status" className="px-[6px] py-[8px] font-sans text-xs text-tx-3">
          {entries.length > 0 ? t('attributes.no_matches') : emptyText}
        </p>
      ) : raw ? (
        <JsonView
          className={searchable ? 'mt-[4px]' : 'mt-[32px]'}
          value={Object.fromEntries(matches)}
          ariaLabel={label}
          maxHeight={480}
        />
      ) : (
        groups.map(({ group, entries: listed }, index) => (
          <div key={group} className={index === 0 ? 'mt-[2px]' : 'mt-[6px]'}>
            <h3 className="flex h-[28px] items-center gap-1.5 px-[6px] font-sans text-xs font-strong text-tx-3">
              {t(`view.tags.groups.${group}`)}
              <span className="font-body tabular-nums">{listed.length}</span>
            </h3>
            <dl className="min-w-0">
              {listed.map(([key, value]) => {
                const text = attributeText(value);
                const scalar = ['string', 'number', 'boolean'].includes(typeof value);
                const canFilter = filterable && scalar && text.length > 0 && /^[a-zA-Z_][\w.]*$/.test(key) && filter !== null;

                return (
                  <div
                    key={key}
                    data-testid="trace-tag"
                    className="group relative grid min-w-0 grid-cols-[minmax(0,2fr)_minmax(0,3fr)] items-center gap-[8px] rounded-sm px-[6px] py-[5px] hover:bg-bg-2 has-[:focus-visible]:bg-bg-2"
                  >
                    <dt className="truncate font-sans text-xs leading-[18px] text-tx-2" title={key}>{key}</dt>
                    <dd className="flex min-w-0 items-center gap-1">
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span tabIndex={0} className="min-w-0 flex-1 truncate rounded-sm font-sans text-xs leading-[18px] text-tx-0 focus-visible:bg-bg-3 focus-visible:outline-none">
                            {text || '""'}
                          </span>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-[min(32rem,80vw)] whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
                          {text || '""'}
                        </TooltipContent>
                      </Tooltip>
                      <div className="flex shrink-0 items-center sm:absolute sm:inset-y-0 sm:right-[2px] sm:rounded-md sm:bg-bg-2 sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100 [&:has([data-state=open])]:opacity-100">
                        <CopyIconButton
                          className="h-[24px] w-[24px]"
                          label={t('attributes.copy_value_for', { key })}
                          copiedLabel={t('attributes.copied')}
                          copied={copied === key}
                          onClick={() => {
                            void copy(text, key);
                          }}
                        />
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button variant="ghost" size="icon" className="h-[24px] w-[24px]" aria-label={t('attributes.actions', { key })}>
                              <MoreHorizontal className="h-3.5 w-3.5" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            <DropdownMenuItem onSelect={() => void copy(key, key)}>{t('attributes.copy_key')}</DropdownMenuItem>
                            <DropdownMenuItem onSelect={() => void copy(text, key)}>{t('attributes.copy_value')}</DropdownMenuItem>
                            <DropdownMenuItem onSelect={() => void copy(`${key}=${text}`, key)}>{t('attributes.copy_pair')}</DropdownMenuItem>
                            {filterable && (
                              <>
                                <DropdownMenuSeparator />
                                {(['=', '!='] as const).map((operator) => (
                                  <DropdownMenuItem
                                    key={operator}
                                    disabled={!canFilter}
                                    disabledReason={!canFilter ? t(filter === null ? 'attributes.fields_only' : 'attributes.scalar_only') : undefined}
                                    onSelect={() => addFilter(key, value as string | number | boolean, operator)}
                                  >
                                    {t(operator === '=' ? 'attributes.include' : 'attributes.exclude')}
                                  </DropdownMenuItem>
                                ))}
                              </>
                            )}
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>
                    </dd>
                  </div>
                );
              })}
            </dl>
          </div>
        ))
      )}
    </section>
  );
}
