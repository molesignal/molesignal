import { Blocks, CodeXml } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { QueryToolbarButton, QueryToolbarGroup } from './Workbench';

/**
 * How a query is written, on every explorer: `code` is the statement in an
 * editor, `builder` is the same query put together from controls.
 *
 * The switch is two icons and nothing else (`</>` for the statement, blocks for
 * the builder), each naming itself in a tooltip and to assistive technology: it
 * shares the workbench bar with the page's tabs and actions, and every pixel it
 * takes is one the folded-up query does not get.
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
  const options = [
    { value: 'code', icon: CodeXml, label: t('query_mode.code') },
    { value: 'builder', icon: Blocks, label: t('query_mode.builder') },
  ] as const;

  return (
    <QueryToolbarGroup
      role="group"
      aria-label={t('query_mode.aria')}
      data-query-mode={mode}
      className="border-0 bg-[var(--control-surface)]"
    >
      {options.map(({ value, icon: Icon, label }) => (
        <Tooltip key={value}>
          <TooltipTrigger asChild>
            <QueryToolbarButton
              active={mode === value}
              aria-pressed={mode === value}
              aria-label={label}
              tone="indigo"
              flat
              className="w-9 px-0"
              onClick={() => onChange(value)}
            >
              <Icon aria-hidden="true" className="h-4 w-4" />
            </QueryToolbarButton>
          </TooltipTrigger>
          <TooltipContent>{label}</TooltipContent>
        </Tooltip>
      ))}
    </QueryToolbarGroup>
  );
}
