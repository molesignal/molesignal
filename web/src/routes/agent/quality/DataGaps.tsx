import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as qualityApi from '@/api/agent/quality';
import type { DataGaps, GapExample, GapGroup, GapKind } from '@/api/agent/quality';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { Facts, type Fact } from '@/shell/readout/Facts';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { formatRelativeMicros } from '@/time/relative';

import { AnswerDrawer } from './AnswerDrawer';
import { gapFix, gapNote, gapShare } from './gapsModel';
import { QUALITY_DAYS } from './model';
import { QUALITY_KEY, useQualityFilter, useScoringSettings } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

/**
 * What the tool calls of a period's answers say is missing from the data:
 * where it is (the same stream queried for nothing, the structure looked up
 * again and again), one name for a service across signals, and tools that sum
 * data up. Grouped by stream and service, with the answers that found each;
 * personal chats only count in the totals.
 */
export function QualityDataGaps() {
  const { t } = useTranslation('agent');
  const filter = useQualityFilter();
  const { threshold } = useScoringSettings();
  const [open, setOpen] = React.useState<GapExample | null>(null);
  const gaps = useQuery({
    queryKey: [...QUALITY_KEY, 'gaps', filter.days],
    queryFn: () => qualityApi.dataGaps(filter.days),
    retry: false,
  });

  const period = (
    <FittedSelect
      aria-label={t('quality.filters.days')}
      value={String(filter.days)}
      onValueChange={(value) => filter.setDays(Number(value))}
      options={QUALITY_DAYS.map((choice) => ({
        value: String(choice),
        label: t('quality.filters.last_days', { count: choice }),
      }))}
      maxWidth={200}
    />
  );

  return (
    <ModulePage title={t('quality.views.gaps')} description={t('quality.gaps.description')} action={period}>
      {gaps.isPending ? (
        <ProductState variant="loading" />
      ) : gaps.isError ? (
        <ProductState variant="error" error={gaps.error} />
      ) : gaps.data.gaps.answers === 0 ? (
        <EmptyTab title={t('quality.gaps.empty_title')} hint={t('quality.gaps.empty_hint')} />
      ) : (
        <div data-quality-gaps className="flex flex-col gap-6">
          <Totals gaps={gaps.data.gaps} />
          {gaps.data.gaps.groups.length === 0 ? (
            <p className="m-0 font-sans text-xs text-tx-2">{t('quality.gaps.none_listed')}</p>
          ) : (
            qualityApi.GAP_KINDS.map((kind) => (
              <KindSection
                key={kind}
                kind={kind}
                groups={gaps.data.gaps.groups.filter((group) => group.kind === kind)}
                onOpen={setOpen}
              />
            ))
          )}
        </div>
      )}
      <AnswerDrawer
        answer={open ? { targetKind: 'chat_answer', targetId: open.targetId, question: open.question } : null}
        threshold={threshold}
        onClose={() => setOpen(null)}
      />
    </ModulePage>
  );
}

function Totals({ gaps }: { gaps: DataGaps }) {
  const { t } = useTranslation('agent');
  const share = gapShare(gaps.withGaps, gaps.answers);

  const facts: Fact[] = [
    { key: 'answers', label: t('quality.gaps.totals.answers'), value: gaps.answers.toLocaleString() },
    {
      key: 'with-gaps',
      label: t('quality.gaps.totals.with_gaps'),
      value: share === null ? '—' : `${gaps.withGaps.toLocaleString()} · ${share}%`,
      tone: gaps.withGaps > 0 ? 'warning' : 'good',
    },
    ...gaps.kinds.map((count) => ({
      key: count.kind,
      label: t(`quality.gaps.kinds.${count.kind}.title`),
      value: count.answers.toLocaleString(),
      hint: t(`quality.gaps.kinds.${count.kind}.hint`),
    })),
  ];

  return <Facts label={t('quality.gaps.totals.label')} facts={facts} data-quality-gap-totals="" />;
}

function KindSection({
  kind,
  groups,
  onOpen,
}: {
  kind: GapKind;
  groups: readonly GapGroup[];
  onOpen: (example: GapExample) => void;
}) {
  const { t, i18n } = useTranslation('agent');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const nowMicros = Date.now() * 1000;

  if (groups.length === 0) return null;

  return (
    <section aria-labelledby={`quality-gaps-${kind}`} data-quality-gap-kind={kind}>
      <h3 id={`quality-gaps-${kind}`} className={`m-0 ${uiSectionTitleClass}`}>
        {t(`quality.gaps.kinds.${kind}.title`)}
      </h3>
      <p className="m-0 mt-1 font-sans text-xs text-tx-2">{gapFix(t, kind)}</p>
      <ul className="m-0 mt-2 flex list-none flex-col gap-2 p-0">
        {groups.map((group) => (
          <li
            key={`${group.rule}:${group.stream ?? ''}:${group.service ?? ''}`}
            data-quality-gap={`${group.rule}:${group.stream ?? ''}`}
            className="rounded-md bg-bg-2 px-3 py-2 font-sans text-xs"
          >
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <span className="min-w-0 flex-1 text-tx-0">{gapNote(t, group)}</span>
              <span className="tabular-nums text-tx-2">{t('quality.gaps.answers', { count: group.answers })}</span>
              <span className="text-tx-3" title={formatMicrosActive(group.lastAt)}>
                {formatRelativeMicros(group.lastAt, locale, nowMicros)}
              </span>
            </div>
            {group.examples.length > 0 && (
              <div className="mt-1.5 flex min-w-0 flex-wrap gap-x-3 gap-y-1">
                {group.examples.map((example) => (
                  <button
                    key={example.targetId}
                    type="button"
                    data-quality-gap-example={example.targetId}
                    title={example.question}
                    onClick={() => onOpen(example)}
                    className="max-w-[320px] truncate text-left text-[var(--blue-ink)] hover:text-tx-0"
                  >
                    {example.question || t('quality.answers.no_question')}
                  </button>
                ))}
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
