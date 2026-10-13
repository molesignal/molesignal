import { Flame, ListChecks } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { formatCount, type TopFunction } from '@/viz/profiles/flamebearer';

/**
 * What a profile analysis found and what it was merged from, in one line over
 * the flame graph.
 *
 * It opens with the function that costs the most, the reason a reader comes
 * here, and picking it finds it in the graph. The rest is the scope: the
 * profile type and its total, the samples, the services and the time captured.
 * The count of profiles at the end is how the scope is changed: it opens the
 * list the graph is merged from.
 */
export function ProfileSummary({
  typeLabel,
  totalValue,
  sampleCount,
  serviceCount,
  duration,
  selectedCount,
  profileCount,
  hotFunction,
  onSelectHotFunction,
  onOpenProfiles,
}: {
  typeLabel: string;
  /** Already formatted: "31.85 s", "1.2 GiB". `null` before a flame graph is merged. */
  totalValue: string | null;
  sampleCount: number;
  serviceCount: number;
  /** The capture time of the merged profiles, formatted; `null` when none is selected. */
  duration: string | null;
  selectedCount: number;
  profileCount: number;
  hotFunction: TopFunction | null;
  onSelectHotFunction: (name: string) => void;
  onOpenProfiles: () => void;
}) {
  const { t } = useTranslation('profiles');
  const facts = [
    totalValue ? `${typeLabel} ${totalValue}` : typeLabel,
    t('analysis.samples', { count: formatCount(sampleCount) }),
    t('analysis.service_count', { count: serviceCount }),
    ...(duration ? [t('analysis.captured', { duration })] : []),
  ];

  return (
    <section
      data-profile-summary
      aria-label={t('analysis.summary_aria')}
      className="flex min-w-0 flex-wrap items-center gap-x-[12px] gap-y-[6px] rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] font-sans text-xs [box-shadow:var(--shadow-functional-surface)]"
    >
      <button
        type="button"
        data-profile-hottest
        disabled={!hotFunction}
        onClick={() => hotFunction && onSelectHotFunction(hotFunction.name)}
        title={hotFunction?.name}
        className="flex min-w-0 max-w-full items-center gap-[6px] rounded-md px-[6px] py-[4px] text-left enabled:hover:bg-bg-2 enabled:focus-visible:bg-bg-2 disabled:cursor-default"
      >
        <Flame aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-[var(--orange-ink)]" />
        <span className="shrink-0 text-tx-2">{t('analysis.hottest')}</span>
        <span className={cn('min-w-0 truncate font-strong', hotFunction ? 'text-tx-0' : 'text-tx-3')}>
          {hotFunction?.name ?? t('analysis.no_hotspot')}
        </span>
        {hotFunction ? (
          <span className="shrink-0 tabular-nums text-[var(--orange-ink)]">
            {t('analysis.hot_share', { percent: hotFunction.selfPct.toFixed(1) })}
          </span>
        ) : null}
      </button>

      <span data-profile-scope className="flex min-w-0 flex-wrap items-center gap-x-[6px] gap-y-[2px] text-tx-2">
        {facts.map((fact, index) => (
          <span key={fact} className="whitespace-nowrap tabular-nums">
            {index > 0 ? <span aria-hidden="true" className="mr-[6px] text-tx-3">·</span> : null}
            {fact}
          </span>
        ))}
      </span>

      <button
        type="button"
        data-profile-selection-open
        onClick={onOpenProfiles}
        aria-label={t('analysis.profiles_button_aria', { selected: selectedCount, total: profileCount })}
        className="ml-auto flex shrink-0 items-center gap-[6px] rounded-md bg-bg-2 px-[10px] py-[5px] font-strong tabular-nums text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
      >
        <ListChecks aria-hidden="true" className="h-3.5 w-3.5" />
        {t('analysis.profiles_button', { selected: selectedCount, total: profileCount })}
      </button>
    </section>
  );
}
