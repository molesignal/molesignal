import { APM_ANOMALY_METRICS, APM_METRICS } from './apmSource';
import { ApmSourceFields } from './ApmSourceFields';
import { RUM_ANOMALY_METRICS, RUM_METRICS } from './rumSource';
import { RumSourceFields } from './RumSourceFields';
import type { TypedSource } from './typedSource';

/**
 * The fields that say which figure of APM or of RUM a rule watches. The window a RUM rule is
 * evaluated over is part of its fields; an APM rule's is a field of the editor, which differs
 * between the rule and the anomaly editor.
 */
export function SourceFields({
  source,
  onChange,
  periodSecs,
  onPeriodChange,
  anomaly = false,
}: {
  source: TypedSource;
  onChange: (next: TypedSource) => void;
  periodSecs: number;
  onPeriodChange: (secs: number) => void;
  /** The figures offered are the ones watched against the days before. */
  anomaly?: boolean;
}) {
  if (source.kind === 'apm') {
    return (
      <ApmSourceFields
        draft={source.query}
        onChange={(query) => onChange({ kind: 'apm', query })}
        metrics={anomaly ? APM_ANOMALY_METRICS : APM_METRICS}
      />
    );
  }

  return (
    <RumSourceFields
      draft={source.query}
      onChange={(query) => onChange({ kind: 'rum', query })}
      periodSecs={periodSecs}
      onPeriodChange={onPeriodChange}
      metrics={anomaly ? RUM_ANOMALY_METRICS : RUM_METRICS}
    />
  );
}
