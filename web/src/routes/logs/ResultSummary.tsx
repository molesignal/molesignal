import {
  BarChart3,
  Download,
  FileText,
  RefreshCw,
  Table2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { QueryIconAction } from '@/shell/query/IconAction';
import { ModuleToggle } from '@/shell/query/ModuleToggle';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { LogColumnMenu, type LogColumnDropPosition } from './ColumnMenu';

interface LogResultSummaryProps {
  eventCount: number;
  tookMs: number;
  timeWindowLabel: string;
  pending: boolean;
  fields: string[];
  visibleFields: string[];
  onToggleField: (field: string) => void;
  onReorderField: (source: string, target: string, position: LogColumnDropPosition) => void;
  /** The columns are the reader's own arrangement for this stream. */
  columnsArranged?: boolean | undefined;
  onResetColumns?: (() => void) | undefined;
  histogramOpen: boolean;
  onHistogramOpenChange: (open: boolean) => void;
  onDownloadCsv: () => void;
  onDownloadLog: () => void;
  /** Which view of the results is shown: the switch stands first in the row. */
  views?: React.ReactNode;
  /** Whether the rows are shown one by one: only then do columns and row height mean anything. */
  listControls?: boolean;
  /**
   * Whether the view is of the loaded results: their count, their histogram
   * and their download are said for it. A view that works out its own answer
   * (statistics, an inspection) has none of the three.
   */
  resultControls?: boolean;
}

/**
 * The row above the log rows: which view of the results is shown and what the
 * query returned on the left, how the result is shown on the right. What
 * stands there is said by signs alone, as in the bar above: the columns, the
 * switch that opens and closes the histogram over the rows, the download. The
 * field list beside the rows folds from its own head.
 */
export function LogResultSummary({
  eventCount,
  tookMs,
  timeWindowLabel,
  pending,
  fields,
  visibleFields,
  onToggleField,
  onReorderField,
  columnsArranged,
  onResetColumns,
  histogramOpen,
  onHistogramOpenChange,
  onDownloadCsv,
  onDownloadLog,
  views,
  listControls = true,
  resultControls = true,
}: LogResultSummaryProps) {
  const { t } = useTranslation('logs');

  return (
    <div
      data-log-result-summary
      className="flex min-h-11 flex-wrap items-center gap-2 px-4 py-1.5 font-sans text-xs"
    >
      {views}
      {resultControls && (
        <>
          <span className="h-1.5 w-1.5 rounded-full bg-orange" />
          <span className="font-semibold text-tx-0">
            {t('explore.table.events_summary', {
              events: eventCount.toLocaleString(),
              ms: tookMs,
            })}
          </span>
          <span className="text-tx-3">·</span>
          <span className="whitespace-nowrap text-tx-2">{timeWindowLabel}</span>
          <RefreshCw className={`h-3 w-3 text-tx-3 ${pending ? 'animate-spin' : ''}`} />
        </>
      )}
      <div data-log-result-actions className="ml-auto flex items-center gap-1">
        {listControls && (
          <LogColumnMenu
            fields={fields}
            visibleFields={visibleFields}
            onToggleField={onToggleField}
            onReorderField={onReorderField}
            arranged={columnsArranged}
            onReset={onResetColumns}
          />
        )}
        {resultControls && (
          <ModuleToggle
            open={histogramOpen}
            icon={BarChart3}
            actionLabel={histogramOpen ? t('explore.histogram.hide') : t('explore.histogram.show')}
            onOpenChange={onHistogramOpenChange}
          />
        )}
        {resultControls && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <QueryIconAction
                icon={Download}
                label={t('explore.results.download')}
                disabled={eventCount === 0}
              />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-52">
              <DropdownMenuLabel>{t('explore.results.download')}</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={onDownloadCsv}>
                <Table2 className="h-3.5 w-3.5" />
                {t('explore.results.download_csv')}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onDownloadLog}>
                <FileText className="h-3.5 w-3.5" />
                {t('explore.results.download_log')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </div>
  );
}
