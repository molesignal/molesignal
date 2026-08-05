import { useTranslation } from 'react-i18next';

import { QueryState } from '@/shell/query/State';
import type { QueryResult } from '@/types/query';

export function TableView({
  result,
  pending,
  error,
}: {
  result: QueryResult | undefined;
  pending: boolean;
  error: unknown;
}) {
  const { t } = useTranslation('metrics');
  if (error) return <QueryState state="error" error={error} />;
  if (pending && !result) return <QueryState state="loading" />;
  if (!result) {
    return <QueryState state="empty" emptyLabel={t('explore.results.no_table')} />;
  }
  if (result.rows.length === 0) {
    return <QueryState state="empty" emptyLabel={t('explore.chart.empty')} />;
  }

  return (
    <div className="overflow-auto" data-testid="metrics-result-table">
      <table className="w-full min-w-[720px] border-collapse font-mono text-xs">
        <thead className="sticky top-0 z-[1] bg-bg-2">
          <tr>
            {result.columns.map((column) => (
              <th
                key={column}
                className="border-b border-r border-bd-0 px-3 py-2.5 text-left font-semibold text-tx-2 last:border-r-0"
              >
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {result.rows.slice(0, 200).map((row, rowIndex) => (
            <tr key={rowIndex} className="hover:bg-bg-2">
              {result.columns.map((column, columnIndex) => (
                <td
                  key={`${rowIndex}:${column}`}
                  className="max-w-[420px] truncate border-b border-r border-bd-0 px-3 py-2 text-tx-1 last:border-r-0"
                  title={formatQueryCell(row[columnIndex])}
                >
                  {formatQueryCell(row[columnIndex])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {result.rows.length > 200 ? (
        <div className="border-t border-bd-0 px-3 py-2 font-sans text-xs text-tx-3">
          {t('explore.results.table_limit', {
            shown: 200,
            total: result.rows.length,
          })}
        </div>
      ) : null}
    </div>
  );
}

function formatQueryCell(value: unknown): string {
  if (value === null) return 'null';
  if (value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
