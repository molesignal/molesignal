import { useTranslation } from 'react-i18next';

import { QueryToolbarButton, QueryToolbarGroup } from './Workbench';

/**
 * How a query is written, on every explorer: `code` is the statement in an
 * editor, `builder` is the same query put together from controls.
 *
 * The switch is two words and nothing else: it shares the workbench bar with
 * the page's tabs and actions, and every pixel it takes is one the folded-up
 * query does not get.
 */
export type QueryEditorMode = 'code' | 'builder';

export function QueryModeSwitch({
  mode,
  onChange,
}: {
  mode: QueryEditorMode;
  onChange: (mode: QueryEditorMode) => void;
}) {
  const { t } = useTranslation('common');

  return (
    <QueryToolbarGroup
      role="group"
      aria-label={t('query_mode.aria')}
      data-query-mode={mode}
      className="border-0 bg-[var(--control-surface)]"
    >
      <QueryToolbarButton
        active={mode === 'code'}
        aria-pressed={mode === 'code'}
        tone="indigo"
        flat
        onClick={() => onChange('code')}
      >
        {t('query_mode.code')}
      </QueryToolbarButton>
      <QueryToolbarButton
        active={mode === 'builder'}
        aria-pressed={mode === 'builder'}
        tone="indigo"
        flat
        onClick={() => onChange('builder')}
      >
        {t('query_mode.builder')}
      </QueryToolbarButton>
    </QueryToolbarGroup>
  );
}
