import { Braces, List, MoreHorizontal, Search } from 'lucide-react';
import { useContext, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { traceFieldQueryFromParams } from '@/routes/traces/urlState';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { Button } from '@/shell/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/shell/ui/dropdown-menu';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { appendAttributeClause, attributeClause, attributeEntries, AttributeFilterContext, attributeGroup, attributeValue } from './model';

export function Attributes({ attributes }: { attributes: unknown }) {
  const { t } = useTranslation('traces');
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState(false);
  const [raw, setRaw] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  useEffect(() => {
    if (copied === null) return;
    const timer = window.setTimeout(() => setCopied(null), 1800);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const filter = useContext(AttributeFilterContext);
  const navigate = useNavigate();
  const location = useLocation();
  const entries = attributeEntries(attributes);
  const matches = entries.filter(([key, value]) => `${key} ${attributeValue(value)}`.toLowerCase().includes(search.trim().toLowerCase()));
  const grouped = entries.length > 16;
  const ordered = grouped ? [...matches].sort(([a], [b]) => attributeGroup(a).localeCompare(attributeGroup(b)) || a.localeCompare(b)) : matches;
  const visible = expanded || search.trim() ? ordered : ordered.slice(0, 8);
  const copy = async (text: string, key: string) => {
    try { await writeClipboardText(text); setCopied(key); }
    catch { toast.error(t('attributes.copy_failed')); }
  };
  const addFilter = (key: string, value: string | number | boolean, operator: '=' | '!=') => {
    if (filter) { filter(key, value, operator); return; }
    const params = new URLSearchParams(location.search);
    const query = traceFieldQueryFromParams(params);
    params.delete('sql');
    params.set('q', appendAttributeClause(query, attributeClause(key, value, operator)));
    params.set('tab', 'spans');
    navigate(`/traces?${params.toString()}`);
  };
  return (
    <section className="min-w-0 border-b border-bd-0 p-4" aria-label={t('attributes.title')}>
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="text-xs font-semibold text-tx-2">{t('attributes.title')} <span className="ml-2 font-mono tabular-nums text-tx-3">{entries.length}</span></h3>
        <div className="flex rounded-md bg-bg-2 p-0.5" role="group" aria-label={t('attributes.view')}>
          <Button variant="ghost" size="icon" className={`h-7 w-7 ${!raw ? 'bg-indigo-dim text-indigo-soft' : ''}`} aria-label={t('attributes.list')} aria-pressed={!raw} onClick={() => setRaw(false)}><List className="h-3.5 w-3.5" /></Button>
          <Button variant="ghost" size="icon" className={`h-7 w-7 ${raw ? 'bg-indigo-dim text-indigo-soft' : ''}`} aria-label={t('attributes.json')} aria-pressed={raw} onClick={() => setRaw(true)}><Braces className="h-3.5 w-3.5" /></Button>
        </div>
      </div>
      <div className="relative mb-2">
        <Search aria-hidden="true" className="pointer-events-none absolute left-2.5 top-2.5 h-3.5 w-3.5 text-tx-3" />
        <Input aria-label={t('attributes.search')} placeholder={t('attributes.search')} className="h-9 min-w-0 pl-8 text-xs" value={search} onChange={(e) => setSearch(e.target.value)} />
      </div>
      {!matches.length ? <p className="py-4 text-xs text-tx-3" role="status">{t(entries.length ? 'attributes.no_matches' : 'attributes.empty')}</p> : raw ? (
        <pre className="whitespace-pre-wrap break-words rounded-md bg-bg-1 p-3 font-mono text-xs leading-5 text-tx-1 [overflow-wrap:anywhere]">{JSON.stringify(Object.fromEntries(matches), null, 2)}</pre>
      ) : (
        <dl className="min-w-0">
          {visible.map(([key, value], index) => {
            const text = attributeValue(value);
            const scalar = ['string', 'number', 'boolean'].includes(typeof value);
            const canFilter = scalar && text.length > 0 && /^[a-zA-Z_][\w.]*$/.test(key) && filter !== null;
            const group = attributeGroup(key);
            const startGroup = grouped && (index === 0 || attributeGroup(visible[index - 1]![0]) !== group);
            return (
              <div key={key}>
                {startGroup && <div className="mt-2 flex justify-between text-[10px] font-semibold tracking-normal text-tx-3">{t(`attributes.groups.${group}`)}<span>{matches.filter(([k]) => attributeGroup(k) === group).length}</span></div>}
                <div className="group min-w-0 border-b border-bd-0 py-1.5 last:border-b-0">
                  <dt className="truncate font-mono text-xs leading-4 text-tx-3" title={key}>{key}</dt>
                  <dd className="mt-0.5 flex min-w-0 items-center gap-1">
                    <Tooltip><TooltipTrigger asChild><span tabIndex={0} className="min-w-0 flex-1 truncate rounded-sm text-sm leading-5 text-tx-0 focus-visible:bg-bg-2">{text || '""'}</span></TooltipTrigger><TooltipContent className="max-w-[min(32rem,80vw)] whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{text || '""'}</TooltipContent></Tooltip>
                    <div className="flex shrink-0 items-center sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100 [&:has([data-state=open])]:opacity-100">
                      <CopyIconButton className="h-6 w-6" label={t('attributes.copy_value_for', { key })} copiedLabel={t('attributes.copied')} copied={copied === key} onClick={() => { void copy(text, key); }} />
                      <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon" className="h-6 w-6" aria-label={t('attributes.actions', { key })}><MoreHorizontal className="h-3.5 w-3.5" /></Button></DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => { void copy(key, key); }}>{t('attributes.copy_key')}</DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => { void copy(text, key); }}>{t('attributes.copy_value')}</DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => { void copy(`${key}=${text}`, key); }}>{t('attributes.copy_pair')}</DropdownMenuItem>
                          <DropdownMenuSeparator />
                          {(['=', '!='] as const).map((operator) => <DropdownMenuItem key={operator} disabled={!canFilter} disabledReason={!canFilter ? t(filter === null ? 'attributes.fields_only' : 'attributes.scalar_only') : undefined} onSelect={() => addFilter(key, value as string | number | boolean, operator)}>{t(operator === '=' ? 'attributes.include' : 'attributes.exclude')}</DropdownMenuItem>)}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </dd>
                </div>
              </div>
            );
          })}
        </dl>
      )}
      {!raw && !search.trim() && entries.length > 8 && <Button variant="ghost" className="mt-2 h-8 w-full text-xs text-indigo-soft" onClick={() => setExpanded(!expanded)}>{t(expanded ? 'attributes.show_less' : 'attributes.show_all', { count: entries.length })}</Button>}
    </section>
  );
}
