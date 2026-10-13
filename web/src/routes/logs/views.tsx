import { useTranslation } from 'react-i18next';

import { ViewTabs } from '@/shell/tabs/ViewTabs';

/**
 * The ways the results of a log query are looked at: the logs one by one, what
 * they add up to, what the model makes of them, and the kinds of message they
 * are. One query, one time range and one field list serve all four.
 */
export const LOG_RESULT_VIEWS = ['raw', 'stats', 'inspection', 'patterns'] as const;
export type LogResultView = (typeof LOG_RESULT_VIEWS)[number];

interface LogResultViewTabsProps {
  value: LogResultView;
  onChange: (view: LogResultView) => void;
}

/** Which view of the results is shown, at the head of the results. */
export function LogResultViewTabs({ value, onChange }: LogResultViewTabsProps) {
  const { t } = useTranslation('logs');

  return (
    <ViewTabs
      label={t('explore.views.aria')}
      value={value}
      onValueChange={onChange}
      items={LOG_RESULT_VIEWS.map((view) => ({ key: view, label: t(`explore.views.${view}`) }))}
      dataAttributes={{ 'data-log-result-views': value }}
    />
  );
}
