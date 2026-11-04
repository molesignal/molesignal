import { useTranslation } from 'react-i18next';

import type { ScoreSource } from '@/api/agent/quality';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { QUALITY_DAYS } from './model';

const SOURCE_CHOICES: ReadonlyArray<ScoreSource | 'all'> = [
  'all',
  'team_chat',
  'automation',
  'incident_rca',
  'personal_chat',
];

/**
 * The period and the source a view looks at. The list of answers leaves
 * personal chats out: they only count in the totals.
 */
export function QualityFilters({
  days,
  source,
  onDays,
  onSource,
  personal = true,
}: {
  days: number;
  source: ScoreSource | undefined;
  onDays: (days: number) => void;
  onSource: (source: ScoreSource | 'all') => void;
  /** Personal chats can be chosen. */
  personal?: boolean;
}) {
  const { t } = useTranslation('agent');

  return (
    <div className="flex flex-wrap items-center gap-2">
      <FittedSelect
        aria-label={t('quality.filters.days')}
        value={String(days)}
        onValueChange={(value) => onDays(Number(value))}
        options={QUALITY_DAYS.map((choice) => ({
          value: String(choice),
          label: t('quality.filters.last_days', { count: choice }),
        }))}
        maxWidth={200}
      />
      <FittedSelect
        aria-label={t('quality.filters.source')}
        value={source && (personal || source !== 'personal_chat') ? source : 'all'}
        onValueChange={(value) => onSource(value as ScoreSource | 'all')}
        options={SOURCE_CHOICES.filter((choice) => personal || choice !== 'personal_chat').map(
          (choice) => ({
            value: choice,
            label: t(`quality.sources.${choice}`),
          }),
        )}
        maxWidth={220}
      />
    </div>
  );
}
