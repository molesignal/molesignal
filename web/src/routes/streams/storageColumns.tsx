import type { TFunction } from 'i18next';

import type { DataTableColumn } from '@/admin';

import type { DisplayStream } from './model';

function formatBytes(value: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const unit = Math.min(Math.floor(Math.log2(Math.max(value, 1)) / 10), units.length - 1);
  return `${(value / 1024 ** unit).toLocaleString(undefined, { maximumFractionDigits: unit === 0 ? 0 : 2 })} ${units[unit]}`;
}

/** Persisted inventory totals, independent of the health observation window. */
export function streamStorageColumns(t: TFunction<'streams'>): DataTableColumn<DisplayStream>[] {
  return ([
    ['total_rows', 'data_count', false],
    ['collected_bytes', 'collected_size', true],
    ['current_stored_bytes', 'stored_size', true],
    ['index_bytes', 'index_size', true],
  ] as const).map(([field, label, bytes]) => ({
    key: field,
    header: t(`list.columns.${label}`),
    width: 144,
    cell: (stream) => {
      const value = stream.runtime?.stats_available ? stream.runtime[field] : null;
      const partial = field === 'collected_bytes' && value != null && !stream.runtime?.collected_bytes_complete;
      return (
        <span
          className="font-mono text-xs tabular-nums text-tx-1"
          title={t(`list.storage_notes.${label}`)}
        >
          {partial ? '≥ ' : ''}
          {value == null || !Number.isFinite(value) ? '—' : bytes ? formatBytes(value) : value.toLocaleString()}
        </span>
      );
    },
  }));
}
