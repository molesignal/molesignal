import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import {
  type DiffFunction,
  formatChange,
  formatChangePct,
  formatValue,
} from '@/viz/profiles/flamebearer';

/** As many functions as answer "what changed" at a glance; the rest are a press away. */
const TOP = 10;
const ALL = 100;

/**
 * The answer a comparison is opened for: which functions cost more or less of
 * their own than in the baseline, the largest change first, whichever way it
 * went. Picking one marks it in the flame graph under the table.
 */
export function ChangedFunctions({
  rows,
  units,
  selected,
  onSelect,
  className,
}: {
  rows: DiffFunction[];
  units: string;
  selected: string | null;
  onSelect: (name: string) => void;
  className?: string | undefined;
}) {
  const { t } = useTranslation('profiles');
  const [all, setAll] = React.useState(false);
  const shown = rows.slice(0, all ? ALL : TOP);
  const largest = Math.max(1, ...shown.map((row) => Math.abs(row.delta)));

  return (
    <section
      data-compare-changed
      aria-label={t('compare.changed.title')}
      className={cn(
        'overflow-hidden rounded-md bg-[var(--functional-surface)] font-sans [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className="flex min-h-[40px] flex-wrap items-center gap-x-[10px] gap-y-[4px] px-[12px] py-[6px]">
        <h2 className="text-sm font-strong text-tx-0">{t('compare.changed.title')}</h2>
        <span className="text-xs tabular-nums text-tx-2">
          {t('compare.changed.count', { count: rows.length })}
        </span>
        {rows.length > TOP ? (
          <button
            type="button"
            data-compare-changed-more
            onClick={() => setAll((value) => !value)}
            className="ml-auto rounded-md bg-bg-2 px-[10px] py-[5px] text-xs font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
          >
            {all
              ? t('compare.changed.show_top', { count: TOP })
              : t('compare.changed.show_all', { count: Math.min(rows.length, ALL) })}
          </button>
        ) : null}
      </div>

      {rows.length === 0 ? (
        <p className="px-[12px] pb-[12px] text-xs text-tx-2">{t('compare.changed.none')}</p>
      ) : (
        <Table className="text-xs">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              {/* The name takes the room the three measures leave. */}
              <TableHead className="w-full">{t('compare.changed.function')}</TableHead>
              <TableHead className="min-w-[110px] whitespace-nowrap text-right">
                {t('compare.changed.baseline')}
              </TableHead>
              <TableHead className="min-w-[110px] whitespace-nowrap text-right">
                {t('compare.changed.comparison')}
              </TableHead>
              <TableHead className="min-w-[300px] whitespace-nowrap">{t('compare.changed.change')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {shown.map((row) => {
              const more = row.delta > 0;
              const share = (Math.abs(row.delta) / largest) * 50;

              return (
                <TableRow
                  key={row.name}
                  data-compare-function={row.name}
                  data-state={selected === row.name ? 'selected' : undefined}
                  role="button"
                  tabIndex={0}
                  aria-pressed={selected === row.name}
                  title={t('compare.changed.select_hint')}
                  onClick={() => onSelect(row.name)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      onSelect(row.name);
                    }
                  }}
                  className="cursor-pointer focus-visible:bg-bg-3 focus-visible:outline-none"
                >
                  <TableCell className="w-full max-w-0 truncate text-tx-0">{row.name}</TableCell>
                  <TableCell className="whitespace-nowrap text-right text-tx-2">
                    {row.baseline > 0 ? formatValue(row.baseline, units) : '—'}
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-right text-tx-1">
                    {row.comparison > 0 ? formatValue(row.comparison, units) : '—'}
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-[10px]">
                      {/* Less grows to the left of the middle, more to the right of it. */}
                      <span aria-hidden="true" className="relative h-[6px] w-[96px] shrink-0 rounded-full bg-bg-3">
                        <span className="absolute inset-y-[-2px] left-1/2 w-px bg-bd-2" />
                        <span
                          className={cn('absolute inset-y-0 rounded-full', more ? 'bg-red' : 'bg-green')}
                          style={more ? { left: '50%', width: `${share}%` } : { right: '50%', width: `${share}%` }}
                        />
                      </span>
                      <span
                        className={cn(
                          'whitespace-nowrap font-strong',
                          more ? 'text-[var(--red-ink)]' : 'text-[var(--green-ink)]',
                        )}
                      >
                        {formatChange(row.delta, units)}
                      </span>
                      <span className="whitespace-nowrap text-tx-2">
                        {row.deltaPct === null
                          ? t('compare.changed.new')
                          : row.comparison === 0
                            ? t('compare.changed.gone')
                            : formatChangePct(row.deltaPct)}
                      </span>
                    </div>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </section>
  );
}
