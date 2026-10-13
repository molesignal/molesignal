import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import type { DisplayReport } from '../reportTypes';
import { changesHandsOnSave } from './model';
import { useRunAs } from './useRunAs';

/**
 * Enabling or pausing a report saves it, and a save makes the saver the user the report runs
 * as. Where that changes whose access the report reads data with, the switch says so before
 * it is used: in a tooltip, and as the description of the switch itself.
 */
export function RunAsToggleHint({
  report,
  disabled,
  children,
}: {
  report: DisplayReport;
  /** A switch that cannot be used explains itself; it changes nothing. */
  disabled: boolean;
  /** Renders the switch, described by the element with the given id when there is a hint. */
  children: (describedBy: string | undefined) => React.ReactElement;
}) {
  const { t } = useTranslation('reports');
  const hintId = React.useId();
  const runAs = useRunAs(report, report.format);

  if (disabled || !changesHandsOnSave(runAs)) return children(undefined);

  const hint = t('run_as.toggle_hint');

  return (
    <Tooltip>
      {/* The switch keeps its own `data-state`; the tooltip's goes on the wrapper. */}
      <TooltipTrigger asChild>
        <span className="inline-flex shrink-0">
          {children(hintId)}
          <span id={hintId} className="sr-only">
            {hint}
          </span>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" className="max-w-xs">
        {hint}
      </TooltipContent>
    </Tooltip>
  );
}
