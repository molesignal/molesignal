import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { GRADE_TEXT } from './ink';
import type { Level } from './model';
import { formatVital } from '../overview/model';

/** A figure of a sentence, set in the strong step. */
export const FIGURE = <span className="font-display text-tx-0" />;

/** What a level came to, with its grade in the colour of its word; a level no view had says so. */
export function Reading({ level }: { level: Pick<Level, 'metric' | 'value' | 'grade'> }) {
  const { t } = useTranslation('rum');

  if (level.value === null || level.grade === null) {
    return <span className="text-tx-3">{t('performance.statement.not_measured')}</span>;
  }

  return (
    <Trans
      i18nKey="performance.statement.reading"
      ns="rum"
      values={{ value: formatVital(level.metric, level.value), grade: t(`overview.vitals.grade.${level.grade}`) }}
      components={{ figure: FIGURE, grade: <span className={cn('font-strong', GRADE_TEXT[level.grade])} /> }}
    />
  );
}
