import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as webApi from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { analyzeLoadedValues, type FieldAnalysisKind } from '@/shell/query/fieldAnalysis/model';
import { FieldAnalysisPanel } from '@/shell/query/fieldAnalysis/Panel';
import { answerFromPayload, type FieldAnalysisSource, useFieldAnalysis } from '@/shell/query/fieldAnalysis/source';

import type { TraceFieldDef } from '../fieldQueryModel';
import {
  describeTraceValue,
  hasTraceLevelValues,
  isTraceTimeKind,
  TRACE_TYPE_GLYPH,
  TRACE_TYPE_TONE,
  traceLevelValue,
  traceValueKind,
  type TraceFieldRecord,
  type TraceValueKind,
} from './model';
import { loadedTraceWindow, type TraceFieldValueSource } from './valuesQuery';

interface TraceFieldAnalysisProps {
  field: TraceFieldDef;
  /** The traces on the page, each once. */
  traces: readonly TraceFieldRecord[];
  /** Where the spans of those traces are read from. */
  source: TraceFieldValueSource;
  onClose: () => void;
}

/**
 * How a field's values are read. What its name says comes before how it is
 * stored: a count of nanoseconds since the epoch is a moment, and the kind of a
 * span is a name, though both are stored as whole numbers.
 */
export function traceFieldAnalysisKind(field: Pick<TraceFieldDef, 'dataType'>, valueKind: TraceValueKind): FieldAnalysisKind {
  if (isTraceTimeKind(valueKind)) return 'time';
  if (valueKind === 'span_kind') return 'text';
  if (field.dataType === 'int64' || field.dataType === 'float64') return 'number';

  return field.dataType === 'bool' ? 'bool' : 'text';
}

/**
 * What a field holds across the traces on the page, beside the field list.
 *
 * The list has one row per trace. A field a trace has one value for (its id,
 * its duration, its two counts) is analysed on those rows. A field of a span
 * is not on them: it is read in every span of the listed traces.
 */
export function TraceFieldAnalysis({ field, traces, source, onClose }: TraceFieldAnalysisProps) {
  const { t } = useTranslation('traces');
  const format = useTimeFormatter();
  const valueKind = traceValueKind(field);
  const kind = traceFieldAnalysisKind(field, valueKind);
  const onRows = hasTraceLevelValues(field);
  const spans = React.useMemo<FieldAnalysisSource | undefined>(() => {
    const ids = traces.map((trace) => trace.id).filter(Boolean);
    if (onRows || ids.length === 0 || !source.orgId || !source.stream) return undefined;

    // The stretch of time the listed traces cover, and not the whole range that was asked for.
    const window = loadedTraceWindow(traces) ?? source.range;

    return {
      key: ['traces', source.orgId, source.stream, field.name, field.dataType, ids],
      load: async (signal) =>
        answerFromPayload(
          await webApi.tracesFieldStats(
            { stream: source.stream, from: window.start, to: window.end, trace_ids: ids, field: field.name },
            signal,
          ),
        ),
    };
  }, [field.dataType, field.name, onRows, source.orgId, source.range, source.stream, traces]);
  // The rows of the list hold nothing of a span: there is no analysis of a span's field to fall back on.
  const loaded = React.useCallback(() => {
    if (!onRows) return null;

    return analyzeLoadedValues(
      traces.map((trace) => traceLevelValue(trace, field.name)),
      kind,
      // A trace is placed in time by when it began.
      {
        micros: traces.map((trace) => (trace.startNs > 0 ? Math.floor(trace.startNs / 1_000) : null)),
        range: loadedTraceWindow(traces) ?? source.range,
      },
    );
  }, [field.name, kind, onRows, source.range, traces]);
  const state = useFieldAnalysis(spans, loaded);

  return (
    <FieldAnalysisPanel
      name={field.name}
      chip={{ glyph: TRACE_TYPE_GLYPH[field.dataType], tone: TRACE_TYPE_TONE[field.dataType] }}
      kind={kind}
      subject={onRows ? 'traces' : 'spans'}
      state={state}
      scope={t(onRows ? 'explore.labels.analysis_scope_traces' : 'explore.labels.analysis_scope_spans', {
        count: traces.length,
      })}
      // A duration is read in its natural unit and a moment as a time, as in the values under the row.
      describe={
        valueKind === 'plain'
          ? undefined
          : (value) => describeTraceValue(valueKind, value, (micros) => format.micros(micros))
      }
      onClose={onClose}
    />
  );
}
