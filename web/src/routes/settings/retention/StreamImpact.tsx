import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn, MetadataStrip } from '@/admin';
import type { StreamRetentionImpact, StreamRetentionPreview } from '@/api/instance';
import { useTimeFormatter } from '@/lib/time';
import { formatBytes, formatCount } from '@/routes/home/canvas/format';
import { ChromeButton } from '@/shell/chrome';

import { ImpactFailure, ImpactStatus } from './ImpactStatus';
import type { RetentionPreviewState } from './model';

/** Streams listed before the rest are asked for. The largest come first. */
const FIRST_LISTED = 10;

/** The totals of an estimate, one fact to a row: above the list, and again in the confirmation. */
export function useStreamImpactFacts(preview: StreamRetentionPreview | null) {
  const { t } = useTranslation('settings-admin');
  const time = useTimeFormatter();

  if (!preview) return undefined;

  return [
    { label: t('stream_retention.preview.streams'), value: formatCount(preview.streamCount) },
    { label: t('stream_retention.preview.rows'), value: formatCount(preview.rows) },
    { label: t('stream_retention.preview.size'), value: formatBytes(preview.storedBytes) },
    { label: t('stream_retention.preview.earliest'), value: time.micros(preview.earliestAt, false) },
    ...(preview.unavailableStreams > 0
      ? [
          {
            label: t('stream_retention.preview.unread'),
            value: t('stream_retention.preview.unavailable', { count: preview.unavailableStreams }),
          },
        ]
      : []),
  ];
}

/**
 * Under the days to keep while a shorter retention is typed: what it would delete, stream by
 * stream. It is an estimate from the file catalog, and it says so.
 */
export function StreamImpact({ impact }: { impact: RetentionPreviewState<StreamRetentionPreview> }) {
  const { t } = useTranslation('settings-admin');

  if (impact.status === 'idle') return null;

  return (
    <div data-retention-preview className="mt-4 flex flex-col gap-2">
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <h3 className="font-sans text-sm font-strong text-tx-0">{t('stream_retention.preview.title')}</h3>
        {impact.status === 'ready' && (
          <span className="font-sans text-xs text-tx-3">{t('stream_retention.preview.estimated')}</span>
        )}
      </div>
      {impact.status === 'pending' && <ImpactStatus>{t('stream_retention.preview.estimating')}</ImpactStatus>}
      {impact.status === 'failed' && (
        <ImpactFailure onRetry={impact.retry}>{t('stream_retention.preview.failed')}</ImpactFailure>
      )}
      {impact.status === 'ready' && <StreamImpactFigures preview={impact.preview} />}
    </div>
  );
}

function StreamImpactFigures({ preview }: { preview: StreamRetentionPreview }) {
  const { t } = useTranslation('settings-admin');
  const facts = useStreamImpactFacts(preview);
  const columns = useImpactColumns(preview.streams);
  const [showAll, setShowAll] = React.useState(false);

  if (preview.streamCount === 0 && preview.unavailableStreams === 0) {
    return <ImpactStatus>{t('stream_retention.preview.nothing')}</ImpactStatus>;
  }

  const listed = showAll ? preview.streams : preview.streams.slice(0, FIRST_LISTED);

  return (
    <>
      <MetadataStrip items={facts} />
      {listed.length > 0 && (
        <DataTable
          rows={listed}
          columns={columns}
          rowKey={(stream) => stream.id}
          fixedMinWidth={columns.length > 5 ? 720 : 560}
        />
      )}
      {listed.length < preview.streamCount && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <p className="font-sans text-xs text-tx-2">
            {t('stream_retention.preview.showing', { shown: listed.length, total: preview.streamCount })}
          </p>
          {listed.length < preview.streams.length && (
            <ChromeButton variant="ghost" onClick={() => setShowAll(true)}>
              {t('stream_retention.preview.show_all')}
            </ChromeButton>
          )}
        </div>
      )}
      <p className="font-sans text-xs text-tx-3">{t('stream_retention.preview.not_counted')}</p>
    </>
  );
}

function useImpactColumns(streams: readonly StreamRetentionImpact[]): DataTableColumn<StreamRetentionImpact>[] {
  const { t } = useTranslation('settings-admin');
  const { t: ts } = useTranslation('streams');
  const time = useTimeFormatter();

  // One organization need not be named on every row.
  const severalOrganizations = new Set(streams.map((stream) => stream.orgId)).size > 1;

  return [
    {
      key: 'name',
      header: t('stream_retention.preview.column_stream'),
      cell: (stream) => (
        <span title={stream.name} className="block truncate text-tx-0">
          {stream.name}
        </span>
      ),
    },
    ...(severalOrganizations
      ? [
          {
            key: 'organization',
            header: t('stream_retention.preview.column_organization'),
            width: 160,
            cell: (stream: StreamRetentionImpact) => (
              <span title={stream.orgName} className="block truncate">
                {stream.orgName}
              </span>
            ),
          },
        ]
      : []),
    {
      key: 'type',
      header: t('stream_retention.preview.column_type'),
      width: 88,
      cell: (stream) => ts(`list.tabs.${stream.streamType}`),
    },
    {
      key: 'rows',
      header: t('stream_retention.preview.rows'),
      width: 88,
      className: 'text-right',
      headerClassName: 'text-right',
      cell: (stream) => formatCount(stream.rows),
    },
    {
      key: 'size',
      header: t('stream_retention.preview.size'),
      width: 96,
      className: 'text-right',
      headerClassName: 'text-right',
      cell: (stream) => formatBytes(stream.storedBytes),
    },
    {
      key: 'earliest',
      header: t('stream_retention.preview.earliest'),
      width: 144,
      cell: (stream) => time.micros(stream.earliestAt, false),
    },
  ];
}
