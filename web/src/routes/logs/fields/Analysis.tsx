import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as webApi from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { analyzeLoadedValues } from '@/shell/query/fieldAnalysis/model';
import { FieldAnalysisPanel } from '@/shell/query/fieldAnalysis/Panel';
import {
  answerFromPayload,
  type FieldAnalysisSource,
  type FieldAnalysisState,
  useFieldAnalysis,
} from '@/shell/query/fieldAnalysis/source';
import type { HistogramRange } from '@/shell/query/histogram/model';

import { logMatchRequest } from '../cursorQuery';
import type { LogFieldDef } from '../fieldQueryModel';
import type { CountedLogQuery } from '../histogramCounts';
import { logFieldAnalysisKind } from './analysisKind';
import { LOG_FIELD_TYPE_GLYPH, LOG_FIELD_TYPE_TONE } from './Row';

interface LogFieldAnalysisProps {
  field: LogFieldDef;
  /**
   * The field query the rows came from, as it was run: the field is read in
   * everything it matches. A statement the reader wrote has none (`null`), and
   * its field is analysed in the rows it returned.
   */
  counted: CountedLogQuery | null;
  /** The time range the query was run over. */
  range: HistogramRange | null;
  /** The loaded records. */
  records: Record<string, unknown>[];
  /** When each loaded record was written, in microseconds, in their order. */
  moments: ReadonlyArray<number | null>;
  /** The field is masked for this reader: what is loaded of it is masks. */
  masked: boolean;
  onClose: () => void;
}

const PROTECTED: FieldAnalysisState = {
  analysis: null,
  scope: 'loaded',
  pending: false,
  unavailable: 'field_protected',
  retry: () => undefined,
};

/**
 * What a log field holds across the results, beside the field list. It is read
 * with the request the list itself sends, so the numbers are of the rows the
 * list pages through, every one of them.
 */
export function LogFieldAnalysis({ field, counted, range, records, moments, masked, onClose }: LogFieldAnalysisProps) {
  const { t } = useTranslation('logs');
  const format = useTimeFormatter();
  const kind = logFieldAnalysisKind(field) ?? 'text';
  const source = React.useMemo<FieldAnalysisSource | undefined>(() => {
    if (!counted || !range || masked) return undefined;

    const { orgId, stream, statement, globalFilters, ranAt } = counted;

    return {
      key: ['logs', orgId, stream, statement, JSON.stringify(globalFilters), ranAt, range.start, range.end, field.name],
      load: async (signal) =>
        answerFromPayload(
          await webApi.logsFieldStats(
            { ...logMatchRequest(stream, statement, globalFilters, range), field: field.name },
            signal,
          ),
        ),
    };
  }, [counted, field.name, masked, range]);
  const loaded = React.useCallback(
    () =>
      masked
        ? null
        : analyzeLoadedValues(
            records.map((record) => record[field.name]),
            kind,
            range ? { micros: moments, range } : undefined,
          ),
    [field.name, kind, masked, moments, range, records],
  );
  const read = useFieldAnalysis(source, loaded);
  const state = masked ? PROTECTED : read;

  return (
    <FieldAnalysisPanel
      name={field.name}
      chip={{ glyph: LOG_FIELD_TYPE_GLYPH[field.type], tone: LOG_FIELD_TYPE_TONE[field.type] }}
      kind={kind}
      subject="logs"
      state={state}
      scope={
        state.scope === 'all'
          ? t('explore.field_analysis.scope_all')
          : t('explore.field_analysis.scope_loaded', { count: records.length })
      }
      // A moment is stored as a count of microseconds, and read as a time.
      describe={kind === 'time' ? (value) => format.micros(Number(value)) : undefined}
      onClose={onClose}
    />
  );
}
