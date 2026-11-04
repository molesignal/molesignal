import { Download } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';

import type { DataTableColumn } from '../DataTable';
import { downloadCsv, rowsToCsv, type CsvColumn } from './model';

/** The columns an export can write: those with a plain value and a head in words. */
export function exportableColumns<T>(columns: readonly DataTableColumn<T>[]): CsvColumn<T>[] {
  return columns.flatMap((column) =>
    column.value && typeof column.header === 'string'
      ? [{ header: column.header, value: column.value }]
      : [],
  );
}

function stamp(): string {
  return new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
}

/**
 * Writes the rows of a list as they stand, filtered and in view, to a CSV file
 * named after the list and the moment. The file is made here: nothing is sent.
 */
export function TableExportButton<T>({
  name,
  columns,
  rows,
  size = 'sm',
  className,
}: {
  /** The file's name before its date, in letters and dashes. */
  name: string;
  columns: readonly DataTableColumn<T>[];
  rows: readonly T[];
  /** `md` beside the page's other head buttons. */
  size?: 'sm' | 'md' | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation('common');

  return (
    <ChromeButton
      size={size}
      className={className}
      disabled={rows.length === 0}
      disabledReason={rows.length === 0 ? t('table.export_empty') : undefined}
      onClick={() => downloadCsv(`${name}-${stamp()}.csv`, rowsToCsv(exportableColumns(columns), rows))}
      data-testid="table-export"
    >
      <Download className="h-3.5 w-3.5" />
      {t('table.export_csv')}
    </ChromeButton>
  );
}
