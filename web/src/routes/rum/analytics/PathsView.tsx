import { GitBranch } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import type { PathsParams } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { PathsChart } from './PathsChart';
import { useSuggestions, usePaths } from './useAnalytics';
import { ValueCombobox } from './ValueCombobox';
import { isBrowser, scopeFilters } from '../overview/model';
import { CardState, OverviewCard } from '../overview/OverviewCard';
import { CONTROL } from '../overview/ScopeRow';
import type { ScopeContext } from '../ScopedFrame';

const DEPTHS = [1, 2, 3, 4, 5, 6] as const;
const FIELDS = ['view.url_path_group', 'view.url_path', 'view.name'] as const;
const DEFAULT_DEPTH = 3;

/**
 * Where sessions went: from a page, the pages they opened next, or to a page, the pages they came
 * from, as columns of the most visited pages with the ribbons between them. Without a page it
 * starts from the first page of each session. Everything asked is in the address.
 */
export function PathsView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { application, scope, range, language, window, setWindow } = context;
  const browser = isBrowser(application.platform);
  const [params, setParams] = useSearchParams();

  const by = (FIELDS as readonly string[]).includes(params.get('page') ?? '') ? (params.get('page') as string) : browser ? FIELDS[0] : 'view.name';
  // `pivot` in the address: `anchor` there is the moment the shell pins, which the shell writes back after the page takes it out.
  const anchor = params.get('pivot') ?? '';
  const backward = params.get('dir') === 'before';
  const depth = Math.min(6, Math.max(1, Number(params.get('depth')) || DEFAULT_DEPTH));

  const change = React.useCallback(
    (changes: Record<string, string | null>) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);

          for (const [name, value] of Object.entries(changes)) {
            if (value) next.set(name, value);
            else next.delete(name);
          }

          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  const request = React.useMemo<PathsParams>(
    () => ({
      from: range.from,
      to: range.to,
      filters: scopeFilters(scope),
      ...(anchor && { anchor }),
      backward,
      depth,
      by,
    }),
    [anchor, backward, by, depth, range, scope],
  );

  const read = usePaths(request);
  const suggestions = useSuggestions('views', by, scope, range);
  const pages = (suggestions.data?.values ?? []).map((entry) => entry.value).filter((value) => value !== '');
  const nobody = read.isSuccess && !read.data?.unavailable && read.data.sessions === 0;

  return (
    <div data-rum-analytics="paths" className="space-y-[12px]">
      <OverviewCard
        icon={GitBranch}
        title={t('analytics.paths.title')}
        description={t(backward ? 'analytics.paths.description_before' : 'analytics.paths.description_after')}
        testId="paths"
        tone="text-[var(--chart-5)]"
      >
        <div className="flex flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-2">
          <FittedSelect
            aria-label={t('analytics.paths.direction')}
            data-rum-paths-direction=""
            value={backward ? 'before' : 'after'}
            onValueChange={(value) => change({ dir: value === 'before' ? 'before' : null })}
            options={[
              { value: 'after', label: t('analytics.paths.after') },
              { value: 'before', label: t('analytics.paths.before') },
            ]}
            maxWidth={200}
            className={cn(CONTROL, 'font-strong text-tx-1')}
          />
          <ValueCombobox
            label={t('analytics.paths.anchor')}
            value={anchor}
            onChange={(value) => change({ pivot: value || null })}
            suggestions={pages}
            placeholder={t(backward ? 'analytics.paths.anchor_last' : 'analytics.paths.anchor_first')}
            className="w-[260px] max-w-full"
          />
          {anchor && (
            <button
              type="button"
              data-rum-paths-clear=""
              onClick={() => change({ pivot: null })}
              className="inline-flex h-8 items-center rounded-md border-0 bg-transparent px-2 font-sans text-xs text-tx-2 outline-none hover:text-tx-0 focus-visible:text-tx-0"
            >
              {t('analytics.paths.clear')}
            </button>
          )}
          <FittedSelect
            aria-label={t('analytics.paths.depth')}
            data-rum-paths-depth=""
            value={String(depth)}
            onValueChange={(value) => change({ depth: Number(value) === DEFAULT_DEPTH ? null : value })}
            options={DEPTHS.map((count) => ({ value: String(count), label: t('analytics.paths.depth_pages', { count }) }))}
            maxWidth={160}
            className={cn(CONTROL, 'text-tx-1')}
          />
          {browser && (
            <FittedSelect
              aria-label={t('analytics.paths.told_by')}
              data-rum-paths-by=""
              value={by}
              onValueChange={(value) => change({ page: value === FIELDS[0] ? null : value, pivot: null })}
              options={FIELDS.map((field) => ({ value: field, label: t(`analytics.paths.by.${field.replaceAll('.', '_')}`) }))}
              maxWidth={220}
              className={cn(CONTROL, 'text-tx-1')}
            />
          )}
        </div>
        <CardState query={read} empty={nobody} emptyTitle={t('analytics.paths.nobody')}>
          {read.data && <PathsChart paths={read.data} anchor={anchor} onPick={(page) => change({ pivot: page })} language={language} />}
        </CardState>
        {nobody && (
          <div className="flex justify-center pb-4">
            <ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>
          </div>
        )}
      </OverviewCard>
    </div>
  );
}
