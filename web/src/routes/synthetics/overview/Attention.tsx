import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ProbeLocation } from '@/api/synthetics';
import { formatRelativeMicros } from '@/time/relative';

import { SyntheticsSection } from '../Frame';
import { CheckStateMark, RunStrip } from '../marks';
import { STATE_COLOR, type CheckAttention } from '../summary';

/**
 * The checks that are not passing, each with where it fails, what it last said
 * went wrong, and since when: what the statement above counts, by name.
 */
export function ChecksNeedingAttention({
  items,
  locations,
  nowMicros,
}: {
  items: readonly CheckAttention[];
  locations: readonly ProbeLocation[];
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const locationName = (id: string) =>
    locations.find((location) => location.id === id)?.name ?? id;

  return (
    <SyntheticsSection title={t('attention.title')}>
      <ul className="divide-y divide-bd-0 border-t border-bd-0">
        {items.map(({ row, locationIds, error, since }) => {
          const facts = [
            t(`kinds.${row.monitor.kind}`),
            locationIds.length > 0 &&
              t('attention.from_locations', {
                names: locationIds.map(locationName).join(', '),
              }),
            since !== undefined &&
              t('attention.since', {
                when: formatRelativeMicros(since, i18n.language, nowMicros),
              }),
          ].filter(Boolean);

          return (
            <li key={row.monitor.id}>
              <Link
                to={`/synthetics/checks/${encodeURIComponent(row.monitor.id)}`}
                aria-label={t('attention.open', { name: row.monitor.name })}
                className="block px-4 py-[12px] outline-none transition-colors hover:bg-bg-2 focus-visible:bg-bg-2"
              >
                <div className="flex min-w-0 items-center gap-[10px]">
                  <span
                    aria-hidden="true"
                    className="h-[8px] w-[8px] shrink-0 rounded-full"
                    style={{ background: STATE_COLOR[row.monitor.state] }}
                  />
                  <span className="min-w-0 flex-1 truncate font-sans text-sm font-strong text-tx-0">
                    {row.monitor.name}
                  </span>
                  <RunStrip results={row.results} className="hidden shrink-0 sm:inline-flex" />
                  <span className="w-[84px] shrink-0">
                    <CheckStateMark state={row.monitor.state} />
                  </span>
                </div>
                <p className="mt-[4px] truncate pl-[18px] font-sans text-xs text-tx-2">
                  {facts.join(' · ')}
                </p>
                {/* What the check last said went wrong, when it said anything. */}
                {error && (
                  <p className="mt-[6px] line-clamp-2 text-pretty pl-[18px] font-sans text-xs leading-relaxed text-tx-1">
                    {error}
                  </p>
                )}
              </Link>
            </li>
          );
        })}
      </ul>
    </SyntheticsSection>
  );
}
