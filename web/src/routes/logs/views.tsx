import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useQueryTabSearchParams } from '@/shell/queryTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

/**
 * The ways the results of a log query are looked at: the logs one by one, what
 * they add up to, what the model makes of them, and the kinds of message they
 * are. One query, one time range and one field list serve all four.
 */
export const LOG_RESULT_VIEWS = ['raw', 'stats', 'inspection', 'patterns'] as const;
export type LogResultView = (typeof LOG_RESULT_VIEWS)[number];

/**
 * The view the address names (`view`), so a copy of the address opens the
 * results the way they were looked at; the logs themselves when it names none.
 */
export function useLogResultView(): [LogResultView, (view: LogResultView) => void] {
  const [params, setParams] = useQueryTabSearchParams();
  const named = params.get('view');
  const view = LOG_RESULT_VIEWS.find((candidate) => candidate === named) ?? 'raw';
  const setView = React.useCallback(
    (next: LogResultView) => {
      setParams(
        (current) => {
          const updated = new URLSearchParams(current);
          if (next === 'raw') updated.delete('view');
          else updated.set('view', next);
          return updated;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  return [view, setView];
}

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
